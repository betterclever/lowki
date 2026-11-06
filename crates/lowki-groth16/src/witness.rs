//! Circom witness calculation: runs the circuit's witness-generator `.wasm` and speaks
//! circom 2.x's host protocol (the code path `snarkjs.wtns.calculate` uses via `circom_runtime`'s
//! `witness_calculator.js`):
//!
//! * imports: `runtime.{exceptionHandler, printErrorMessage, writeBufferMessage, showSharedRWMemory}`
//! * `init(sanityCheck)`, then per input signal: `getInputSignalSize(hMSB, hLSB)`, and for every
//!   flattened element write the value (little-endian 32-bit limbs) with
//!   `writeSharedRWMemory(j, limb_j)` and call `setInputSignal(hMSB, hLSB, i)`;
//!   `(hMSB, hLSB)` is the 64-bit FNV-1a hash of the signal name.
//! * `getInputSize()` must equal the number of values set.
//! * for each `i < getWitnessSize()`: `getWitness(i)`, then read limbs via `readSharedRWMemory(j)`.
//!
//! The protocol is implemented once, against [`Instance`]. Two engines run the `.wasm`:
//!
//! * [`wasmi`] (a pure-Rust interpreter): native builds, and the fallback on wasm32.
//! * on `wasm32` only, the host's own `WebAssembly` engine (V8 in Chrome/Node), driven from Rust
//!   through `js_sys` (see `witness/js.rs`). It JIT-compiles the circuit, so it is ~30× faster
//!   than interpreting it inside wasm32. Selected by `cfg(target_arch = "wasm32")`, not by a
//!   feature: `wasm32-unknown-unknown` always has a JS host, and the result is identical.

use ark_bn254::Fr;
use ark_ff::{BigInteger256, PrimeField};
use wasmi::{Caller, Engine, Extern, Linker, Memory, Module, Store, TypedFunc};
use zeroize::{Zeroize, Zeroizing};

use crate::Error;

#[cfg(target_arch = "wasm32")]
mod js;

/// One fresh instance of a witness generator (fresh linear memory). Errors are messages;
/// the caller decides whether they are [`Error::Wasm`] or [`Error::Witness`].
trait Instance {
    fn init(&mut self, sanity_check: i32) -> Result<(), String>;
    fn get_input_signal_size(&mut self, msb: i32, lsb: i32) -> Result<i32, String>;
    fn set_input_signal(&mut self, msb: i32, lsb: i32, i: i32) -> Result<(), String>;
    fn write_shared(&mut self, j: i32, v: i32) -> Result<(), String>;
    fn read_shared(&mut self, j: i32) -> Result<i32, String>;
    fn get_witness(&mut self, i: i32) -> Result<(), String>;
    fn get_witness_size(&mut self) -> Result<i32, String>;
    fn get_input_size(&mut self) -> Result<i32, String>;
    fn get_n32(&mut self) -> Result<i32, String>;
    fn get_raw_prime(&mut self) -> Result<(), String>;
    /// Zero the instance's whole linear memory (it held secret inputs and intermediates).
    fn zeroize_memory(&mut self);
}

enum Backend {
    Wasmi(WasmiModule),
    #[cfg(target_arch = "wasm32")]
    Js(js::JsModule),
}

/// A compiled circom witness generator. Cheap to call repeatedly: each calculation
/// instantiates a fresh instance (fresh linear memory), which is zeroed before drop.
pub struct WitnessCalculator {
    backend: Backend,
    n32: usize,
    witness_size: usize,
    input_size: usize,
}

/// 64-bit FNV-1a, as `fnvHash` in witness_calculator.js (hashes `charCodeAt` per UTF-16 unit;
/// circom signal names are ASCII).
fn fnv1a64(s: &str) -> u64 {
    let mut h: u64 = 0xCBF2_9CE4_8422_2325;
    for c in s.encode_utf16() {
        h ^= u64::from(c);
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
    }
    h
}

/// `exceptionHandler` codes, as in witness_calculator.js.
fn exception_kind(code: i32) -> &'static str {
    match code {
        1 => "Signal not found",
        2 => "Too many signals set",
        3 => "Signal already set",
        4 => "Assert Failed",
        5 => "Not enough memory",
        6 => "Input signal array access exceeds the size",
        _ => "Unknown error",
    }
}

/// Appends a `printErrorMessage` line (circom assert / error text; only circuit-template
/// text, no signal values), capped.
fn push_err(err_str: &mut String, msg: &str) {
    if err_str.len() < 8192 {
        err_str.push_str(msg);
        err_str.push('\n');
    }
}

