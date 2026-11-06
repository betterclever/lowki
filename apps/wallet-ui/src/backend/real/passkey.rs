//! WebAuthn ceremonies with the PRF extension.
//!
//! The server builds the options (lowki-server, webauthn-rs) and verifies the result. This
//! side converts base64url fields to bytes, adds the PRF evaluation with Lowki's fixed salt,
//! and returns the PRF output separately: it goes to the key worker and **never** to the
//! server (`clientExtensionResults.prf.results` is removed before posting).

use serde_json::Value;
use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::JsFuture;

use super::codec::{b64u, unb64u};
use super::js::{bytes, get, obj, parse_json, set, stringify, to_bytes};

/// The PRF input (`eval.first`). Fixed and versioned: every device must use the same salt to
/// get the same PRF output for a credential, so changing it orphans every wrapped seed.
pub const PRF_SALT: &[u8] = b"lowki/prf/v1";

#[derive(Debug)]
pub enum PasskeyError {
    /// The user closed the prompt (or it timed out): `NotAllowedError`.
    Cancelled,
    /// This authenticator already holds a passkey for this account (`InvalidStateError`).
    AlreadyRegistered,
    /// WebAuthn itself is unavailable here (`SecurityError`, `NotSupportedError`, no API).
    Unavailable(String),
}

pub struct Ceremony {
    /// The credential as JSON for the server, with PRF results removed.
    pub credential: Value,
    /// base64url credential ID.
    pub id: String,
    /// 32-byte PRF output, when the authenticator returned one.
    pub prf: Option<Vec<u8>>,
}

/// Recursively drops `null` members (webauthn-rs serializes absent options as null, which
/// `navigator.credentials` rejects for enum members).
fn drop_nulls(v: &mut Value) {
    match v {
        Value::Object(m) => {
            m.retain(|_, x| !x.is_null());
            m.values_mut().for_each(drop_nulls);
        }
        Value::Array(a) => a.iter_mut().for_each(drop_nulls),
        _ => {}
    }
}

fn b64_field_to_bytes(target: &JsValue, key: &str) {
    if let Some(s) = get(target, key).as_string()
        && let Some(b) = unb64u(&s)
    {
        set(target, key, &bytes(&b));
    }
}

fn convert_list(pk: &JsValue, key: &str) {
    let list = get(pk, key);
    if let Some(arr) = list.dyn_ref::<js_sys::Array>() {
        for c in arr.iter() {
            b64_field_to_bytes(&c, "id");
        }
    }
}

fn prf_ext() -> JsValue {
    obj(&[("prf", obj(&[("eval", obj(&[("first", bytes(PRF_SALT))]))]))])
}

/// `options` is the server's `{publicKey: {...}}`.
fn public_key(options: &Value, allow: Option<&str>) -> Result<JsValue, PasskeyError> {
    let mut pk = options.get("publicKey").cloned().ok_or_else(|| PasskeyError::Unavailable("bad options".into()))?;
    drop_nulls(&mut pk);
    let pk = parse_json(&pk.to_string());
    b64_field_to_bytes(&pk, "challenge");
    let user = get(&pk, "user");
    if user.is_object() {
        b64_field_to_bytes(&user, "id");
    }
    convert_list(&pk, "excludeCredentials");
    if let Some(id) = allow.and_then(unb64u) {
        let arr = js_sys::Array::new();
        arr.push(&obj(&[("type", "public-key".into()), ("id", bytes(&id))]));
        set(&pk, "allowCredentials", &arr);
    } else {
        convert_list(&pk, "allowCredentials");
    }
    // Keep the server's extensions (e.g. `prf: {}` at registration) and evaluate PRF.
    let ext = get(&pk, "extensions");
    let ext = if ext.is_object() { ext } else { obj(&[]) };
    set(&ext, "prf", &get(&prf_ext(), "prf"));
    set(&pk, "extensions", &ext);
    Ok(pk)
}

fn credentials() -> Result<JsValue, PasskeyError> {
    let win: JsValue = web_sys::window().ok_or_else(|| PasskeyError::Unavailable("no window".into()))?.into();
    let c = get(&get(&win, "navigator"), "credentials");
    if c.is_undefined() || get(&win, "PublicKeyCredential").is_undefined() {
        return Err(PasskeyError::Unavailable("WebAuthn is not available".into()));
    }
    Ok(c)
}

fn map_err(e: JsValue) -> PasskeyError {
    let name = get(&e, "name").as_string().unwrap_or_default();
    match name.as_str() {
        "NotAllowedError" | "AbortError" => PasskeyError::Cancelled,
        "InvalidStateError" => PasskeyError::AlreadyRegistered,
        _ => PasskeyError::Unavailable(format!("{name}: {}", get(&e, "message").as_string().unwrap_or_default())),
    }
}

