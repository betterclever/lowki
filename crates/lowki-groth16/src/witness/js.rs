//! wasm32 backend: the circom witness generator runs on the host's own `WebAssembly` engine
//! (V8 in Chrome and Node), JIT-compiled, instead of being interpreted by wasmi inside our
//! module. Only bindings live here; the protocol is `super::WitnessCalculator::calculate_inner`.
//!
//! * The module is compiled once per [`super::WitnessCalculator`] (`new WebAssembly.Module`, or
//!   `WebAssembly.compile` from `new_async`). Each calculation gets a fresh `WebAssembly.Instance`
//!   with its own imports (Rust closures) and its own linear memory.
//! * Secrets: limbs cross into Rust one `i32` at a time (as with wasmi) and land in the caller's
//!   `Zeroizing` buffers; the instance's whole memory is zeroed before it is dropped. JS engine
//!   temporaries (numbers on the JS stack) cannot be wiped, as for any JS caller.

use std::cell::RefCell;
use std::rc::Rc;

use js_sys::{Object, Reflect, Uint8Array, WebAssembly};
use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::*;

use super::{Instance, exception_kind, push_err, trap_message};

#[wasm_bindgen]
extern "C" {
    /// `instance.exports` of a circom 2.x witness generator.
    #[derive(Clone)]
    type CircomExports;
    #[wasm_bindgen(method, catch)]
    fn init(this: &CircomExports, sanity_check: i32) -> Result<(), JsValue>;
    #[wasm_bindgen(method, catch, js_name = getInputSignalSize)]
    fn get_input_signal_size(this: &CircomExports, msb: i32, lsb: i32) -> Result<i32, JsValue>;
    #[wasm_bindgen(method, catch, js_name = setInputSignal)]
    fn set_input_signal(this: &CircomExports, msb: i32, lsb: i32, i: i32) -> Result<(), JsValue>;
    #[wasm_bindgen(method, catch, js_name = writeSharedRWMemory)]
    fn write_shared(this: &CircomExports, j: i32, v: i32) -> Result<(), JsValue>;
    #[wasm_bindgen(method, catch, js_name = readSharedRWMemory)]
    fn read_shared(this: &CircomExports, j: i32) -> Result<i32, JsValue>;
    #[wasm_bindgen(method, catch, js_name = getWitness)]
    fn get_witness(this: &CircomExports, i: i32) -> Result<(), JsValue>;
    #[wasm_bindgen(method, catch, js_name = getWitnessSize)]
    fn get_witness_size(this: &CircomExports) -> Result<i32, JsValue>;
    #[wasm_bindgen(method, catch, js_name = getInputSize)]
    fn get_input_size(this: &CircomExports) -> Result<i32, JsValue>;
    #[wasm_bindgen(method, catch, js_name = getFieldNumLen32)]
    fn get_n32(this: &CircomExports) -> Result<i32, JsValue>;
    #[wasm_bindgen(method, catch, js_name = getRawPrime)]
    fn get_raw_prime(this: &CircomExports) -> Result<(), JsValue>;
    #[wasm_bindgen(method, catch, js_name = getMessageChar)]
    fn get_message_char(this: &CircomExports) -> Result<i32, JsValue>;
    #[wasm_bindgen(method, getter)]
    fn memory(this: &CircomExports) -> WebAssembly::Memory;
}

fn js_message(v: &JsValue) -> String {
    match v.dyn_ref::<js_sys::Error>() {
        Some(e) => String::from(e.message()),
        None => format!("{v:?}"),
    }
}

pub(super) struct JsModule {
    module: WebAssembly::Module,
}

impl JsModule {
    /// `new WebAssembly.Module(bytes)`: synchronous. Workers accept any size; browsers may cap
    /// the size on the main thread (then this errors and the caller falls back to wasmi).
    pub(super) fn compile(wasm: &[u8]) -> Result<Self, String> {
        let module = WebAssembly::Module::new(&Uint8Array::from(wasm)).map_err(|e| js_message(&e))?;
        Ok(Self { module })
    }

    /// `await WebAssembly.compile(bytes)`: no size cap on any thread.
    pub(super) async fn compile_async(wasm: &[u8]) -> Result<Self, String> {
        let p = WebAssembly::compile(&Uint8Array::from(wasm));
        let m = wasm_bindgen_futures::JsFuture::from(p)
            .await
            .map_err(|e| js_message(&e))?;
        Ok(Self {
            module: m.unchecked_into(),
        })
    }

