//! wasm-bindgen surface for the browser benchmarks. Timing is done in JS (`js/harness.ts`)
//! around these calls, exactly as for the TS side.
use crate::cases::{Case, cases};
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
pub struct MicroBench {
    cases: Vec<Case>,
}

#[wasm_bindgen]
impl MicroBench {
    #[wasm_bindgen(constructor)]
    pub fn new(fixtures_json: &str) -> MicroBench {
        let fx: serde_json::Value = serde_json::from_str(fixtures_json).unwrap();
        MicroBench { cases: cases(&fx) }
    }
    pub fn len(&self) -> usize {
        self.cases.len()
    }
    pub fn id(&self, i: usize) -> String {
        self.cases[i].id.to_string()
    }
    pub fn batch(&self, i: usize) -> usize {
        self.cases[i].batch
    }
    /// 0 = adaptive.
    pub fn iters(&self, i: usize) -> usize {
        self.cases[i].iters.unwrap_or(0)
    }
    pub fn run(&mut self, i: usize) {
        (self.cases[i].run)()
    }
    pub fn check(&mut self, i: usize) -> String {
        (self.cases[i].check)()
    }
}

#[cfg(feature = "g16")]
mod g16 {
    use crate::g16::*;
    use lowki_groth16::CircuitProver;
    use wasm_bindgen::prelude::*;

    #[wasm_bindgen]
    pub struct G16 {
        p: CircuitProver,
        input: String,
    }

    #[wasm_bindgen]
    impl G16 {
        /// Parses the zkey and validates the circuit wasm (timed by the caller as "load").
        #[wasm_bindgen(constructor)]
        pub fn new(circuit_name: &str, wasm: &[u8], zkey: &[u8], input_json: String) -> G16 {
            G16 { p: CircuitProver::load(circuit(circuit_name), wasm, zkey).unwrap(), input: input_json }
        }
        /// "webassembly" (host engine) or "wasmi".
        pub fn backend(&self) -> String {
            self.p.witness_backend().to_string()
        }
        pub fn witness(&self) -> usize {
            witness(&self.p, &self.input)
        }
        /// Returns `{proof, publicSignals}` JSON.
        pub fn prove(&self) -> String {
            to_json(&prove(&self.p, &self.input))
        }
    }

    #[cfg(feature = "webgpu")]
    mod gpu {
        use super::*;
        use lowki_groth16::{GpuProver, open_gpu};
        use std::rc::Rc;

        #[wasm_bindgen]
        pub struct G16Gpu {
            p: Rc<GpuProver>,
            input: Rc<String>,
        }

        #[wasm_bindgen]
        impl G16Gpu {
            pub fn prove(&self) -> js_sys::Promise {
                let (p, input) = (self.p.clone(), self.input.clone());
                wasm_bindgen_futures::future_to_promise(async move {
                    Ok(JsValue::from_str(&to_json(&prove_gpu(&p, &input).await)))
                })
            }
        }

        /// Opens WebGPU, loads the circuit and uploads its proving-key bases. Returns
        /// `[G16Gpu, open_ms, load_ms, upload_ms]`.
        #[wasm_bindgen]
        pub async fn g16_gpu(circuit_name: String, wasm: Vec<u8>, zkey: Vec<u8>, input_json: String, window_bits: u32) -> Result<js_sys::Array, JsError> {
            let now = || js_sys::Date::now();
            let t = now();
            let gpu = open_gpu().await.map_err(|e| JsError::new(&e.to_string()))?;
            let t_open = now() - t;
            let t = now();
            let cpu = CircuitProver::load(circuit(&circuit_name), &wasm, &zkey).unwrap();
            let t_load = now() - t;
            let t = now();
            let mut gp = GpuProver::new(gpu, cpu).map_err(|e| JsError::new(&e.1.to_string()))?;
            if window_bits != 0 {
                gp.set_window_bits(window_bits); // tuning experiments only
            }
            let t_up = now() - t;
            let out = js_sys::Array::new();
            out.push(&G16Gpu { p: Rc::new(gp), input: Rc::new(input_json) }.into());
            out.push(&t_open.into());
            out.push(&t_load.into());
            out.push(&t_up.into());
            Ok(out)
        }
    }
}

#[wasm_bindgen]
pub fn fb_check() -> bool {
    crate::fieldbench::check_mont32()
}
#[wasm_bindgen]
pub fn fb_mul_ark(n: u32) -> String {
    crate::fieldbench::mul_ark(n).to_string()
}
#[wasm_bindgen]
pub fn fb_mul_32(n: u32) -> String {
    crate::fieldbench::mul_32(n).to_string()
}
#[wasm_bindgen]
pub fn fb_fft_ark(log_n: u32) -> String {
    crate::fieldbench::fft_ark(log_n).to_string()
}
