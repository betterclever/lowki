//! Small helpers for building and reading plain JS objects.

use js_sys::{Object, Reflect, Uint8Array};
use wasm_bindgen::{JsCast, JsValue};

pub fn obj(pairs: &[(&str, JsValue)]) -> JsValue {
    let o = Object::new();
    for (k, v) in pairs {
        let _ = Reflect::set(&o, &JsValue::from_str(k), v);
    }
    o.into()
}

pub fn get(v: &JsValue, key: &str) -> JsValue {
    Reflect::get(v, &JsValue::from_str(key)).unwrap_or(JsValue::UNDEFINED)
}

pub fn set(target: &JsValue, key: &str, v: &JsValue) {
    let _ = Reflect::set(target, &JsValue::from_str(key), v);
}

pub fn get_str(v: &JsValue, key: &str) -> Option<String> {
    get(v, key).as_string()
}

/// Reads a `Uint8Array` or `ArrayBuffer` field.
pub fn get_bytes(v: &JsValue, key: &str) -> Option<Vec<u8>> {
    let f = get(v, key);
    to_bytes(&f)
}

pub fn to_bytes(f: &JsValue) -> Option<Vec<u8>> {
    if f.is_instance_of::<Uint8Array>() || f.is_instance_of::<js_sys::ArrayBuffer>() {
        return Some(Uint8Array::new(f).to_vec());
    }
    // Any other ArrayBufferView (DataView, other typed arrays).
    let buf = get(f, "buffer");
    if f.is_object() && buf.is_instance_of::<js_sys::ArrayBuffer>() {
        let off = get(f, "byteOffset").as_f64().unwrap_or(0.0) as u32;
        let len = get(f, "byteLength").as_f64().unwrap_or(0.0) as u32;
        return Some(Uint8Array::new_with_byte_offset_and_length(&buf, off, len).to_vec());
    }
    None
}

pub fn bytes(b: &[u8]) -> JsValue {
    Uint8Array::from(b).into()
}

/// Overwrites a JS byte array we handed out or received (best effort: copies the engine
/// made can't be reached).
pub fn wipe(v: &JsValue) {
    if let Some(a) = v.dyn_ref::<Uint8Array>() {
        a.fill(0, 0, a.length());
    }
}

/// `JSON.parse`, returning `undefined` on error.
pub fn parse_json(s: &str) -> JsValue {
    js_sys::JSON::parse(s).unwrap_or(JsValue::UNDEFINED)
}

pub fn stringify(v: &JsValue) -> String {
    js_sys::JSON::stringify(v).ok().and_then(|s| s.as_string()).unwrap_or_default()
}