async fn ceremony(method: &str, pk: JsValue) -> Result<Ceremony, PasskeyError> {
    let c = credentials()?;
    let f: js_sys::Function = get(&c, method).dyn_into().map_err(|_| PasskeyError::Unavailable("no WebAuthn".into()))?;
    let promise: js_sys::Promise = f
        .call1(&c, &obj(&[("publicKey", pk)]))
        .map_err(map_err)?
        .dyn_into()
        .map_err(|_| PasskeyError::Unavailable("no promise".into()))?;
    let cred = JsFuture::from(promise).await.map_err(map_err)?;
    if cred.is_null() || cred.is_undefined() {
        return Err(PasskeyError::Cancelled);
    }
    let ext = call0(&cred, "getClientExtensionResults").unwrap_or(JsValue::UNDEFINED);
    let first = get(&get(&get(&ext, "prf"), "results"), "first");
    let prf = to_bytes(&first).filter(|b| b.len() == 32);
    let json = credential_json(&cred);
    let id = get(&cred, "id").as_string().unwrap_or_default();
    Ok(Ceremony { credential: json, id, prf })
}

fn call0(target: &JsValue, method: &str) -> Option<JsValue> {
    let f: js_sys::Function = get(target, method).dyn_into().ok()?;
    f.call0(target).ok()
}

/// `PublicKeyCredential.toJSON()` where available, else the same shape by hand.
fn credential_json(cred: &JsValue) -> Value {
    let js = call0(cred, "toJSON").filter(JsValue::is_object).unwrap_or_else(|| {
        let r = get(cred, "response");
        let enc = |k: &str| to_bytes(&get(&r, k)).map(|b| JsValue::from_str(&b64u(&b))).unwrap_or(JsValue::NULL);
        let response = obj(&[
            ("clientDataJSON", enc("clientDataJSON")),
            ("attestationObject", enc("attestationObject")),
            ("authenticatorData", enc("authenticatorData")),
            ("signature", enc("signature")),
            ("userHandle", enc("userHandle")),
        ]);
        obj(&[
            ("id", get(cred, "id")),
            ("rawId", to_bytes(&get(cred, "rawId")).map(|b| JsValue::from_str(&b64u(&b))).unwrap_or(JsValue::NULL)),
            ("type", "public-key".into()),
            ("response", response),
            ("clientExtensionResults", obj(&[])),
        ])
    });
    let mut v: Value = serde_json::from_str(&stringify(&js)).unwrap_or(Value::Null);
    // The PRF output stays in the browser.
    for key in ["clientExtensionResults", "extensions"] {
        if let Some(prf) = v.get_mut(key).and_then(|e| e.get_mut("prf")).and_then(Value::as_object_mut) {
            prf.remove("results");
        }
    }
    drop_nulls(&mut v);
    v
}

/// `navigator.credentials.create` with the server's options, PRF evaluated if possible.
pub async fn create(options: &Value) -> Result<Ceremony, PasskeyError> {
    let mut c = ceremony("create", public_key(options, None)?).await?;
    same_origin_registration(&mut c.credential);
    Ok(c)
}

/// STOPGAP until lowki-server accepts registrations made inside an app's iframe.
///
/// The wallet runs in a cross-origin iframe, so the browser sets `crossOrigin: true` (and
/// `topOrigin`) in `clientDataJSON`. webauthn-rs 0.5 rejects every such registration
/// (`CredentialCrossOrigin`, not configurable). Registration uses `attestation: "none"`, so
/// nothing signs `clientDataJSON` and the server can't rely on the flag anyway; this clears
/// it and keeps type, challenge and origin, which the server does check. Assertions are not
/// touched (they are signed, and webauthn-rs accepts cross-origin assertions).
/// Proper fix (server): accept `crossOrigin: true` when `topOrigin` is a registered app origin.
fn same_origin_registration(cred: &mut Value) {
    let attested = cred["response"]["attestationObject"].as_str().and_then(unb64u).is_some_and(|a| {
        // CBOR map with "fmt": "none" (text string 0x63 'fmt', then 0x64 'none').
        a.windows(9).any(|w| w == b"cfmtdnone")
    });
    let Some(cdj) = cred["response"]["clientDataJSON"].as_str().and_then(unb64u) else { return };
    let Ok(mut cd) = serde_json::from_slice::<Value>(&cdj) else { return };
    if !attested || cd.get("crossOrigin") != Some(&Value::Bool(true)) {
        return;
    }
    if let Some(m) = cd.as_object_mut() {
        m.insert("crossOrigin".into(), Value::Bool(false));
        m.remove("topOrigin");
    }
    cred["response"]["clientDataJSON"] = Value::String(b64u(cd.to_string().as_bytes()));
}

/// `navigator.credentials.get`. With `allow`, only that credential (just created) is offered.
pub async fn get_assertion(options: &Value, allow: Option<&str>) -> Result<Ceremony, PasskeyError> {
    ceremony("get", public_key(options, allow)?).await
}
