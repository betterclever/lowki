//! Development affordances: URL flags, deep links and the dev panel.
//! This is the only UI module that knows the backend is a mock.
//!
//! URL flags (all optional):
//! - `app=game-x|pixel-market|kingside` and `app_name=...`: the embedding app
//! - `origin=https://...`: the parent origin to accept messages from and reply to
//! - `screen=...`: deep link (see [`DEEP_LINKS`])
//! - `fail=passkey_cancelled|prf_unsupported|network|policy_rejected`
//! - `speed=0..4`: delay multiplier for the mock (0 = instant)
//! - `empty=1`: fresh wallet with no funds or activity
//! - `platform=iphone|mac|windows|android|other`: passkey wording
//! - `returning=1`: Google sign-in is a returning user
//! - `reminder=1`: deep links start with the recovery code not saved
//! - `dev=1`: show the dev panel
//! - `embed=1`: transparent backdrop for iframe embedding
//! - `theme=light|dark`: force a theme (otherwise prefers-color-scheme)
//! - `backend=mock|real`: which backend runs (default `real`; `screen=` implies `mock`)
//! - `google=1`: show "Continue with Google" with the real backend (always shown with the mock)

use std::cell::RefCell;
use std::rc::Rc;

use leptos::prelude::*;
use leptos::task::spawn_local;
use serde_json::json;

use crate::backend::mock::{FailMode, MockBackend, MockControls, fake_address, known_apps, usdc_address};
use crate::backend::{AppInfo, AuthBackend, DelegationPolicy, PolicyRule, TxRequest, VaultDirection};
use crate::platform::Platform;
use crate::protocol::DappToWallet;
use crate::state::{Ctx, ErrorKind, Screen};

thread_local! {
    static MOCK: RefCell<Option<Rc<MockBackend>>> = const { RefCell::new(None) };
}

fn mock() -> Rc<MockBackend> {
    MOCK.with(|m| m.borrow().clone().expect("mock installed"))
}

pub const DEEP_LINKS: &[&str] = &[
    "signin", "email", "unlock", "passkey", "recovery", "home", "send", "vault-in", "vault-out", "settings",
    "confirm-message", "confirm-typed", "confirm-tx", "delegation", "delegation-rejected", "error-passkey",
    "error-prf", "error-network", "error-policy",
];

#[derive(Clone, Debug)]
pub struct Flags {
    pub app: AppInfo,
    pub origin: String,
    pub screen: Option<String>,
    pub fail: Option<FailMode>,
    pub speed: f64,
    pub empty: bool,
    pub platform: Option<Platform>,
    pub returning: bool,
    pub reminder: bool,
    pub dev: bool,
    pub embed: bool,
    pub theme: Option<String>,
    /// The mock backend runs (`backend=mock`, or a `screen=` deep link).
    pub mock: bool,
    pub google: bool,
}

pub fn flags() -> Flags {
    let search = web_sys::window().and_then(|w| w.location().search().ok()).unwrap_or_default();
    let q = web_sys::UrlSearchParams::new_with_str(&search).ok();
    let get = |k: &str| q.as_ref().and_then(|q| q.get(k));
    let on = |k: &str| get(k).is_some_and(|v| v != "0" && v != "false");
    let app_id = get("app").unwrap_or_else(|| "game-x".into());
    let mut app = known_apps().into_iter().find(|a| a.id == app_id).unwrap_or(AppInfo {
        id: app_id.clone(),
        name: app_id.clone(),
        origin: "https://example.app".into(),
        hue: 210,
    });
    if let Some(n) = get("app_name") {
        app.name = n;
    }
    let origin = get("origin").unwrap_or_else(|| "*".into());
    if origin != "*" {
        app.origin = origin.clone();
    }
    let mock = match get("backend").as_deref() {
        Some("mock") => true,
        Some("real") => false,
        _ => get("screen").is_some() || cfg!(feature = "mock-default"),
    };
    let google = match get("google").as_deref() {
        Some(v) => v != "0" && v != "false",
        None => mock,
    };
    Flags {
        app,
        origin,
        mock,
        google,
        screen: get("screen"),
        fail: get("fail").and_then(|f| FailMode::parse(&f)),
        speed: get("speed").and_then(|s| s.parse().ok()).unwrap_or(1.0),
        empty: on("empty"),
        platform: get("platform").and_then(|p| Platform::parse(&p)),
        returning: on("returning"),
        reminder: on("reminder"),
        dev: on("dev"),
        embed: on("embed"),
        theme: get("theme").filter(|t| t == "light" || t == "dark"),
    }
}

