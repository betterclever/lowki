//! Root component: screen routing, app requests, focus management.

use leptos::prelude::*;
use leptos::task::spawn_local;
use wasm_bindgen::JsCast;

use crate::backend::backend;
use crate::dev::{self, DevPanel, Flags};
use crate::platform::Platform;
use crate::protocol::{self, DappToWallet, RejectReason, WalletToDapp};
use crate::screens::confirm::{ConfirmMessage, ConfirmTx, ConfirmTyped};
use crate::screens::delegation::DelegationConsent;
use crate::screens::errors::ErrorScreen;
use crate::screens::home::Home;
use crate::screens::onboarding::{EmailCode, Recovery, SecureWallet, SignIn, Unlock};
use crate::screens::send::{Send, VaultMoveScreen};
use crate::screens::settings::Settings;
use crate::state::{Ctx, Screen};

fn is_request_screen(s: &Screen) -> bool {
    matches!(
        s,
        Screen::ConfirmMessage { .. } | Screen::ConfirmTyped { .. } | Screen::ConfirmTx { .. } | Screen::Delegation { .. }
    )
}

/// A request arrived from the app (postMessage or dev panel).
pub fn handle_request(c: Ctx, m: DappToWallet) {
    let id = m.id().to_string();
    if let DappToWallet::SignIn { .. } = m {
        match c.session.get_untracked() {
            Some(s) => {
                c.sign_in_request.set(Some(id));
                c.announce_session(&s);
            }
            None => {
                c.sign_in_request.set(Some(id));
                c.reset(Screen::SignIn);
            }
        }
        return;
    }
    if let DappToWallet::SignOut { .. } = m {
        spawn_local(async move {
            let _ = backend().sign_out().await;
            c.session.set(None);
            c.post(WalletToDapp::SignedOut { id: Some(id) });
            c.reset(Screen::SignIn);
        });
        return;
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
        DappToWallet::SignTypedData { id, typed_data } => Screen::ConfirmTyped { id, data: typed_data },
        DappToWallet::SendTransaction { id, tx } => Screen::ConfirmTx { id, tx },
        DappToWallet::RequestDelegation { id, policy } => Screen::Delegation { id, policy },
        DappToWallet::SignIn { .. } | DappToWallet::SignOut { .. } => return,
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
    provide_context(c);
    dev::apply_initial(c, &flags);

    protocol::listen(flags.origin.clone(), move |m| handle_request(c, m));
    c.post(WalletToDapp::Ready { app_id: flags.app.id.clone() });

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
                    Screen::Home => view! { <Home /> }.into_any(),
                    Screen::Send => view! { <Send /> }.into_any(),
                    Screen::Vault(d) => view! { <VaultMoveScreen direction=d /> }.into_any(),
                    Screen::Settings => view! { <Settings /> }.into_any(),
                    Screen::ConfirmMessage { id, message } => view! { <ConfirmMessage id=id message=message /> }.into_any(),
                    Screen::ConfirmTyped { id, data } => view! { <ConfirmTyped id=id data=data /> }.into_any(),
                    Screen::ConfirmTx { id, tx } => view! { <ConfirmTx id=id tx=tx /> }.into_any(),
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
