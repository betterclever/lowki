//! Root component: screen routing, app requests, focus management.

use leptos::prelude::*;
use leptos::task::spawn_local;
use wasm_bindgen::JsCast;

use crate::backend::{Amount, Token, WalletError, backend};
use crate::dev::{self, DevPanel, Flags};
use crate::platform::Platform;
use crate::protocol::{self, DappToWallet, RejectReason, WalletToDapp};
use crate::screens::confirm::{ConfirmMessage, ConfirmPrivate, ConfirmTx, ConfirmTyped};
use crate::screens::delegation::DelegationConsent;
use crate::screens::errors::ErrorScreen;
use crate::screens::home::Home;
use crate::screens::wallet_home::{PickAccount, SweepScreen, WalletHome};
use crate::screens::onboarding::{EmailCode, RecoverCode, Recovery, SecureWallet, SignIn, Unlock};
use crate::screens::send::{Send, VaultMoveScreen};
use crate::screens::settings::Settings;
use crate::state::{Ctx, Screen};

fn is_request_screen(s: &Screen) -> bool {
    request_id(s).is_some()
}

/// The app request a screen answers, if it is a request sheet.
fn request_id(s: &Screen) -> Option<&str> {
    match s {
        Screen::ConfirmMessage { id, .. }
        | Screen::ConfirmTyped { id, .. }
        | Screen::ConfirmTx { id, .. }
        | Screen::ConfirmPrivate { id, .. }
        | Screen::Delegation { id, .. } => Some(id),
        _ => None,
    }
}

thread_local! {
    /// The chain this wallet sends on, or `None` (the mock accepts any chain).
    static WALLET_CHAIN: std::cell::Cell<Option<u64>> = const { std::cell::Cell::new(None) };
}

/// The real backend's chain: `LOWKI_CHAIN_ID` at build time, else Sepolia (as `real::chain`).
fn real_chain_id() -> u64 {
    option_env!("LOWKI_CHAIN_ID").and_then(|s| s.parse().ok()).unwrap_or(crate::backend::real::chain::CHAIN_ID)
}

fn chain_ok(c: Ctx, id: &str, chain_id: Option<u64>) -> bool {
    match (chain_id, WALLET_CHAIN.with(|w| w.get())) {
        (Some(want), Some(have)) if want != have => {
            c.reject(id, RejectReason::UnsupportedChain, &format!("This wallet is on chain {have}, not {want}"));
            false
        }
        _ => true,
    }
}

/// `domain.chainId` of typed data, as a number (it may be a number or a decimal/hex string).
fn typed_chain(v: &serde_json::Value) -> Option<u64> {
    let c = v.get("domain")?.get("chainId")?;
    match c {
        serde_json::Value::Number(n) => n.as_u64(),
        serde_json::Value::String(s) => match s.strip_prefix("0x") {
            Some(h) => u64::from_str_radix(h, 16).ok(),
            None => s.parse().ok(),
        },
        _ => None,
    }
}

/// `exp` of a JWT (unverified: it is our own server's token, read only to decide on refresh).
fn jwt_exp(token: &str) -> Option<f64> {
    let payload = crate::backend::real::codec::unb64u(token.split('.').nth(1)?)?;
    serde_json::from_slice::<serde_json::Value>(&payload).ok()?.get("exp")?.as_f64()
}

fn token_of(asset: &str) -> Option<Token> {
    match asset.to_ascii_uppercase().as_str() {
        "USDC" => Some(Token::Usdc),
        "ETH" => Some(Token::Eth),
        "MON" => Some(Token::Mon),
        _ => None,
    }
}

