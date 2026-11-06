//! Private balance, Move in/out and Send privately (Privacy Pools v1) for the real backend.
//!
//! The flows live in `packages/lowki-kohaku-host/src/private-wallet.ts` (Kohaku's
//! `PrivacyPoolsV1Protocol` on the Lowki Host, the worker prover adapter, Kohaku's relayer
//! client); the glue's `openPrivate` exposes them. This file only:
//! - hands the glue a `request(req) -> Promise<resp>` bridge to this backend's key worker
//!   (envelope signatures, `ppShield`, `ppNote`, `ppLoadCircuits`, `ppProveWithdraw` all run in
//!   the same worker as signing; the proof runs on WebGPU when the browser has it);
//! - maps the glue's stage callbacks onto [`ProofProgress`];
//! - parses the JSON results.
//!
//! Signing and broadcasting the deposit stays in Rust (`RealBackend::transact`), over the same
//! routed chain provider as every other read.

use std::cell::RefCell;
use std::rc::Rc;

use serde::Deserialize;
use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::*;

use super::chain::{call_method, glue};
use super::js::obj;
use super::worker::WorkerClient;
use crate::backend::{ProgressFn, ProofProgress, ProofStage};

#[derive(Debug, Deserialize)]
pub struct PoolBalance {
    pub approved: String,
    pub pending: String,
}

#[derive(Debug, Deserialize)]
pub struct ShieldTx {
    pub to: String,
    pub data: String,
    pub value: String,
    pub precommitment: String,
    #[serde(rename = "depositIndex")]
    pub deposit_index: u32,
}

#[derive(Debug, Deserialize)]
pub struct Approved {
    pub label: String,
    pub balance: String,
    pub ms: f64,
}

#[derive(Debug, Deserialize)]
pub struct ProofInfo {
    pub backend: String,
    pub ms: f64,
}

#[derive(Debug, Deserialize)]
pub struct Timings {
    #[serde(rename = "syncMs")]
    pub sync_ms: f64,
    #[serde(rename = "proveMs")]
    pub prove_ms: f64,
    #[serde(rename = "submitMs")]
    pub submit_ms: f64,
}

#[derive(Debug, Deserialize)]
pub struct Unshield {
    #[serde(rename = "txHash")]
    pub tx_hash: String,
    pub net: String,
    pub fee: String,
    #[serde(rename = "relayFeeBps")]
    pub relay_fee_bps: String,
    #[serde(rename = "nullifierHash")]
    pub nullifier_hash: String,
    pub proof: Option<ProofInfo>,
    pub timings: Timings,
}

/// One Privacy Pools session per unlocked wallet (account index 0 of the seed: the private
/// balance is the user's, not any one app's).
#[derive(Default)]
pub struct PrivatePool {
    session: RefCell<Option<JsValue>>,
}

fn parse<T: for<'de> Deserialize<'de>>(v: JsValue) -> Result<T, String> {
    let s = v.as_string().ok_or("glue returned no JSON")?;
    serde_json::from_str(&s).map_err(|e| format!("glue JSON: {e}"))
}

impl PrivatePool {
    /// Forgets the session (sign-out, or a different seed).
    pub fn reset(&self) {
        *self.session.borrow_mut() = None;
    }

    async fn session(&self, worker: &Rc<WorkerClient>) -> Result<JsValue, String> {
        if let Some(s) = self.session.borrow().clone() {
            return Ok(s);
        }
        let g = glue().ok_or("the Anon-RPC/private module (lowki-glue) didn't load")?;
        let w = worker.clone();
        // The worker bridge: the glue's WorkerTransport.request. Error responses are returned
        // as values (the TS side raises them), never thrown.
        let request = Closure::<dyn Fn(JsValue) -> js_sys::Promise>::new(move |req: JsValue| {
            let w = w.clone();
            wasm_bindgen_futures::future_to_promise(async move {
                Ok(match w.call(req).await {
                    Ok(r) => r,
                    Err(e) => obj(&[
                        ("type", "error".into()),
                        ("code", JsValue::from_str(&e.code)),
                        ("message", JsValue::from_str(&e.message)),
                    ]),
                })
            })
        })
        .into_js_value();
        let s = call_method(&g, "openPrivate", &[request]).await?;
        *self.session.borrow_mut() = Some(s.clone());
        Ok(s)
    }

    pub async fn balance(&self, worker: &Rc<WorkerClient>) -> Result<PoolBalance, String> {
        let s = self.session(worker).await?;
        parse(call_method(&s, "balance", &[]).await?)
    }

    pub async fn prepare_shield(&self, worker: &Rc<WorkerClient>, wei: u128) -> Result<ShieldTx, String> {
        let s = self.session(worker).await?;
        parse(call_method(&s, "prepareShield", &[JsValue::from_str(&wei.to_string())]).await?)
    }

    /// Syncs until the ASP approves the deposit. `on_poll(fraction)` while waiting.
    pub async fn wait_approved(&self, worker: &Rc<WorkerClient>, precommitment: &str, on_poll: Rc<dyn Fn(f32)>) -> Result<Approved, String> {
        let s = self.session(worker).await?;
        let cb = Closure::<dyn Fn(String, f64)>::new(move |state: String, ms: f64| {
            // No ETA from an ASP: ease towards 90 % over ~30 s, then wait.
            let f = if state == "approved" { 1.0 } else { (0.9 * (1.0 - (-ms / 12_000.0).exp())) as f32 };
            on_poll(f);
        });
        let r = call_method(&s, "waitApproved", &[JsValue::from_str(precommitment), cb.as_ref().unchecked_ref::<JsValue>().clone()]).await;
        drop(cb);
        parse(r?)
    }

    /// Kohaku sync -> worker proof -> relayer. Reports the three stages.
    pub async fn unshield(&self, worker: &Rc<WorkerClient>, wei: u128, recipient: &str, progress: ProgressFn) -> Result<Unshield, String> {
        let s = self.session(worker).await?;
        let cb = Closure::<dyn Fn(String, f64, String)>::new(move |stage: String, fraction: f64, _detail: String| {
            let stage = match stage.as_str() {
                "sync" => ProofStage::SyncingPool,
                "prove" => ProofStage::BuildingProof,
                _ => ProofStage::Submitting,
            };
            progress(ProofProgress { stage, fraction: fraction as f32 });
        });
        let r = call_method(
            &s,
            "unshield",
            &[JsValue::from_str(&wei.to_string()), JsValue::from_str(recipient), cb.as_ref().unchecked_ref::<JsValue>().clone()],
        )
        .await;
        drop(cb);
        parse(r?)
    }
}
