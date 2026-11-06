//! Delegation consent: the app's server asks for a policy-limited signer.

use leptos::prelude::*;
use leptos::task::spawn_local;

use crate::backend::{DelegationPolicy, DelegationReview, WalletError, backend};
use crate::format::{human_date, policy_can, policy_cant};
use crate::protocol::{RejectReason, WalletToDapp};
use crate::state::{ErrorKind, Screen, ctx};
use crate::ui::components::*;
use crate::ui::icons::{Ic, Icon};

#[component]
pub fn DelegationConsent(id: String, policy: DelegationPolicy) -> impl IntoView {
    let c = ctx();
    let app = c.app.get_untracked();
    let pol = policy.clone();
    let review = LocalResource::new(move || {
        let p = pol.clone();
        let app = c.app.get_untracked();
        async move { backend().review_delegation(&app, &p).await }
    });
    let id_e = id.clone();
    Effect::new(move |_| match review.get() {
        Some(Err(WalletError::PolicyRejected { reason })) => {
            c.reject(&id_e, RejectReason::PolicyRejected, &reason);
            c.replace(Screen::Error(ErrorKind::PolicyRejected { id: id_e.clone(), reason }));
        }
        Some(Err(e)) => {
            c.reject(&id_e, RejectReason::NetworkError, &e.message());
            c.fail(e);
        }
        _ => {}
    });
    let busy = RwSignal::new(false);
    let id_a = id.clone();
    let approve = move || {
        let Some(Ok(rv)) = review.get_untracked() else { return };
        busy.set(true);
        let id = id_a.clone();
        spawn_local(async move {
            let r = backend().approve_delegation(&c.app.get_untracked(), &rv).await;
            busy.set(false);
            match r {
                Ok(d) => {
                    c.post(WalletToDapp::DelegationApproved {
                        id,
                        delegation_id: d.id.clone(),
                        expires: d.policy.expires.clone(),
                        attestation_build: d.attestation.build.clone(),
                    });
                    c.say(format!("{} can now act within these limits", d.app.name));
                    c.done();
                }
                Err(e) => {
                    c.reject(&id, RejectReason::NetworkError, &e.message());
                    c.fail(e);
                }
            }
        });
    };
    let id_r = id.clone();
    let reject = move || {
        c.reject(&id_r, RejectReason::UserRejected, "User declined");
        c.done();
    };
    let reject_x = reject.clone();
    let can = policy_can(&policy);
    let cant = policy_cant(&policy);
    let expires = human_date(&policy.expires);
    let name = app.name.clone();
    let ready = Signal::derive(move || matches!(review.get(), Some(Ok(_))));

    view! {
        <Sheet class="request consent" action=TopAction::Close on_close=Callback::new(move |_| reject_x())>
            <div class="requester">
                <AppTile app=app.clone() />
                <div>
                    <p class="label">{format!("Request from {} server", app.name)}</p>
                    <p class="row-sub">{app.origin.trim_start_matches("https://").to_string()}</p>
                </div>
            </div>
            <h1 class="title" tabindex="-1">{format!("Let {} act for you?", app.name)}</h1>
            <p class="muted sm">{format!("Its server can sign a few things from your {name} account while you're away. Only within these limits.")}</p>

            <section aria-labelledby="can-h">
                <h2 id="can-h" class="label">"Can"</h2>
                <ul class="plist can" data-test="policy-can">
                    {can.into_iter().map(|s| view! { <li><Icon i=Ic::Check /><span>{s}</span></li> }).collect_view()}
                </ul>
            </section>
            <section aria-labelledby="cant-h">
                <h2 id="cant-h" class="label">"Can't"</h2>
                <ul class="plist cant" data-test="policy-cant">
                    {cant.into_iter().map(|s| view! { <li><Icon i=Ic::Minus /><span>{s}</span></li> }).collect_view()}
                </ul>
            </section>

            <div class="meta" data-test="attestation">
                <div><span class="muted">"Ends"</span><span>{expires}</span></div>
                <div><span class="muted">"On chain"</span><span>"Nothing is posted"</span></div>
                <div>
                    <span class="muted">"Signer"</span>
                    {move || match review.get() {
                        Some(Ok(DelegationReview { attestation: a, .. })) => view! {
                            <span class=if a.verified { "verified" } else { "unverified" } title=a.measurement.clone()>
                                <Icon i=if a.verified { Ic::Shield } else { Ic::Alert } size=16 />
                                {if a.verified { "Verified signer, build " } else { "Not verified, build " }}
                                <span class="mono">{a.build.clone()}</span>
                            </span>
                        }.into_any(),
                        _ => view! { <span class="muted inline-wait"><Spinner />"Checking…"</span> }.into_any(),
                    }}
                </div>
                {move || match review.get() {
                    Some(Ok(DelegationReview { attestation: a, .. })) => Some(view! {
                        <details class="details plain">
                            <summary>"Enclave measurement"</summary>
                            <pre>{a.measurement.clone()}</pre>
                        </details>
                    }),
                    _ => None,
                }}
            </div>
            <div class="actions-col">
                <Btn label="Approve".to_string() busy=busy busy_label="Sealing the key…" disabled=Signal::derive(move || !ready.get()) test="approve" on_click=approve />
                <button class="btn quiet" type="button" data-test="reject" disabled=move || busy.get() on:click=move |_| reject()>"Not now"</button>
            </div>
            <p class="fine">"You can revoke access anytime in Settings."</p>
        </Sheet>
    }
}
