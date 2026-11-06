//! wasm-bindgen front: `new LowkiWorker(autoLockMinutes?)`, `await worker.handle(request)`.
//! Requests/responses cross as plain JS objects (serde-wasm-bindgen; byte fields as
//! `Uint8Array`). A `setTimeout` timer locks (zeroizes) the session at expiry even when
//! no further message arrives; every request also checks expiry first.

use std::cell::RefCell;
use std::rc::{Rc, Weak};

use wasm_bindgen::prelude::*;

use crate::error::WorkerError;
use crate::protocol::{Request, Response};
use crate::{Action, Core, DEFAULT_AUTO_LOCK_MS};

fn now_ms() -> f64 {
    js_sys::Date::now()
}

fn to_js(r: &Response) -> JsValue {
    let ser = serde_wasm_bindgen::Serializer::new().serialize_maps_as_objects(true).serialize_large_number_types_as_bigints(false);
    serde::Serialize::serialize(r, &ser).unwrap_or_else(|_| JsValue::from_str("serialization failed"))
}

/// serde's messages can quote the offending input (e.g. a PRF output passed as a string);
/// only structural messages, which name fields/variants but no values, are passed through.
pub(crate) fn parse_error_message(e: &str) -> String {
    let e = e.trim_start_matches("Error: ");
    const SAFE: [&str; 4] = ["missing field", "unknown variant", "unknown field", "duplicate field"];
    if SAFE.iter().any(|p| e.starts_with(p)) {
        format!("bad request: {e}")
    } else {
        "bad request: a field has the wrong type or shape".to_string()
    }
}

const ARTIFACT_FIELDS: [&str; 4] = ["withdrawWasm", "withdrawZkey", "commitmentWasm", "commitmentZkey"];

/// serde-wasm-bindgen copies a `Uint8Array` into Rust one element at a time, which took
/// ~10 s for the 21 MB of circuit artifacts. For `ppLoadCircuits`, copy them in bulk
/// (`Uint8Array.copy_to`) and deserialize the rest of the (shallow-copied) request normally.
type Artifacts = Option<[Option<serde_bytes::ByteBuf>; 4]>;
fn take_artifacts(request: JsValue) -> (JsValue, Artifacts) {
    let is_load = js_sys::Reflect::get(&request, &JsValue::from_str("type"))
        .ok()
        .and_then(|t| t.as_string())
        .is_some_and(|t| t == "ppLoadCircuits");
    if !is_load || !request.is_object() {
        return (request, None);
    }
    let copy = js_sys::Object::assign(&js_sys::Object::new(), request.unchecked_ref());
    let mut out: [Option<serde_bytes::ByteBuf>; 4] = Default::default();
    for (i, f) in ARTIFACT_FIELDS.iter().enumerate() {
        let key = JsValue::from_str(f);
        if let Ok(v) = js_sys::Reflect::get(&copy, &key)
            && let Some(arr) = v.dyn_ref::<js_sys::Uint8Array>()
        {
            out[i] = Some(serde_bytes::ByteBuf::from(arr.to_vec()));
            let _ = js_sys::Reflect::delete_property(&copy, &key);
        }
    }
    (copy.into(), Some(out))
}

#[wasm_bindgen]
pub struct LowkiWorker {
    core: Rc<RefCell<Core>>,
}

fn schedule_lock(core: &Rc<RefCell<Core>>, at_ms: f64) {
    let weak: Weak<RefCell<Core>> = Rc::downgrade(core);
    let cb = Closure::once_into_js(move || {
        if let Some(c) = weak.upgrade()
            && let Ok(mut c) = c.try_borrow_mut()
        {
            c.lock_if_expired(now_ms());
        }
    });
    let global = js_sys::global();
    if let Ok(set_timeout) = js_sys::Reflect::get(&global, &JsValue::from_str("setTimeout"))
        && let Ok(f) = set_timeout.dyn_into::<js_sys::Function>()
    {
        // +50 ms so the timer fires after the deadline, not just before it.
        let delay = (at_ms - now_ms()).max(0.0) + 50.0;
        if let Ok(handle) = f.call2(&global, &cb, &JsValue::from_f64(delay)) {
            // Node: don't keep the process alive just for the lock timer.
            if let Ok(unref) = js_sys::Reflect::get(&handle, &JsValue::from_str("unref"))
                && let Ok(unref) = unref.dyn_into::<js_sys::Function>()
            {
                let _ = unref.call0(&handle);
            }
        }
    }
}

#[wasm_bindgen]
impl LowkiWorker {
    /// `autoLockMinutes` defaults to 15.
    #[wasm_bindgen(constructor)]
    pub fn new(auto_lock_minutes: Option<f64>) -> LowkiWorker {
        let ms = auto_lock_minutes.map(|m| m * 60_000.0).unwrap_or(DEFAULT_AUTO_LOCK_MS);
        LowkiWorker { core: Rc::new(RefCell::new(Core::new(ms, Box::new(rand_core::OsRng)))) }
    }

    /// Handles one request object; always resolves to a response object (errors are
    /// `{type: "error", code, message}`).
    pub async fn handle(&self, request: JsValue) -> JsValue {
        let (request, artifacts) = take_artifacts(request);
        let req: Request = match serde_wasm_bindgen::from_value(request) {
            Ok(r) => r,
            Err(e) => return to_js(&WorkerError::invalid(parse_error_message(&e.to_string())).into()),
        };
        let req = match (req, artifacts) {
            (Request::PpLoadCircuits { use_gpu, .. }, Some([ww, wz, cw, cz])) => Request::PpLoadCircuits {
                withdraw_wasm: ww,
                withdraw_zkey: wz,
                commitment_wasm: cw,
                commitment_zkey: cz,
                use_gpu,
            },
            (r, _) => r,
        };

        let action = {
            let Ok(mut core) = self.core.try_borrow_mut() else {
                return to_js(&WorkerError::new(crate::error::ErrorCode::Internal, "worker busy").into());
            };
            let was = core.expires_at_ms();
            let a = core.prepare(req, now_ms());
            if let Some(exp) = core.expires_at_ms()
                && was != Some(exp)
            {
                schedule_lock(&self.core, exp);
            }
            a
        };
        let resp = match action {
            Action::LoadCircuits(l) => match crate::load_circuits(*l).await {
                Ok(p) => self.core.borrow_mut().install_provers(p),
                Err(e) => e.into(),
            },
            a => crate::run_proof(a, &now_ms).await,
        };
        to_js(&resp)
    }

    /// Locks immediately (zeroizes the seed).
    pub fn lock(&self) {
        if let Ok(mut c) = self.core.try_borrow_mut() {
            c.lock();
        }
    }

    #[wasm_bindgen(js_name = isUnlocked)]
    pub fn is_unlocked(&self) -> bool {
        self.core.try_borrow().map(|c| c.is_unlocked()).unwrap_or(true)
    }
}
