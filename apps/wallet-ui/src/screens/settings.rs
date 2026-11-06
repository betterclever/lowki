//! Settings: sign-in methods, delegations, recovery status, your apps.

use leptos::prelude::*;
use leptos::task::spawn_local;

use crate::backend::{CodeStatus, Delegation, MethodKind, SignInMethod, backend};
use crate::format::{human_date, policy_can};
use crate::platform::backup_name;
use crate::protocol::WalletToDapp;
use crate::state::{Screen, ctx};
use crate::ui::components::*;
use crate::ui::icons::{Ic, Icon};

#[component]
pub fn Settings() -> impl IntoView {
    let c = ctx();
    let methods = RwSignal::new(None::<Vec<SignInMethod>>);
    let delegations = RwSignal::new(None::<Vec<Delegation>>);
    let recovery = LocalResource::new(move || async move { backend().recovery_status().await.ok() });
    let apps = LocalResource::new(move || async move { backend().app_accounts().await.ok() });
    spawn_local(async move {
        if let Ok(m) = backend().sign_in_methods().await {
            methods.set(Some(m));
        }
    });
    spawn_local(async move {
        match backend().delegations().await {
            Ok(d) => delegations.set(Some(d)),
            Err(e) => {
                c.fail(e);
            }
        }
    });
    let busy_method = RwSignal::new(None::<MethodKind>);
    let toggle_method = move |m: SignInMethod| {
        busy_method.set(Some(m.kind));
        spawn_local(async move {
            let r = if m.detail.is_some() { backend().unlink_method(m.kind).await } else { backend().link_method(m.kind).await };
            busy_method.set(None);
            match r {
                Ok(list) => {
                    methods.set(Some(list));
                    c.say(if m.detail.is_some() { "Sign-in method removed" } else { "Sign-in method added" });
                }
                Err(e) => {
                    c.fail(e);
                }
            }
        });
    };
    let signing_out = RwSignal::new(false);
    let sign_out = move || {
        signing_out.set(true);
        spawn_local(async move {
            let _ = backend().sign_out().await;
            c.session.set(None);
            c.post(WalletToDapp::SignedOut { id: None });
            c.reset(Screen::SignIn);
        });
    };
    let current_app = c.app.get_untracked().id;

    view! {
        <Sheet back=true class="settings">
            <h1 class="title" tabindex="-1">"Settings"</h1>

            <section class="section" aria-labelledby="m-h">
                <h2 id="m-h" class="label">"Sign-in methods"</h2>
                <ul class="list boxed">
                    {move || methods.get().map(|ms| ms.into_iter().map(|m| {
                        let (icon, name) = match m.kind {
                            MethodKind::Passkey => (view! { <Icon i=Ic::Passkey /> }.into_any(), "Passkey"),
                            MethodKind::Google => (view! { <Icon i=Ic::Google /> }.into_any(), "Google"),
                            MethodKind::Email => (view! { <Icon i=Ic::Mail /> }.into_any(), "Email"),
                        };
                        let linked = m.detail.is_some();
                        let sub = m.detail.clone().unwrap_or_else(|| "Not linked".into());
                        let is_passkey = m.kind == MethodKind::Passkey;
                        let kind = m.kind;
                        let m2 = m.clone();
                        view! {
                            <li class="row">
                                <span class="row-icon">{icon}</span>
                                <span class="row-main"><span class="row-title">{name}</span><span class="row-sub">{sub}</span></span>
                                {if is_passkey && linked {
                                    view! { <span class="pill">"Main key"</span> }.into_any()
                                } else {
                                    view! {
                                        <button class=if linked { "btn quiet sm" } else { "btn secondary sm" } type="button"
                                            aria-label=format!("{} {name}", if linked { "Unlink" } else { "Link" })
                                            disabled=move || busy_method.get().is_some()
                                            on:click=move |_| toggle_method(m2.clone())>
                                            {move || if busy_method.get() == Some(kind) { view! { <Spinner /> }.into_any() } else { view! { <span>{if linked { "Unlink" } else { "Link" }}</span> }.into_any() }}
                                        </button>
                                    }.into_any()
                                }}
                            </li>
                        }
                    }).collect_view())}
                </ul>
            </section>

            <section class="section" aria-labelledby="d-h">
                <h2 id="d-h" class="label">"Apps that can act for you"</h2>
                {move || match delegations.get() {
                    None => view! { <div class="loading-block"><Spinner /><span>"Loading…"</span></div> }.into_any(),
                    Some(list) if list.is_empty() => view! {
                        <div class="empty small"><p>"No app can act for you right now. Apps have to ask, and you approve each one."</p></div>
                    }.into_any(),
                    Some(list) => view! {
                        <ul class="deleg-list" data-test="delegations">
                            {list.into_iter().map(|d| view! { <DelegationRow d=d list=delegations /> }).collect_view()}
                        </ul>
                    }.into_any(),
                }}
            </section>

            <section class="section" aria-labelledby="r-h">
                <h2 id="r-h" class="label">"Recovery"</h2>
                <p class="section-sub">"Your wallet is split into three pieces. Any two bring it back."</p>
                {move || recovery.get().flatten().map(|r| {
                    let code_line = match r.code {
                        CodeStatus::BackedUp(t) => (true, format!("Backed up to {}", backup_name(t))),
                        CodeStatus::Saved => (true, "You said you saved it".to_string()),
                        CodeStatus::NotSaved => (false, "Not saved yet".to_string()),
                    };
                    view! {
                        <ul class="list boxed shares">
                            <ShareRow ok=r.passkey_share title="Passkey" sub=if r.passkey_share { "On your devices" } else { "Not set up" } />
                            <ShareRow ok=r.server_share title="Lowki server" sub="Released only when you sign in with Google" />
                            <ShareRow ok=code_line.0 title="Recovery code" sub=code_line.1 />
                        </ul>
                    }
                })}
            </section>

            <section class="section" aria-labelledby="a-h">
                <h2 id="a-h" class="label">"Your apps"</h2>
                <p class="section-sub"><Icon i=Ic::Eye size=15 />"Only you can see this list. Each app gets its own address and can't link it to the others."</p>
                <ul class="list boxed" data-test="your-apps">
                    {move || apps.get().flatten().map(|list| list.into_iter().map(|a| {
                        let here = a.app.id == current_app;
                        view! {
                            <li class="row">
                                <AppTile app=a.app.clone() small=true />
                                <span class="row-main">
                                    <span class="row-title">{a.app.name.clone()}{here.then(|| view! { <span class="pill sm">"This app"</span> })}</span>
                                    <span class="row-sub">{format!("{} · {}", a.app.origin.trim_start_matches("https://"), a.last_used)}</span>
                                </span>
                                <Address addr=a.address.clone() what=format!("{} address", a.app.name) />
                            </li>
                        }
                    }).collect_view())}
                </ul>
            </section>

            <div class="actions-col">
                <button class="btn secondary" type="button" disabled=move || signing_out.get() on:click=move |_| sign_out()>
                    <Icon i=Ic::Exit /><span>"Sign out of this app"</span>
                </button>
            </div>
        </Sheet>
    }
}