/// A trap message, with the accumulated `printErrorMessage` text unless already included.
fn trap_message(e: String, err_str: &str) -> String {
    if err_str.is_empty() || e.contains(err_str.trim_end()) {
        e
    } else {
        format!("{e}; {}", err_str.trim_end())
    }
}

impl WitnessCalculator {
    /// Compile a circom witness generator (`*.wasm` from the circuit artifacts).
    ///
    /// On wasm32 this compiles synchronously with the host's `WebAssembly` engine (allowed on
    /// workers, and on Chrome's main thread for modules this size; see the README). If the host
    /// refuses, it falls back to wasmi; [`WitnessCalculator::new_async`] avoids that.
    pub fn new(wasm: &[u8]) -> Result<Self, Error> {
        #[cfg(target_arch = "wasm32")]
        if let Ok(m) = js::JsModule::compile(wasm) {
            return Self::from_backend(Backend::Js(m));
        }
        Self::from_backend(Backend::Wasmi(WasmiModule::new(wasm)?))
    }

    /// wasm32 only: compile with `WebAssembly.compile` (async, off the calling thread, no size
    /// limit on any thread). Falls back to wasmi only if there is no usable `WebAssembly`.
    #[cfg(target_arch = "wasm32")]
    pub async fn new_async(wasm: &[u8]) -> Result<Self, Error> {
        match js::JsModule::compile_async(wasm).await {
            Ok(m) => Self::from_backend(Backend::Js(m)),
            Err(_) => Self::from_backend(Backend::Wasmi(WasmiModule::new(wasm)?)),
        }
    }

    /// Always the wasmi interpreter (differential tests on wasm32; same as `new` natively).
    #[doc(hidden)]
    pub fn new_wasmi(wasm: &[u8]) -> Result<Self, Error> {
        Self::from_backend(Backend::Wasmi(WasmiModule::new(wasm)?))
    }

    /// Which engine runs the witness generator: `"webassembly"` (the host's, wasm32) or `"wasmi"`.
    pub fn backend(&self) -> &'static str {
        match self.backend {
            Backend::Wasmi(_) => "wasmi",
            #[cfg(target_arch = "wasm32")]
            Backend::Js(_) => "webassembly",
        }
    }

    /// Reads the static circuit parameters once (and validates the prime).
    fn from_backend(backend: Backend) -> Result<Self, Error> {
        let mut calc = Self {
            backend,
            n32: 0,
            witness_size: 0,
            input_size: 0,
        };
        match &calc.backend {
            Backend::Wasmi(m) => calc.read_params(m.instantiate()?)?,
            #[cfg(target_arch = "wasm32")]
            Backend::Js(m) => calc.read_params(m.instantiate().map_err(Error::Wasm)?)?,
        }
        Ok(calc)
    }

    fn read_params(&mut self, mut inst: impl Instance) -> Result<(), Error> {
        let n32 = inst.get_n32().map_err(Error::Wasm)? as usize;
        if n32 != 8 {
            return Err(Error::Wasm(format!("unexpected field size n32={n32}, want 8 (bn128)")));
        }
        inst.get_raw_prime().map_err(Error::Wasm)?;
        let mut limbs = [0u64; 4];
        for j in 0..8 {
            let w = inst.read_shared(j as i32).map_err(Error::Wasm)? as u32;
            limbs[j / 2] |= u64::from(w) << (32 * (j % 2));
        }
        if BigInteger256::new(limbs) != Fr::MODULUS {
            return Err(Error::Wasm(
                "witness generator is not over the bn128 scalar field".into(),
            ));
        }
        self.n32 = n32;
        self.witness_size = inst.get_witness_size().map_err(Error::Wasm)? as usize;
        self.input_size = inst.get_input_size().map_err(Error::Wasm)? as usize;
        Ok(())
    }

    /// Number of signals in the full witness (including the leading constant `1`).
    pub fn witness_size(&self) -> usize {
        self.witness_size
    }

    /// Total number of input field elements the circuit expects.
    pub fn input_size(&self) -> usize {
        self.input_size
    }

    /// Compute the full witness for `inputs` (signal name -> flattened values).
    /// Equivalent to `snarkjs.wtns.calculate` + `wtns.exportJson`.
    pub fn calculate(&self, inputs: &[(&str, &[Fr])]) -> Result<Zeroizing<Vec<Fr>>, Error> {
        match &self.backend {
            Backend::Wasmi(m) => self.run(m.instantiate()?, inputs),
            #[cfg(target_arch = "wasm32")]
            Backend::Js(m) => self.run(m.instantiate().map_err(Error::Wasm)?, inputs),
        }
    }

    fn run(&self, mut inst: impl Instance, inputs: &[(&str, &[Fr])]) -> Result<Zeroizing<Vec<Fr>>, Error> {
        let res = self.calculate_inner(&mut inst, inputs);
        inst.zeroize_memory();
        res
    }

    fn calculate_inner(&self, inst: &mut impl Instance, inputs: &[(&str, &[Fr])]) -> Result<Zeroizing<Vec<Fr>>, Error> {
        inst.init(1).map_err(Error::Witness)?;

        let mut counter = 0usize;
        for (name, values) in inputs {
            let h = fnv1a64(name);
            let (msb, lsb) = ((h >> 32) as u32 as i32, h as u32 as i32);
            let size = inst.get_input_signal_size(msb, lsb).map_err(Error::Witness)?;
            if size < 0 {
                return Err(Error::Input(format!("signal {name} not found")));
            }
            if values.len() != size as usize {
                return Err(Error::Input(format!(
                    "signal {name}: expected {size} values, got {}",
                    values.len()
                )));
            }
            for (i, v) in values.iter().enumerate() {
                let mut limbs = Zeroizing::new(v.into_bigint().0);
                for j in 0..self.n32 {
                    let w = (limbs[j / 2] >> (32 * (j % 2))) as u32 as i32;
                    inst.write_shared(j as i32, w).map_err(Error::Witness)?;
                }
                limbs.zeroize();
                inst.set_input_signal(msb, lsb, i as i32).map_err(Error::Witness)?;
                counter += 1;
            }
        }
        if counter < self.input_size {
            return Err(Error::Input(format!(
                "not all inputs have been set: only {counter} out of {}",
                self.input_size
            )));
        }

        let mut w = Zeroizing::new(Vec::with_capacity(self.witness_size));
        for i in 0..self.witness_size {
            inst.get_witness(i as i32).map_err(Error::Witness)?;
            let mut limbs = [0u64; 4];
            for j in 0..self.n32 {
                let word = inst.read_shared(j as i32).map_err(Error::Witness)? as u32;
                limbs[j / 2] |= u64::from(word) << (32 * (j % 2));
            }
            let fe = Fr::from_bigint(BigInteger256::new(limbs))
                .ok_or_else(|| Error::Witness(format!("witness[{i}] not reduced")))?;
            limbs.zeroize();
            w.push(fe);
        }
        Ok(w)
    }
}