    pub(super) fn instantiate(&self) -> Result<JsInstance, String> {
        let host = Rc::new(RefCell::new(Host::default()));

        let h = host.clone();
        let exception = Closure::<dyn FnMut(i32) -> Result<(), JsValue>>::new(move |code: i32| {
            let msg = format!(
                "circom runtime error {code}: {}. {}",
                exception_kind(code),
                h.borrow().err_str.trim_end()
            );
            Err(js_sys::Error::new(&msg).into())
        });
        let h = host.clone();
        let print_error = Closure::<dyn FnMut() -> Result<(), JsValue>>::new(move || {
            let msg = h.borrow().read_message()?;
            push_err(&mut h.borrow_mut().err_str, &msg);
            Ok(())
        });
        // `log()` output from the circuit: drained but discarded (it could contain signal values).
        let h = host.clone();
        let write_buffer =
            Closure::<dyn FnMut() -> Result<(), JsValue>>::new(move || h.borrow().read_message().map(|_| ()));
        let show_shared = Closure::<dyn FnMut()>::new(|| {});

        let runtime = Object::new();
        let set = |k: &str, f: &JsValue| Reflect::set(&runtime, &k.into(), f).map(|_| ());
        set("exceptionHandler", exception.as_ref())
            .and_then(|_| set("printErrorMessage", print_error.as_ref()))
            .and_then(|_| set("writeBufferMessage", write_buffer.as_ref()))
            .and_then(|_| set("showSharedRWMemory", show_shared.as_ref()))
            .map_err(|e| js_message(&e))?;
        let imports = Object::new();
        Reflect::set(&imports, &"runtime".into(), &runtime).map_err(|e| js_message(&e))?;

        let instance = WebAssembly::Instance::new(&self.module, &imports).map_err(|e| js_message(&e))?;
        let ex: CircomExports = instance.exports().unchecked_into();
        host.borrow_mut().exports = Some(ex.clone());
        Ok(JsInstance {
            host,
            ex,
            _closures: (exception, print_error, write_buffer, show_shared),
        })
    }
}

#[derive(Default)]
struct Host {
    /// Accumulated `printErrorMessage` output.
    err_str: String,
    /// Set right after instantiation (imports need the instance's `getMessageChar`).
    exports: Option<CircomExports>,
}

impl Host {
    fn read_message(&self) -> Result<String, JsValue> {
        let ex = self
            .exports
            .as_ref()
            .ok_or_else(|| JsValue::from_str("instance not ready"))?;
        let mut s = String::new();
        loop {
            let c = ex.get_message_char()?;
            if c == 0 || s.len() > 4096 {
                break;
            }
            s.push(char::from(c as u8));
        }
        Ok(s)
    }
}

type Closures = (
    Closure<dyn FnMut(i32) -> Result<(), JsValue>>,
    Closure<dyn FnMut() -> Result<(), JsValue>>,
    Closure<dyn FnMut() -> Result<(), JsValue>>,
    Closure<dyn FnMut()>,
);

pub(super) struct JsInstance {
    host: Rc<RefCell<Host>>,
    ex: CircomExports,
    /// The imports must outlive every call into the instance.
    _closures: Closures,
}

impl JsInstance {
    fn trap(&self, e: JsValue) -> String {
        trap_message(js_message(&e), &self.host.borrow().err_str)
    }
}

impl Instance for JsInstance {
    fn init(&mut self, sanity_check: i32) -> Result<(), String> {
        self.ex.init(sanity_check).map_err(|e| self.trap(e))
    }
    fn get_input_signal_size(&mut self, msb: i32, lsb: i32) -> Result<i32, String> {
        self.ex.get_input_signal_size(msb, lsb).map_err(|e| self.trap(e))
    }
    fn set_input_signal(&mut self, msb: i32, lsb: i32, i: i32) -> Result<(), String> {
        self.ex.set_input_signal(msb, lsb, i).map_err(|e| self.trap(e))
    }
    fn write_shared(&mut self, j: i32, v: i32) -> Result<(), String> {
        self.ex.write_shared(j, v).map_err(|e| self.trap(e))
    }
    fn read_shared(&mut self, j: i32) -> Result<i32, String> {
        self.ex.read_shared(j).map_err(|e| self.trap(e))
    }
    fn get_witness(&mut self, i: i32) -> Result<(), String> {
        self.ex.get_witness(i).map_err(|e| self.trap(e))
    }
    fn get_witness_size(&mut self) -> Result<i32, String> {
        self.ex.get_witness_size().map_err(|e| self.trap(e))
    }
    fn get_input_size(&mut self) -> Result<i32, String> {
        self.ex.get_input_size().map_err(|e| self.trap(e))
    }
    fn get_n32(&mut self) -> Result<i32, String> {
        self.ex.get_n32().map_err(|e| self.trap(e))
    }
    fn get_raw_prime(&mut self) -> Result<(), String> {
        self.ex.get_raw_prime().map_err(|e| self.trap(e))
    }
    fn zeroize_memory(&mut self) {
        let mem = Uint8Array::new(&self.ex.memory().buffer());
        mem.fill(0, 0, mem.length());
    }
}
