//! Client for lowki-worker running in a dedicated module Web Worker.
//!
//! The worker (`crates/lowki-worker`, `js/worker.js` + `pkg/web`) is the only place keys
//! exist. This side only moves request objects in and response objects out:
//! `postMessage({id, request})` -> `{id, response}`.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use js_sys::{Function, Promise};
use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::JsFuture;

use super::js::{get, get_str};

/// Where trunk copies the worker (see `index.html`).
pub const WORKER_URL: &str = "lowki-worker/js/worker.js";

/// A worker failure: `{type: "error", code, message}`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorkerFail {
    pub code: String,
    pub message: String,
}

type Pending = Rc<RefCell<HashMap<u32, Function>>>;

pub struct WorkerClient {
    worker: web_sys::Worker,
    pending: Pending,
    next: Cell<u32>,
    _on_message: Closure<dyn Fn(web_sys::MessageEvent)>,
    _on_error: Closure<dyn Fn(web_sys::Event)>,
}

impl WorkerClient {
    pub fn spawn() -> Result<Self, String> {
        let opts = web_sys::WorkerOptions::new();
        opts.set_type(web_sys::WorkerType::Module);
        opts.set_name("lowki-keys");
        let worker = web_sys::Worker::new_with_options(WORKER_URL, &opts).map_err(|e| format!("{e:?}"))?;
        let pending: Pending = Rc::new(RefCell::new(HashMap::new()));
        let p = pending.clone();
        let on_message = Closure::<dyn Fn(web_sys::MessageEvent)>::new(move |ev: web_sys::MessageEvent| {
            let data = ev.data();
            let Some(id) = get(&data, "id").as_f64() else { return };
            if let Some(resolve) = p.borrow_mut().remove(&(id as u32)) {
                let _ = resolve.call1(&JsValue::NULL, &get(&data, "response"));
            }
        });
        worker.set_onmessage(Some(on_message.as_ref().unchecked_ref()));
        let p = pending.clone();
        // A load or runtime error in the worker fails every open request.
        let on_error = Closure::<dyn Fn(web_sys::Event)>::new(move |_ev: web_sys::Event| {
            let err = super::js::obj(&[
                ("type", "error".into()),
                ("code", "internal".into()),
                ("message", "the key worker failed to start".into()),
            ]);
            for (_, resolve) in p.borrow_mut().drain() {
                let _ = resolve.call1(&JsValue::NULL, &err);
            }
        });
        worker.set_onerror(Some(on_error.as_ref().unchecked_ref()));
        Ok(Self { worker, pending, next: Cell::new(1), _on_message: on_message, _on_error: on_error })
    }

    /// Sends one request. Resolves to the response object, or the worker's error.
    pub async fn call(&self, request: JsValue) -> Result<JsValue, WorkerFail> {
        let id = self.next.get();
        self.next.set(id.wrapping_add(1));
        let pending = self.pending.clone();
        let promise = Promise::new(&mut |resolve, _reject| {
            pending.borrow_mut().insert(id, resolve);
        });
        let msg = super::js::obj(&[("id", JsValue::from(id)), ("request", request)]);
        if self.worker.post_message(&msg).is_err() {
            self.pending.borrow_mut().remove(&id);
            return Err(WorkerFail { code: "internal".into(), message: "could not reach the key worker".into() });
        }
        let resp = JsFuture::from(promise).await.unwrap_or(JsValue::UNDEFINED);
        if get_str(&resp, "type").as_deref() == Some("error") || resp.is_undefined() {
            return Err(WorkerFail {
                code: get_str(&resp, "code").unwrap_or_else(|| "internal".into()),
                message: get_str(&resp, "message").unwrap_or_default(),
            });
        }
        Ok(resp)
    }
}

impl Drop for WorkerClient {
    fn drop(&mut self) {
        self.worker.terminate();
    }
}