// ------------------------------------------------------------------------------------
// wasmi backend
// ------------------------------------------------------------------------------------

#[derive(Default)]
struct Host {
    /// Accumulated `printErrorMessage` output.
    err_str: String,
}

struct WasmiModule {
    engine: Engine,
    module: Module,
    linker: Linker<Host>,
}

fn read_message(caller: &mut Caller<'_, Host>) -> Result<String, wasmi::Error> {
    let f = caller
        .get_export("getMessageChar")
        .and_then(Extern::into_func)
        .ok_or_else(|| wasmi::Error::new("missing export getMessageChar"))?
        .typed::<(), i32>(&*caller)?;
    let mut s = String::new();
    loop {
        let c = f.call(&mut *caller, ())?;
        if c == 0 || s.len() > 4096 {
            break;
        }
        s.push(char::from(c as u8));
    }
    Ok(s)
}

struct Exports {
    memory: Memory,
    init: TypedFunc<i32, ()>,
    get_input_signal_size: TypedFunc<(i32, i32), i32>,
    set_input_signal: TypedFunc<(i32, i32, i32), ()>,
    write_shared: TypedFunc<(i32, i32), ()>,
    read_shared: TypedFunc<i32, i32>,
    get_witness: TypedFunc<i32, ()>,
    get_witness_size: TypedFunc<(), i32>,
    get_input_size: TypedFunc<(), i32>,
    get_n32: TypedFunc<(), i32>,
    get_raw_prime: TypedFunc<(), ()>,
}

