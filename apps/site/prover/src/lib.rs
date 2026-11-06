//! The showcase site's prover: a thin wasm-bindgen layer over `lowki-groth16` with the
//! `webgpu` feature. No proving logic lives here; it only moves bytes and JSON across.
//!
//! JS flow: `load(wasm, zkey)` -> `Prover`; `open_webgpu()` -> `Gpu`; `prover.use_gpu(gpu)`
//! (falls back to the CPU on any error); `prover.prove(input_json)` -> JSON
//! `{proof, publicSignals, backend, stages: [[name, ms], ...]}`; `verify(vkey, proof_json)`.
use std::cell::RefCell;
use std::rc::Rc;

use lowki_groth16::{
    Circuit, CircuitProver, Error, GpuProver, SnarkjsProof, WithdrawInputs, open_gpu, parse_vkey_json, prove_withdraw,
    verify as g16_verify,
};
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_namespace = performance)]
    fn now() -> f64;
}

fn js_err(e: impl std::fmt::Display) -> JsError {
    JsError::new(&e.to_string())
}

enum Inner {
    Cpu(CircuitProver),
    Gpu(GpuProver),
    Empty,
}

/// An opened WebGPU device.
#[wasm_bindgen]
pub struct GpuDevice {
    gpu: Rc<lowki_gpu::Gpu>,
}

#[wasm_bindgen]
impl GpuDevice {
    /// "name · backend" of the adapter, for the page.
    pub fn adapter(&self) -> String {
        let i = self.gpu.adapter_info();
        let name = if i.name.is_empty() { i.vendor.to_string() } else { i.name.clone() };
        format!("{name} · {:?}", i.backend)
    }
}

/// Opens WebGPU (rejects when there is no usable adapter).
#[wasm_bindgen]
pub async fn open_webgpu() -> Result<GpuDevice, JsError> {
    open_gpu().await.map(|gpu| GpuDevice { gpu }).map_err(js_err)
}

#[wasm_bindgen]
pub struct Prover {
    inner: Rc<RefCell<Inner>>,
}

/// Loads the pinned Privacy Pools 1.0.5 withdraw circuit (sha256-checked, witness `.wasm`
/// compiled asynchronously on the browser's WebAssembly engine).
#[wasm_bindgen]
pub async fn load_withdraw(wasm: Vec<u8>, zkey: Vec<u8>) -> Result<Prover, JsError> {
    let p = CircuitProver::load_async(Circuit::Withdraw, &wasm, &zkey).await.map_err(js_err)?;
    Ok(Prover { inner: Rc::new(RefCell::new(Inner::Cpu(p))) })
}

#[wasm_bindgen]
impl Prover {
    /// Uploads the proving-key bases to the GPU. On failure the prover stays on the CPU and
    /// the error is returned.
    pub fn use_gpu(&self, gpu: &GpuDevice) -> Result<(), JsError> {
        let mut inner = self.inner.borrow_mut();
        match std::mem::replace(&mut *inner, Inner::Empty) {
            Inner::Cpu(cpu) => match GpuProver::new(gpu.gpu.clone(), cpu) {
                Ok(g) => {
                    *inner = Inner::Gpu(g);
                    Ok(())
                }
                Err((cpu, e)) => {
                    *inner = Inner::Cpu(cpu);
                    Err(js_err(e))
                }
            },
            other => {
                *inner = other;
                Ok(())
            }
        }
    }

    /// Drop the GPU and prove on one CPU thread from now on.
    pub fn use_cpu(&self) {
        let mut inner = self.inner.borrow_mut();
        if let Inner::Gpu(g) = std::mem::replace(&mut *inner, Inner::Empty) {
            *inner = Inner::Cpu(g.into_cpu());
        }
    }

    pub fn backend(&self) -> String {
        match &*self.inner.borrow() {
            Inner::Gpu(_) => "webgpu".into(),
            Inner::Cpu(_) => "cpu".into(),
            Inner::Empty => "none".into(),
        }
    }

    pub fn witness_backend(&self) -> String {
        match &*self.inner.borrow() {
            Inner::Gpu(g) => g.cpu().witness_backend().into(),
            Inner::Cpu(c) => c.witness_backend().into(),
            Inner::Empty => "none".into(),
        }
    }

    /// Proves a withdraw. Resolves to `{proof, publicSignals, backend, stages}`. A GPU error
    /// falls back to the CPU for this proof (`backend: "cpu-fallback"`).
    pub fn prove(&self, input_json: String) -> js_sys::Promise {
        let inner = self.inner.clone();
        wasm_bindgen_futures::future_to_promise(async move {
            let inputs = WithdrawInputs::from_json(&input_json).map_err(|e| JsValue::from(js_err(e)))?;
            let t0 = now();
            let inner = inner.borrow();
            let (out, stages, backend) = match &*inner {
                Inner::Gpu(g) => {
                    match g.profile_with_rng(&inputs.as_signals(), &mut rand_core::OsRng, &now).await {
                        Ok((out, st)) => (out, st, "webgpu"),
                        Err(Error::Gpu(_)) => {
                            let out = prove_withdraw(g.cpu(), &inputs).map_err(|e| JsValue::from(js_err(e)))?;
                            (out, vec![("cpu prove", now() - t0)], "cpu-fallback")
                        }
                        Err(e) => return Err(js_err(e).into()),
                    }
                }
                Inner::Cpu(c) => {
                    let out = prove_withdraw(c, &inputs).map_err(|e| JsValue::from(js_err(e)))?;
                    (out, vec![("cpu prove", now() - t0)], "cpu")
                }
                Inner::Empty => return Err(js_err("prover not loaded").into()),
            };
            let total = now() - t0;
            let stages: Vec<_> = stages.iter().map(|(n, ms)| serde_json::json!([n, ms])).collect();
            let json = serde_json::json!({
                "proof": out.proof,
                "publicSignals": out.public_signals,
                "backend": backend,
                "total_ms": total,
                "stages": stages,
            });
            Ok(JsValue::from_str(&json.to_string()))
        })
    }
}

/// Groth16 verification with lowki-groth16's verifier: `proof_json` is `{proof, publicSignals}`
/// as snarkjs prints it.
#[wasm_bindgen]
pub fn verify(vkey_json: &str, proof_json: &str) -> Result<bool, JsError> {
    let vk = parse_vkey_json(vkey_json).map_err(js_err)?;
    let v: serde_json::Value = serde_json::from_str(proof_json).map_err(js_err)?;
    let proof: SnarkjsProof = serde_json::from_value(v["proof"].clone()).map_err(js_err)?;
    let signals: Vec<String> = serde_json::from_value(v["publicSignals"].clone()).map_err(js_err)?;
    g16_verify(&vk, &proof, &signals).map_err(js_err)
}
