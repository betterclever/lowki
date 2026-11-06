//! postMessage protocol between an embedding app (the "dapp") and the Lowki
//! wallet running in an iframe or popup on the Lowki origin.
//!
//! Every message is a JSON object `{ "lowki": 1, "type": "...", ...fields }`.
//! Requests from the app carry an `id`; the wallet answers with the same `id`.
//! The wallet only ever reveals the pairwise address and user ID for the
//! embedding app, never the user's identity or other app accounts.
//!
//! See `apps/wallet-ui/README.md` for the full table.

use serde::{Deserialize, Serialize};
use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::*;

use crate::backend::{DelegationPolicy, TxRequest};

pub const PROTOCOL_VERSION: u8 = 1;

/// App -> wallet.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum DappToWallet {
    /// Open the sign-in sheet.
    SignIn { id: String },
    /// personal_sign of a UTF-8 message.
    SignMessage { id: String, message: String },
    /// eth_signTypedData_v4 payload (domain, types, primaryType, message).
    SignTypedData { id: String, typed_data: serde_json::Value },
    SendTransaction { id: String, tx: TxRequest },
    /// The app server asks for a policy-limited signer on this app account.
    RequestDelegation { id: String, policy: DelegationPolicy },
    SignOut { id: String },
}

/// Why a request did not complete.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RejectReason {
    /// The user declined.
    UserRejected,
    /// Lowki refused the delegation policy.
    PolicyRejected,
    NotSignedIn,
    NetworkError,
    /// Another request is already open.
    Busy,
}

/// Wallet -> app.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum WalletToDapp {
    /// The wallet frame loaded and listens for requests.
    Ready { app_id: String },
    /// Sign-in finished. All three values are scoped to this app only.
    Authenticated { id: Option<String>, address: String, user_id: String, access_token: String },
    MessageSigned { id: String, signature: String },
    TypedDataSigned { id: String, signature: String },
    TransactionSent { id: String, hash: String, sponsored: bool },
    /// Signer created in the enclave. Nothing was posted on chain.
    DelegationApproved { id: String, delegation_id: String, expires: String, attestation_build: String },
    /// The user revoked a delegation from Settings.
    DelegationRevoked { delegation_id: String },
    Rejected { id: String, reason: RejectReason, message: String },
    SignedOut { id: Option<String> },
}

#[derive(Serialize, Deserialize)]
struct Envelope<T> {
    lowki: u8,
    #[serde(flatten)]
    msg: T,
}

impl DappToWallet {
    pub fn id(&self) -> &str {
        match self {
            DappToWallet::SignIn { id }
            | DappToWallet::SignMessage { id, .. }
            | DappToWallet::SignTypedData { id, .. }
            | DappToWallet::SendTransaction { id, .. }
            | DappToWallet::RequestDelegation { id, .. }
            | DappToWallet::SignOut { id } => id,
        }
    }

    /// Parses an incoming JSON string. Non-Lowki messages return `None`.
    pub fn from_json(s: &str) -> Option<Self> {
        let env: Envelope<DappToWallet> = serde_json::from_str(s).ok()?;
        (env.lowki == PROTOCOL_VERSION).then_some(env.msg)
    }
}

impl WalletToDapp {
    pub fn to_json(&self) -> String {
        serde_json::to_string(&Envelope { lowki: PROTOCOL_VERSION, msg: self }).unwrap_or_default()
    }
}

/// Where the wallet sends replies: the embedding parent (iframe) or the
/// opener (popup), restricted to the app's origin.
pub fn post(msg: &WalletToDapp, target_origin: &str) {
    let Some(win) = web_sys::window() else { return };
    let target = if let Ok(Some(opener)) = win.opener().map(|o| o.dyn_into::<web_sys::Window>().ok()) {
        opener
    } else if let Ok(Some(parent)) = win.parent() {
        if parent == win {
            return; // standalone: nobody to tell
        }
        parent
    } else {
        return;
    };
    let json = js_sys::JSON::parse(&msg.to_json()).unwrap_or(JsValue::NULL);
    let _ = target.post_message(&json, target_origin);
}

/// Installs a `message` listener. Messages from origins other than
/// `allowed_origin` are ignored ("*" accepts any, for local demos only).
pub fn listen(allowed_origin: String, on_msg: impl Fn(DappToWallet) + 'static) {
    let Some(win) = web_sys::window() else { return };
    let cb = Closure::<dyn Fn(web_sys::MessageEvent)>::new(move |ev: web_sys::MessageEvent| {
        if allowed_origin != "*" && ev.origin() != allowed_origin {
            return;
        }
        let data = ev.data();
        let json = if let Some(s) = data.as_string() {
            s
        } else {
            match js_sys::JSON::stringify(&data) {
                Ok(s) => s.as_string().unwrap_or_default(),
                Err(_) => return,
            }
        };
        if let Some(m) = DappToWallet::from_json(&json) {
            on_msg(m);
        }
    });
    let _ = win.add_event_listener_with_callback("message", cb.as_ref().unchecked_ref());
    cb.forget();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip() {
        let m = r#"{"lowki":1,"type":"sign_message","id":"r1","message":"hi"}"#;
        assert_eq!(DappToWallet::from_json(m), Some(DappToWallet::SignMessage { id: "r1".into(), message: "hi".into() }));
        let out = WalletToDapp::Rejected { id: "r1".into(), reason: RejectReason::UserRejected, message: "x".into() }.to_json();
        assert!(out.contains(r#""lowki":1"#) && out.contains(r#""type":"rejected""#) && out.contains("user_rejected"));
    }
}