/// A request arrived from the app (postMessage or dev panel).
pub fn handle_request(c: Ctx, m: DappToWallet) {
    let id = m.id().to_string();
    match m {
        DappToWallet::SignIn { .. } => {
            c.sign_in_request.set(Some(id));
            match c.session.get_untracked() {
                Some(s) => c.announce_session(&s),
                None => c.reset(Screen::SignIn),
            }
            return;
        }
        DappToWallet::GetSession { .. } => {
            // Silent: never touches the screen.
            let Some(old) = c.session.get_untracked() else {
                c.reject(&id, RejectReason::NotSignedIn, "Not signed in");
                return;
            };
            // A token with a minute left is answered as is. An older one is re-issued (the
            // backend may then need the passkey if the key worker auto-locked; in a hidden
            // frame the browser refuses that at once and the app gets not_signed_in).
            if jwt_exp(&old.access_token).is_some_and(|exp| exp - 60.0 > js_sys::Date::now() / 1000.0) {
                c.post(c.authenticated(&old, Some(id)));
                return;
            }
            spawn_local(async move {
                match backend().session(&c.app.get_untracked()).await {
                    Ok(s) => {
                        c.post(c.authenticated(&s, Some(id)));
                        if s != old {
                            c.session.set(Some(s));
                        }
                    }
                    Err(WalletError::Network) => c.reject(&id, RejectReason::NetworkError, "Could not reach Lowki"),
                    // Locked (auto-lock) or the server session ended: the app must sign in again.
                    Err(_) => c.reject(&id, RejectReason::NotSignedIn, "Sign in again"),
                }
            });
            return;
        }
        DappToWallet::Cancel { .. } => {
            let screen = c.screen.get_untracked();
            if request_id(&screen) == Some(id.as_str()) {
                c.reject(&id, RejectReason::UserRejected, "Cancelled by the app");
                c.done();
            } else if c.sign_in_request.get_untracked().as_deref() == Some(id.as_str()) {
                c.sign_in_request.set(None);
                c.reject(&id, RejectReason::UserRejected, "Cancelled by the app");
                if c.session.get_untracked().is_none() {
                    c.reset(Screen::SignIn);
                }
            }
            return;
        }
        DappToWallet::SignOut { .. } => {
            spawn_local(async move {
                let _ = backend().sign_out().await;
                c.session.set(None);
                c.clear_sign_in();
                c.post(WalletToDapp::SignedOut { id: Some(id) });
                c.reset(Screen::SignIn);
            });
            return;
        }
        _ => {}
    }
    if c.session.get_untracked().is_none() {
        c.reject(&id, RejectReason::NotSignedIn, "Sign in first");
        return;
    }
    if is_request_screen(&c.screen.get_untracked()) {
        c.reject(&id, RejectReason::Busy, "Another request is open");
        return;
    }
    let s = match m {
        DappToWallet::SignMessage { id, message } => Screen::ConfirmMessage { id, message },
        DappToWallet::SignTypedData { id, typed_data, chain_id } => {
            if let (Some(want), Some(domain)) = (chain_id, typed_chain(&typed_data))
                && want != domain
            {
                c.reject(&id, RejectReason::InvalidRequest, &format!("Typed data is for chain {domain}, the app is on {want}"));
                return;
            }
            Screen::ConfirmTyped { id, data: typed_data }
        }
        DappToWallet::SendTransaction { id, tx, chain_id } => {
            if !chain_ok(c, &id, chain_id) {
                return;
            }
            Screen::ConfirmTx { id, tx }
        }
        DappToWallet::SendPrivately { id, to, amount, asset, chain_id } => {
            if !chain_ok(c, &id, chain_id) {
                return;
            }
            let Some(amount) = token_of(&asset).and_then(|t| Amount::parse(t, &amount)).filter(|a| a.units > 0) else {
                c.reject(&id, RejectReason::InvalidRequest, &format!("Can't send \"{amount}\" {asset}"));
                return;
            };
            Screen::ConfirmPrivate { id, to, amount }
        }
        DappToWallet::RequestDelegation { id, policy } => Screen::Delegation { id, policy },
        DappToWallet::SignIn { .. }
        | DappToWallet::SignOut { .. }
        | DappToWallet::GetSession { .. }
        | DappToWallet::Cancel { .. } => return,
    };
    c.go(s);
}

