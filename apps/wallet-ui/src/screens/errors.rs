//! Full-screen error states. Each one says what happened, what didn't
//! happen, and offers a way forward.

use leptos::prelude::*;
use leptos::task::spawn_local;

use crate::backend::backend;
use crate::state::{ErrorKind, Screen, ctx};
use crate::ui::components::*;
use crate::ui::icons::{Ic, Icon};

#[component]
pub fn ErrorScreen(kind: ErrorKind) -> impl IntoView {
    let c = ctx();
    let p = c.platform.get_untracked();
    let app_name = c.app.get_untracked().name;
    match kind {
        ErrorKind::PasskeyCancelled => view! {
            <Sheet back=true>
                <div class="screen-head center" data-test="error-passkey">
                    <span class="glyph lg"><Icon i=Ic::Passkey size=28 /></span>
                    <h1 class="title" tabindex="-1">"Your passkey didn't respond"</h1>
                    <p class="sub">{format!("The {} prompt was closed, so nothing was saved. Try again; your wallet needs a passkey before it can hold funds.", p.unlock_name())}</p>
                </div>
                <div class="actions-col">
                    <Btn label="Try again".to_string() test="retry" on_click=move || c.back() />
                    <button class="btn quiet" type="button" on:click=move |_| c.reset(Screen::SignIn)>"Use a different sign-in"</button>
                </div>
            </Sheet>
        }.into_any(),
        ErrorKind::PrfUnsupported => {
            let busy = RwSignal::new(false);
            let fallback = move || {
                busy.set(true);
                spawn_local(async move {
                    let r = backend().continue_without_prf().await;
                    busy.set(false);
                    match r {
                        Ok(code) => {
                            c.recovery_code.set(Some(code));
                            c.go(Screen::Recovery);
                        }
                        Err(e) => {
                            c.fail(e);
                        }
                    }
                });
            };
            let href = web_sys::window().and_then(|w| w.location().href().ok()).unwrap_or_default();
            view! {
                <Sheet back=true>
                    <h1 class="title" tabindex="-1" data-test="error-prf">"This browser can't unlock Lowki"</h1>
                    <div class="alert warn">
                        <Icon i=Ic::Alert />
                        <div>
                            <strong>"Passkey PRF isn't supported here"</strong>
                            <span>"Lowki needs a passkey that can make a secret key. This browser or password manager doesn't offer that yet. Recent Chrome, Edge and Safari 18 or later do."</span>
                        </div>
                    </div>
                    <div class="actions-col">
                        <Btn label="Copy link for another browser".to_string() icon=Ic::Copy on_click=move || {
                            copy_text(&href);
                            c.say("Link copied. Open it in Chrome, Edge or Safari.");
                        } />
                        <Btn label="Sign in with Google and recovery code".to_string() kind="quiet" busy=busy test="prf-fallback" on_click=fallback />
                    </div>
                    <p class="fine">"With Google and a recovery code, you'll need the code each time you sign in on a new device."</p>
                </Sheet>
            }.into_any()
        }
        ErrorKind::Network => view! {
            <Sheet back=true>
                <div class="screen-head center" data-test="error-network">
                    <span class="glyph lg"><Icon i=Ic::CloudOff size=28 /></span>
                    <h1 class="title" tabindex="-1">"Can't reach Lowki"</h1>
                    <p class="sub">"Check your connection and try again. Nothing was sent or signed."</p>
                </div>
                <div class="actions-col">
                    <Btn label="Try again".to_string() icon=Ic::Refresh test="retry" on_click=move || c.back() />
                </div>
            </Sheet>
        }.into_any(),
        ErrorKind::PolicyRejected { reason, .. } => view! {
            <Sheet>
                <div class="screen-head" data-test="error-policy">
                    <span class="glyph danger"><Icon i=Ic::Not size=24 /></span>
                    <h1 class="title" tabindex="-1">"Request blocked"</h1>
                    <p class="sub">{format!("{app_name} asked for more access than Lowki allows, so nothing was approved and nothing changed.")}</p>
                </div>
                <div class="alert danger" role="note">
                    <Icon i=Ic::Info />
                    <div><strong>"Why it was blocked"</strong><span>{reason}</span></div>
                </div>
                <p class="fine left">{format!("{app_name} has been told. You don't need to do anything. If you expected this, contact {app_name}.")}</p>
                <div class="actions-col">
                    <Btn label="Close".to_string() test="close" on_click=move || c.done() />
                </div>
            </Sheet>
        }.into_any(),
    }
}