impl WasmiModule {
    fn new(wasm: &[u8]) -> Result<Self, Error> {
        let engine = Engine::default();
        let module = Module::new(&engine, wasm).map_err(|e| Error::Wasm(e.to_string()))?;
        let mut linker = Linker::<Host>::new(&engine);

        linker
            .func_wrap(
                "runtime",
                "exceptionHandler",
                |caller: Caller<'_, Host>, code: i32| -> Result<(), wasmi::Error> {
                    Err(wasmi::Error::new(format!(
                        "circom runtime error {code}: {}. {}",
                        exception_kind(code),
                        caller.data().err_str.trim_end()
                    )))
                },
            )
            .map_err(|e| Error::Wasm(e.to_string()))?;
        linker
            .func_wrap(
                "runtime",
                "printErrorMessage",
                |mut caller: Caller<'_, Host>| -> Result<(), wasmi::Error> {
                    let msg = read_message(&mut caller)?;
                    push_err(&mut caller.data_mut().err_str, &msg);
                    Ok(())
                },
            )
            .map_err(|e| Error::Wasm(e.to_string()))?;
        // `log()` output from the circuit: drained but discarded (it could contain signal values).
        linker
            .func_wrap(
                "runtime",
                "writeBufferMessage",
                |mut caller: Caller<'_, Host>| -> Result<(), wasmi::Error> { read_message(&mut caller).map(|_| ()) },
            )
            .map_err(|e| Error::Wasm(e.to_string()))?;
        linker
            .func_wrap("runtime", "showSharedRWMemory", |_: Caller<'_, Host>| {})
            .map_err(|e| Error::Wasm(e.to_string()))?;

        Ok(Self { engine, module, linker })
    }

    fn instantiate(&self) -> Result<WasmiInstance, Error> {
        let wasm_err = |e: wasmi::Error| Error::Wasm(e.to_string());
        let mut store = Store::new(&self.engine, Host::default());
        let inst = self
            .linker
            .instantiate_and_start(&mut store, &self.module)
            .map_err(wasm_err)?;
        macro_rules! f {
            ($name:literal) => {
                inst.get_typed_func(&store, $name).map_err(wasm_err)?
            };
        }
        let memory = inst
            .get_memory(&store, "memory")
            .ok_or_else(|| Error::Wasm("missing exported memory".into()))?;
        let ex = Exports {
            memory,
            init: f!("init"),
            get_input_signal_size: f!("getInputSignalSize"),
            set_input_signal: f!("setInputSignal"),
            write_shared: f!("writeSharedRWMemory"),
            read_shared: f!("readSharedRWMemory"),
            get_witness: f!("getWitness"),
            get_witness_size: f!("getWitnessSize"),
            get_input_size: f!("getInputSize"),
            get_n32: f!("getFieldNumLen32"),
            get_raw_prime: f!("getRawPrime"),
        };
        Ok(WasmiInstance { store, ex })
    }
}

struct WasmiInstance {
    store: Store<Host>,
    ex: Exports,
}

impl WasmiInstance {
    fn trap(&self, e: wasmi::Error) -> String {
        trap_message(e.to_string(), &self.store.data().err_str)
    }
}

/// `self.ex.$f.call(&mut self.store, $args)` with the trap mapped to a message.
macro_rules! call {
    ($self:ident, $f:ident, $args:expr) => {{
        let r = $self.ex.$f.call(&mut $self.store, $args);
        r.map_err(|e| $self.trap(e))
    }};
}

impl Instance for WasmiInstance {
    fn init(&mut self, sanity_check: i32) -> Result<(), String> {
        call!(self, init, sanity_check)
    }
    fn get_input_signal_size(&mut self, msb: i32, lsb: i32) -> Result<i32, String> {
        call!(self, get_input_signal_size, (msb, lsb))
    }
    fn set_input_signal(&mut self, msb: i32, lsb: i32, i: i32) -> Result<(), String> {
        call!(self, set_input_signal, (msb, lsb, i))
    }
    fn write_shared(&mut self, j: i32, v: i32) -> Result<(), String> {
        call!(self, write_shared, (j, v))
    }
    fn read_shared(&mut self, j: i32) -> Result<i32, String> {
        call!(self, read_shared, j)
    }
    fn get_witness(&mut self, i: i32) -> Result<(), String> {
        call!(self, get_witness, i)
    }
    fn get_witness_size(&mut self) -> Result<i32, String> {
        call!(self, get_witness_size, ())
    }
    fn get_input_size(&mut self) -> Result<i32, String> {
        call!(self, get_input_size, ())
    }
    fn get_n32(&mut self) -> Result<i32, String> {
        call!(self, get_n32, ())
    }
    fn get_raw_prime(&mut self) -> Result<(), String> {
        call!(self, get_raw_prime, ())
    }
    fn zeroize_memory(&mut self) {
        self.ex.memory.data_mut(&mut self.store).zeroize();
    }
}

#[cfg(test)]
mod tests {
    use super::fnv1a64;

    #[test]
    fn fnv_matches_js() {
        // fnvHash("") = cbf29ce484222325; fnvHash("a") = af63dc4c8601ec8c (FNV-1a 64 reference).
        assert_eq!(fnv1a64(""), 0xcbf29ce484222325);
        assert_eq!(fnv1a64("a"), 0xaf63dc4c8601ec8c);
    }
}