#[component]
fn ShareRow(ok: bool, #[prop(into)] title: String, #[prop(into)] sub: String) -> impl IntoView {
    view! {
        <li class="row">
            <span class=if ok { "row-icon ok" } else { "row-icon warn" }><Icon i=if ok { Ic::Check } else { Ic::Alert } size=16 /></span>
            <span class="row-main"><span class="row-title">{title}</span><span class="row-sub">{sub}</span></span>
        </li>
    }
}

#[component]
fn DelegationRow(d: Delegation, list: RwSignal<Option<Vec<Delegation>>>) -> impl IntoView {
    let c = ctx();
    let confirming = RwSignal::new(false);
    let busy = RwSignal::new(false);
    let id = d.id.clone();
    let name = d.app.name.clone();
    let revoke = move || {
        busy.set(true);
        let id = id.clone();
        let name = name.clone();
        spawn_local(async move {
            let r = backend().revoke_delegation(&id).await;
            busy.set(false);
            match r {
                Ok(()) => {
                    c.post(WalletToDapp::DelegationRevoked { delegation_id: id.clone() });
                    list.update(|l| if let Some(l) = l { l.retain(|x| x.id != id) });
                    c.say(format!("{name} can no longer act for you"));
                }
                Err(e) => {
                    c.fail(e);
                }
            }
        });
    };
    let first = policy_can(&d.policy).into_iter().next().unwrap_or_default();
    let app_name = d.app.name.clone();
    view! {
        <li class="deleg">
            <div class="deleg-head">
                <AppTile app=d.app.clone() small=true />
                <span class="row-main">
                    <span class="row-title">{d.app.name.clone()}</span>
                    <span class="row-sub">{format!("Since {} · expires {}", d.granted, human_date(&d.policy.expires))}</span>
                </span>
                <span class="verified sm"><Icon i=Ic::Shield size=14 /><span class="mono">{d.attestation.build.clone()}</span></span>
            </div>
            <p class="deleg-rule">{first}</p>
            {move || if confirming.get() {
                view! {
                    <div class="confirm-inline" role="group" aria-label=format!("Confirm revoking {app_name}")>
                        <p>{format!("Revoke {app_name}'s signer? It stops working right away.")}</p>
                        <div class="actions">
                            <button class="btn secondary sm" type="button" disabled=move || busy.get() on:click=move |_| confirming.set(false)>"Keep"</button>
                            <Btn label="Revoke".to_string() kind="danger sm" busy=busy test="revoke-confirm" on_click=revoke.clone() />
                        </div>
                    </div>
                }.into_any()
            } else {
                view! {
                    <button class="btn danger-outline sm" type="button" data-test="revoke" on:click=move |_| confirming.set(true)>"Revoke access"</button>
                }.into_any()
            }}
        </li>
    }
}