fn focus_heading() {
    let Some(doc) = web_sys::window().and_then(|w| w.document()) else { return };
    if let Ok(Some(el)) = doc.query_selector("#main h1")
        && let Ok(el) = el.dyn_into::<web_sys::HtmlElement>()
    {
        let _ = el.focus();
    }
    if let Some(w) = web_sys::window() {
        w.scroll_to_with_x_and_y(0.0, 0.0);
    }
}

#[component]
pub fn App(flags: Flags) -> impl IntoView {
    let c = Ctx::new(flags.app.clone(), flags.platform.unwrap_or_else(Platform::detect), flags.origin.clone());
    c.show_google.set(flags.google);
    c.home.set(flags.home);
    provide_context(c);
    // Show the app's registered name (lowki-server), not its ID or a name the page chose.
    // The origin under it is the browser's, so a look-alike name still shows its real origin.
    if !flags.home && !flags.mock && !flags.app.id.is_empty() {
        let id = flags.app.id.clone();
        leptos::task::spawn_local(async move {
            if let Some((name, _registered_origin)) = backend().app_info(&id).await
                && !name.is_empty()
            {
                c.app.update(|a| a.name = name);
            }
        });
    }
    dev::apply_initial(c, &flags);

    WALLET_CHAIN.with(|w| w.set((!flags.mock).then(real_chain_id)));
    protocol::listen(flags.origin.clone(), move |m| handle_request(c, m));
    c.post(WalletToDapp::Ready {
        app_id: flags.app.id.clone(),
        capabilities: protocol::CAPABILITIES.iter().map(|s| s.to_string()).collect(),
    });

    // Move focus to the new screen's heading so screen readers and keyboard
    // users land in the right place.
    Effect::new(move |prev: Option<()>| {
        c.screen.track();
        if prev.is_some() {
            request_animation_frame(focus_heading);
        }
    });

    let on_request = Callback::new(move |m: DappToWallet| handle_request(c, m));
    let embed = flags.embed;
    view! {
        <div class=if embed { "stage embedded" } else { "stage" }>
            <main id="main">
                {move || match c.screen.get() {
                    Screen::SignIn => view! { <SignIn /> }.into_any(),
                    Screen::EmailCode { email } => view! { <EmailCode email=email /> }.into_any(),
                    Screen::Unlock => view! { <Unlock /> }.into_any(),
                    Screen::SecureWallet => view! { <SecureWallet /> }.into_any(),
                    Screen::Recovery => view! { <Recovery /> }.into_any(),
                    Screen::RecoverCode => view! { <RecoverCode /> }.into_any(),
                    Screen::Home if c.home.get_untracked() => view! { <WalletHome /> }.into_any(),
                    Screen::Home => view! { <Home /> }.into_any(),
                    Screen::PickAccount(d) => view! { <PickAccount direction=d /> }.into_any(),
                    Screen::Sweep(p) => view! { <SweepScreen payment=p /> }.into_any(),
                    Screen::Send => view! { <Send /> }.into_any(),
                    Screen::Vault(d) => view! { <VaultMoveScreen direction=d /> }.into_any(),
                    Screen::Settings => view! { <Settings /> }.into_any(),
                    Screen::ConfirmMessage { id, message } => view! { <ConfirmMessage id=id message=message /> }.into_any(),
                    Screen::ConfirmTyped { id, data } => view! { <ConfirmTyped id=id data=data /> }.into_any(),
                    Screen::ConfirmTx { id, tx } => view! { <ConfirmTx id=id tx=tx /> }.into_any(),
                    Screen::ConfirmPrivate { id, to, amount } => view! { <ConfirmPrivate id=id to=to amount=amount /> }.into_any(),
                    Screen::Delegation { id, policy } => view! { <DelegationConsent id=id policy=policy /> }.into_any(),
                    Screen::Error(k) => view! { <ErrorScreen kind=k /> }.into_any(),
                }}
            </main>
            <div class="toast-region" role="status" aria-live="polite" data-test="toast">
                {move || c.toast.get().map(|t| view! { <div class="toast">{t}</div> })}
            </div>
            {flags.dev.then(|| view! { <DevPanel on_request=on_request /> })}
        </div>
    }
}
