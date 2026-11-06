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

/// Request types this build understands, announced in `ready.capabilities`. Apps use it to
/// tell a newer wallet from an older one (which silently drops unknown request types).
pub const CAPABILITIES: &[&str] = &[
    "sign_in",
    "get_session",
    "sign_message",
    "sign_typed_data",
    "send_transaction",
    "send_privately",
    "request_delegation",
    "sign_out",
    "cancel",
];

/// App -> wallet.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum DappToWallet {
    /// Open the sign-in sheet.
    SignIn { id: String },
    /// personal_sign of a UTF-8 message.
    SignMessage { id: String, message: String },
    /// eth_signTypedData_v4 payload (domain, types, primaryType, message).
    /// `chain_id` (optional): the app's current chain; when the domain names a chain too,
    /// the two must agree.
    SignTypedData {
        id: String,
        typed_data: serde_json::Value,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        chain_id: Option<u64>,
    },
    /// `chain_id` (optional): the chain the app expects the transaction on. A wallet on
    /// another chain answers `rejected { reason: "unsupported_chain" }`.
    SendTransaction {
        id: String,
        tx: TxRequest,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        chain_id: Option<u64>,
    },
    /// The app server asks for a policy-limited signer on this app account.
    RequestDelegation { id: String, policy: DelegationPolicy },
    SignOut { id: String },
    /// Silent session query: answers `authenticated` (with a fresh access token) or
    /// `rejected { not_signed_in }`, and never shows UI.
    GetSession { id: String },
    /// The app closed its wallet modal: drop the open request `id` (sheet closes, the
    /// request is answered `rejected { user_rejected }`). Nothing happens for other ids.
    Cancel { id: String },
    /// A private transfer from the user's private balance (Privacy Pools), after the user
    /// confirms. `amount` is a decimal string ("12.5"), `asset` a symbol ("USDC", "ETH").
    /// Answered with `private_sent`.
    SendPrivately {
        id: String,
        to: String,
        amount: String,
        asset: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        chain_id: Option<u64>,
    },
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
    /// The request's `chain_id` is not the chain this wallet is on.
    UnsupportedChain,
    /// Malformed request (bad amount, unknown asset, chain mismatch inside typed data).
    InvalidRequest,
}

/// Wallet -> app.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum WalletToDapp {
    /// The wallet frame loaded and listens for requests. `capabilities`: the request types
    /// it understands ([`CAPABILITIES`]).
    Ready { app_id: String, capabilities: Vec<String> },
    /// Sign-in finished. Every value is scoped to this app only.
    /// - `created_at`: ISO 8601, when this wallet first saw this app account (approximate:
    ///   the server's per-app creation time isn't in the token response yet).
    /// - `is_new_user`: the wallet was created in this sign-in.
    /// - `email`: present only when the user chose to share it with this app.
    Authenticated {
        id: Option<String>,
        address: String,
        user_id: String,
        access_token: String,
        created_at: String,
        is_new_user: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        email: Option<String>,
    },
    MessageSigned { id: String, signature: String },
    TypedDataSigned { id: String, signature: String },
    TransactionSent { id: String, hash: String, sponsored: bool },
    /// Answer to `send_privately`.
    PrivateSent { id: String, hash: String },
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
            | DappToWallet::SendPrivately { id, .. }
            | DappToWallet::SignOut { id }
            | DappToWallet::GetSession { id }
            | DappToWallet::Cancel { id } => id,
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

/// Where the wallet sends replies: the opener (popup) or the embedding parent (iframe),
/// restricted to the app's origin.
pub fn post(msg: &WalletToDapp, target_origin: &str) {
    let Some(win) = web_sys::window() else { return };
    // A cross-origin opener is a WindowProxy that fails `instanceof Window`, so `dyn_into`
    // would refuse it: check for null and cast unchecked (postMessage works cross-origin).
    let opener = win.opener().ok().filter(|o| !o.is_null() && !o.is_undefined());
    let target = if let Some(opener) = opener {
        opener.unchecked_into::<web_sys::Window>()
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

    #[test]
    fn v1_messages_still_parse() {
        // No chain_id: the v1 shape.
        let m = r#"{"lowki":1,"type":"send_transaction","id":"t","tx":{"to":"0x01","value":"1","data":"0x"}}"#;
        assert!(matches!(DappToWallet::from_json(m), Some(DappToWallet::SendTransaction { chain_id: None, .. })));
        let m = r#"{"lowki":1,"type":"sign_typed_data","id":"t","typed_data":{}}"#;
        assert!(matches!(DappToWallet::from_json(m), Some(DappToWallet::SignTypedData { chain_id: None, .. })));
    }

    #[test]
    fn additions_parse() {
        let m = r#"{"lowki":1,"type":"send_transaction","id":"t","tx":{"to":"0x01","value":"1","data":"0x"},"chain_id":11155111}"#;
        assert!(matches!(DappToWallet::from_json(m), Some(DappToWallet::SendTransaction { chain_id: Some(11_155_111), .. })));
        assert_eq!(DappToWallet::from_json(r#"{"lowki":1,"type":"get_session","id":"g"}"#), Some(DappToWallet::GetSession { id: "g".into() }));
        assert_eq!(DappToWallet::from_json(r#"{"lowki":1,"type":"cancel","id":"c"}"#), Some(DappToWallet::Cancel { id: "c".into() }));
        let m = r#"{"lowki":1,"type":"send_privately","id":"p","to":"0xab","amount":"5","asset":"USDC"}"#;
        assert_eq!(
            DappToWallet::from_json(m),
            Some(DappToWallet::SendPrivately { id: "p".into(), to: "0xab".into(), amount: "5".into(), asset: "USDC".into(), chain_id: None })
        );
    }

    #[test]
    fn wallet_messages() {
        let r = WalletToDapp::Ready { app_id: "a".into(), capabilities: CAPABILITIES.iter().map(|s| s.to_string()).collect() }.to_json();
        assert!(r.contains(r#""capabilities":["sign_in","get_session""#) && r.contains("send_privately"), "{r}");
        let a = WalletToDapp::Authenticated {
            id: None,
            address: "0x1".into(),
            user_id: "did:lowki:1".into(),
            access_token: "t".into(),
            created_at: "2026-01-01T00:00:00.000Z".into(),
            is_new_user: true,
            email: None,
        }
        .to_json();
        assert!(a.contains(r#""is_new_user":true"#) && a.contains("created_at") && !a.contains("email"), "{a}");
        let p = WalletToDapp::PrivateSent { id: "p".into(), hash: "0xab".into() }.to_json();
        assert!(p.contains(r#""type":"private_sent""#));
        let j = WalletToDapp::Rejected { id: "x".into(), reason: RejectReason::UnsupportedChain, message: String::new() }.to_json();
        assert!(j.contains("unsupported_chain"));
    }
}