/// Install the mock and apply backend-side flags.
pub fn init(m: Rc<MockBackend>, f: &Flags) {
    m.set_fail(f.fail);
    m.set_speed(f.speed);
    m.set_empty(f.empty);
    m.set_returning(f.returning);
    MOCK.with(|cell| *cell.borrow_mut() = Some(m));
}

pub fn sample_message(app: &AppInfo) -> String {
    format!(
        "{} wants you to sign in with your account.\n\nURI: {}\nVersion: 1\nChain: Monad (10143)\nNonce: 8f2c41d07a\nIssued: 2026-11-06T09:41:00Z",
        app.name, app.origin
    )
}

pub fn sample_typed(app: &AppInfo) -> serde_json::Value {
    json!({
        "domain": { "name": format!("{} Market", app.name), "version": "1", "chainId": 10143,
                    "verifyingContract": fake_address(&["market", &app.id]) },
        "primaryType": "Order",
        "types": {
            "EIP712Domain": [
                {"name":"name","type":"string"},{"name":"version","type":"string"},
                {"name":"chainId","type":"uint256"},{"name":"verifyingContract","type":"address"}
            ],
            "Order": [
                {"name":"item","type":"Item"},
                {"name":"currency","type":"address"},
                {"name":"price","type":"uint256"},
                {"name":"seller","type":"address"},
                {"name":"expiresAt","type":"uint256"}
            ],
            "Item": [ {"name":"name","type":"string"}, {"name":"tokenId","type":"uint256"} ]
        },
        "message": {
            "item": { "name": "Iron Sword", "tokenId": "2041" },
            "currency": usdc_address(),
            "price": "12500000",
            "seller": fake_address(&["seller-bjorn"]),
            "expiresAt": "1798675200"
        }
    })
}

pub fn sample_tx(app: &AppInfo) -> TxRequest {
    TxRequest {
        to: fake_address(&["market", &app.id]),
        value: "0".into(),
        data: "0x6a2c4e1b00000000000000000000000000000000000000000000000000000000000007f9".into(),
    }
}

pub fn sample_policy() -> DelegationPolicy {
    DelegationPolicy {
        chain: "monad".into(),
        expires: "2026-12-31".into(),
        rules: vec![PolicyRule::Swap {
            router: "0x7a250d5630b4cf539739df2c5dacb4c659f2488d".into(),
            router_label: None,
            token: "USDC".into(),
            max_per_day: "50".into(),
        }],
    }
}

pub fn sample_bad_policy() -> DelegationPolicy {
    let mut p = sample_policy();
    if let Some(PolicyRule::Swap { max_per_day, .. }) = p.rules.first_mut() {
        *max_per_day = "unlimited".into();
    }
    p
}

/// Apply a `?screen=` deep link: seed the mock so the screen has real data.
pub fn apply_initial(c: Ctx, f: &Flags) {
    if let Some(t) = f.theme.as_deref()
        && let Some(el) = web_sys::window().and_then(|w| w.document()).and_then(|d| d.document_element())
    {
        let _ = el.set_attribute("data-theme", t);
    }
    if let Some(p) = f.platform {
        c.platform.set(p);
    }
    let Some(target) = f.screen.clone().filter(|_| f.mock) else { return };
    let m = mock();
    let app = f.app.clone();
    let fail = f.fail;
    let speed = f.speed;
    let reminder = f.reminder;
    let needs_session = !matches!(target.as_str(), "signin" | "email" | "unlock" | "passkey" | "error-passkey" | "error-prf");
    // Seed instantly, without forced errors, then restore.
    m.set_fail(None);
    m.set_speed(0.0);
    c.screen.set(Screen::SignIn);
    spawn_local(async move {
        if needs_session || target == "recovery" {
            m.seed_signed_in(!reminder);
            if let Ok(s) = m.session(&app).await {
                c.session.set(Some(s));
            }
            c.recovery_reminder.set(reminder);
        }
        if target == "recovery"
            && let Ok(code) = m.create_passkey().await
        {
            c.recovery_code.set(Some(code));
        }
        m.set_fail(fail);
        m.set_speed(speed);
        let s = match target.as_str() {
            "email" => Screen::EmailCode { email: "ada@lovelace.dev".into() },
            "unlock" => Screen::Unlock,
            "passkey" => Screen::SecureWallet,
            "recovery" => Screen::Recovery,
            "home" => Screen::Home,
            "send" => Screen::Send,
            "vault-in" => Screen::Vault(VaultDirection::In),
            "vault-out" => Screen::Vault(VaultDirection::Out),
            "settings" => Screen::Settings,
            "confirm-message" => Screen::ConfirmMessage { id: "dev-1".into(), message: sample_message(&app) },
            "confirm-typed" => Screen::ConfirmTyped { id: "dev-1".into(), data: sample_typed(&app) },
            "confirm-tx" => Screen::ConfirmTx { id: "dev-1".into(), tx: sample_tx(&app) },
            "delegation" => Screen::Delegation { id: "dev-1".into(), policy: sample_policy() },
            "delegation-rejected" => Screen::Delegation { id: "dev-1".into(), policy: sample_bad_policy() },
            "error-passkey" => Screen::Error(ErrorKind::PasskeyCancelled),
            "error-prf" => Screen::Error(ErrorKind::PrfUnsupported),
            "error-network" => Screen::Error(ErrorKind::Network),
            "error-policy" => Screen::Error(ErrorKind::PolicyRejected {
                id: "dev-1".into(),
                reason: "This request has no daily spending limit. Lowki only approves signers with a cap.".into(),
            }),
            _ => Screen::SignIn,
        };
        let base = if needs_session { Screen::Home } else { Screen::SignIn };
        c.reset(base.clone());
        if s != base {
            c.go(s);
        }
    });
}

