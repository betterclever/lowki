//! lowki-server client (cookie API). Calls go to the Lowki origin itself (the server sits
//! behind the same host as the wallet, or behind `trunk serve`'s proxy in dev), with
//! cookies and the `x-lowki-csrf` header. See `crates/lowki-server/README.md`.

use std::cell::RefCell;

use serde_json::Value;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::JsFuture;

/// Base URL of lowki-server. Empty = same origin as the wallet. Set at build time with
/// `LOWKI_SERVER_URL`; never from the URL, because the embedding app controls that.
pub const SERVER_URL: &str = match option_env!("LOWKI_SERVER_URL") {
    Some(u) => u,
    None => "",
};

#[derive(Clone, Debug)]
pub enum ServerError {
    /// Could not reach the server, or it answered with something that isn't JSON.
    Network,
    /// The server answered with an error status.
    Status { status: u16, code: String, message: String },
}

#[derive(Default)]
pub struct Server {
    csrf: RefCell<Option<String>>,
}

impl Server {
    pub fn set_csrf(&self, t: Option<String>) {
        *self.csrf.borrow_mut() = t;
    }

    /// Remembers the CSRF token from any sign-in or session response.
    fn note(&self, v: &Value) {
        if let Some(t) = v.get("csrf_token").and_then(Value::as_str) {
            self.set_csrf(Some(t.to_string()));
        }
    }

    pub async fn get(&self, path: &str) -> Result<Value, ServerError> {
        self.call("GET", path, None).await
    }

    pub async fn post(&self, path: &str, body: Value) -> Result<Value, ServerError> {
        self.call("POST", path, Some(body)).await
    }

    pub async fn put(&self, path: &str, body: Value) -> Result<Value, ServerError> {
        self.call("PUT", path, Some(body)).await
    }

    /// Returns the JSON body for 2xx (202 included).
    pub async fn call(&self, method: &str, path: &str, body: Option<Value>) -> Result<Value, ServerError> {
        let (status, v) = self.raw(method, path, body.as_ref()).await?;
        if !(200..300).contains(&status) {
            return Err(ServerError::Status {
                status,
                code: v.get("error").and_then(Value::as_str).unwrap_or_default().to_string(),
                message: v.get("message").and_then(Value::as_str).unwrap_or_default().to_string(),
            });
        }
        self.note(&v);
        Ok(v)
    }

    async fn raw(&self, method: &str, path: &str, body: Option<&Value>) -> Result<(u16, Value), ServerError> {
        let init = web_sys::RequestInit::new();
        init.set_method(method);
        init.set_credentials(web_sys::RequestCredentials::Include);
        init.set_cache(web_sys::RequestCache::NoStore);
        let headers = web_sys::Headers::new().map_err(|_| ServerError::Network)?;
        let _ = headers.set("accept", "application/json");
        if let Some(b) = body {
            let _ = headers.set("content-type", "application/json");
            init.set_body(&wasm_bindgen::JsValue::from_str(&b.to_string()));
        }
        if let Some(t) = self.csrf.borrow().as_deref() {
            let _ = headers.set("x-lowki-csrf", t);
        }
        init.set_headers(&headers);
        let url = format!("{SERVER_URL}{path}");
        let req = web_sys::Request::new_with_str_and_init(&url, &init).map_err(|_| ServerError::Network)?;
        let win = web_sys::window().ok_or(ServerError::Network)?;
        let resp = JsFuture::from(win.fetch_with_request(&req)).await.map_err(|_| ServerError::Network)?;
        let resp: web_sys::Response = resp.dyn_into().map_err(|_| ServerError::Network)?;
        let status = resp.status();
        let text = JsFuture::from(resp.text().map_err(|_| ServerError::Network)?)
            .await
            .ok()
            .and_then(|t| t.as_string())
            .unwrap_or_default();
        let v: Value = serde_json::from_str(&text).map_err(|_| ServerError::Network)?;
        Ok((status, v))
    }
}