#[component]
pub fn DevPanel(on_request: Callback<DappToWallet>) -> impl IntoView {
    let open = RwSignal::new(false);
    let fail = RwSignal::new(mock().fail());
    let c = crate::state::ctx();
    let req = move |m: DappToWallet| on_request.run(m);
    let n = RwSignal::new(0u32);
    let next_id = move || {
        n.update(|x| *x += 1);
        format!("dev-{}", n.get_untracked())
    };
    view! {
        <div class="dev">
            <button class="dev-toggle" type="button" aria-expanded=move || open.get().to_string() aria-controls="dev-panel"
                on:click=move |_| open.update(|o| *o = !*o)>"Dev"</button>
            <Show when=move || open.get()>
                <div id="dev-panel" class="dev-panel" role="region" aria-label="Developer controls">
                    <label class="label" for="dev-fail">"Force error"</label>
                    <select id="dev-fail" class="input" on:change=move |ev| {
                        let v = event_target_value(&ev);
                        let f = FailMode::parse(&v);
                        mock().set_fail(f);
                        fail.set(f);
                    }>
                        <option value="" selected=move || fail.get().is_none()>"None"</option>
                        {FailMode::all().into_iter().map(|f| view! {
                            <option value=f.key() selected=move || fail.get() == Some(f)>{f.label()}</option>
                        }).collect_view()}
                    </select>
                    <label class="label" for="dev-speed">"Mock speed"</label>
                    <select id="dev-speed" class="input" on:change=move |ev| {
                        mock().set_speed(event_target_value(&ev).parse().unwrap_or(1.0));
                    }>
                        <option value="1">"Realistic"</option>
                        <option value="0.25">"Fast"</option>
                        <option value="3">"Slow"</option>
                    </select>
                    <p class="label">"Simulate app request"</p>
                    <div class="dev-grid">
                        <button class="btn secondary sm" type="button" on:click=move |_| req(DappToWallet::SignMessage { id: next_id(), message: sample_message(&c.app.get_untracked()) })>"Message"</button>
                        <button class="btn secondary sm" type="button" on:click=move |_| req(DappToWallet::SignTypedData { id: next_id(), typed_data: sample_typed(&c.app.get_untracked()) })>"Typed data"</button>
                        <button class="btn secondary sm" type="button" on:click=move |_| req(DappToWallet::SendTransaction { id: next_id(), tx: sample_tx(&c.app.get_untracked()) })>"Transaction"</button>
                        <button class="btn secondary sm" type="button" on:click=move |_| req(DappToWallet::RequestDelegation { id: next_id(), policy: sample_policy() })>"Delegation"</button>
                        <button class="btn secondary sm" type="button" on:click=move |_| req(DappToWallet::RequestDelegation { id: next_id(), policy: sample_bad_policy() })>"Bad policy"</button>
                    </div>
                    <p class="label">"Jump to"</p>
                    <div class="dev-links">
                        {DEEP_LINKS.iter().map(|s| view! { <a href=format!("?screen={s}&dev=1")>{*s}</a> }).collect_view()}
                    </div>
                </div>
            </Show>
        </div>
    }
}
