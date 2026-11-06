//! The wallet home: what the user sees on the Lowki origin itself (top level, no app).
//!
//! - Private balance, with Move in / Move out through a chosen app account.
//! - Your name: claim `<label>.lowki.eth`, then payments found at its stealth addresses.
//! - Your apps: every app the user has used (from the encrypted app list), with its address
//!   and balance, plus demo apps to try.
//!
//! Nothing here is ever sent to an app: in home mode the wallet has no parent to post to.

use std::rc::Rc;

use leptos::prelude::*;
use leptos::task::spawn_local;

use crate::backend::{
    AppInfo, ClaimStage, HomeApp, NameStatus, ProofProgress, StealthPayment, VaultDirection, WalletError, backend,
    label_problem,
};
use crate::format::short_addr;
use crate::screens::send::{DoneView, ProofSteps};
use crate::state::{Screen, ctx};
use crate::ui::components::*;
use crate::ui::icons::{Ic, Icon};

/// The home's own "app": no ID, the Lowki origin. Restored after a Move in/out through an app.
pub fn home_app() -> AppInfo {
    let own = web_sys::window().and_then(|w| w.location().origin().ok()).unwrap_or_default();
    AppInfo { id: String::new(), name: "Lowki".into(), origin: own, hue: 90 }
}

fn ago(unix: i64) -> String {
    if unix <= 0 {
        return String::new();
    }
    let d = (js_sys::Date::now() / 1000.0) as i64 - unix;
    match d {
        i64::MIN..=59 => "just now".into(),
        60..=3599 => format!("{} min ago", d / 60),
        3600..=86_399 => format!("{} h ago", d / 3600),
        86_400..=172_799 => "yesterday".into(),
        _ => format!("{} days ago", d / 86_400),
    }
}

#[component]
pub fn WalletHome() -> impl IntoView {
    let c = ctx();
    if !c.app.get_untracked().id.is_empty() {
        c.app.set(home_app());
    }
    let apps = LocalResource::new(move || {
        c.refresh.track();
        async move { backend().home_apps().await }
    });
    let private = LocalResource::new(move || {
        c.refresh.track();
        async move { backend().private_balance().await }
    });
    let suggestions = LocalResource::new(|| async move { backend().app_suggestions().await });
    let connection = LocalResource::new(|| async move { backend().chain_connection().await });
    // The email the user signed in with, when the wallet knows it (not after a passkey-only login).
    let label = c
        .session
        .with_untracked(|s| s.as_ref().map(|s| s.account_label.clone()))
        .filter(|l| l.contains('@'))
        .unwrap_or_default();

    view! {
        <Sheet action=TopAction::Settings class="home wallet-home">
            <div class="screen-head home-head">
                <h1 class="title" tabindex="-1">"Your wallet"</h1>
                <p class="sub">
                    {(!label.is_empty()).then(|| view! { <span class="who">{label.clone()}</span> " · " })}
                    "Only you can see this page. Each app sees only its own account."
                </p>
            </div>
            {move || c.recovery_reminder.get().then(|| view! {
                <div class="alert" role="note">
                    <Icon i=Ic::Alert />
                    <div><strong>"Recovery code not saved"</strong><span>"Without it, losing your passkey means losing this wallet."</span></div>
                    <button class="btn quiet sm" type="button" on:click=move |_| c.go(Screen::Settings)>"Review"</button>
                </div>
            })}
            {move || connection.get().flatten().map(|cn| view! {
                <p class=if cn.private { "conn private home-conn" } else { "conn direct home-conn" } data-test="rpc-status"
                    data-private=cn.private.to_string()>
                    <span class="conn-dot" aria-hidden="true"></span>
                    <strong>{cn.title}</strong>
                    <span class="conn-detail">{cn.detail}</span>
                </p>
            })}
            <PrivateCard private=private apps=apps />
            <NameSection />
            <AppsSection apps=apps suggestions=suggestions />
        </Sheet>
    }
}

type AppsRes = LocalResource<Result<Vec<HomeApp>, WalletError>>;

#[component]
fn PrivateCard(private: LocalResource<Option<crate::backend::Amount>>, apps: AppsRes) -> impl IntoView {
    let c = ctx();
    let tok = backend().private_token();
    let any_funded = move || {
        apps.get().and_then(Result::ok).is_some_and(|l| l.iter().any(|a| a.balance.is_some_and(|b| b.units > 0)))
    };
    let has_private = move || private.get().flatten().is_some_and(|a| a.units > 0);
    view! {
        <section class="private-card" aria-labelledby="hp-h">
            <div class="private-row">
                <h2 id="hp-h" class="label">"Private balance"</h2>
                <PrivateBadge />
            </div>
            <div class="balance md" data-test="home-private-balance">
                {move || match private.get() {
                    None => view! { <span class="skel w-40 on-ink" aria-label="Loading private balance"></span> }.into_any(),
                    Some(Some(a)) => view! { <span>{a.fmt_plain()}</span><small>{a.token.symbol()}</small> }.into_any(),
                    Some(None) => view! { <span>"—"</span><small>{tok.symbol()}</small> }.into_any(),
                }}
            </div>
            <p class="private-note">{move || match private.get() {
                Some(None) => "The private pool isn't reachable from here right now.",
                _ => "Yours across every app. No app can see it.",
            }}</p>
            <div class="private-actions">
                <button class="btn on-private sm" type="button" data-test="home-move-in" disabled=move || !any_funded()
                    on:click=move |_| c.go(Screen::PickAccount(VaultDirection::In))>
                    <Icon i=Ic::MoveIn size=18 /><span>"Move in"</span>
                </button>
                <button class="btn on-private sm" type="button" data-test="home-move-out" disabled=move || !has_private()
                    on:click=move |_| c.go(Screen::PickAccount(VaultDirection::Out))>
                    <Icon i=Ic::MoveOut size=18 /><span>"Move out"</span>
                </button>
            </div>
        </section>
    }
}

// ---- your name ------------------------------------------------------------------------------

#[derive(Clone, PartialEq)]
enum Claim {
    Idle,
    Working(ClaimStage),
}

#[component]
fn NameSection() -> impl IntoView {
    let status = RwSignal::new(None::<Result<NameStatus, WalletError>>);
    let load = move || {
        spawn_local(async move {
            let r = backend().name_status().await;
            let _ = status.try_set(Some(r));
        });
    };
    load();
    view! {
        <section class="section name-section" aria-labelledby="nm-h">
            <h2 id="nm-h" class="label">"Your name"</h2>
            {move || match status.get() {
                None => view! { <div class="name-card"><span class="skel w-60"></span></div> }.into_any(),
                Some(Err(e)) => view! { <p class="muted sm">{e.message()}</p> }.into_any(),
                Some(Ok(st)) => match st.name.clone() {
                    Some(name) if st.ready => view! { <NameReady name=name /> }.into_any(),
                    Some(name) => {
                        let label = name.split('.').next().unwrap_or_default().to_string();
                        view! { <ClaimForm parent=st.parent.clone() status=status resume=Some(label) /> }.into_any()
                    }
                    None if !st.enabled => view! {
                        <div class="empty small"><p>"Names aren't offered by this Lowki server."</p></div>
                    }.into_any(),
                    None => view! { <ClaimForm parent=st.parent.clone() status=status resume=None /> }.into_any(),
                },
            }}
        </section>
    }
}

#[component]
fn ClaimForm(
    parent: String,
    status: RwSignal<Option<Result<NameStatus, WalletError>>>,
    resume: Option<String>,
) -> impl IntoView {
    let c = ctx();
    let parent = if parent.is_empty() { "lowki.eth".to_string() } else { parent };
    let resuming = resume.is_some();
    let label = RwSignal::new(resume.unwrap_or_default());
    let touched = RwSignal::new(false);
    let phase = RwSignal::new(Claim::Idle);
    let err = RwSignal::new(None::<String>);
    let problem = move || label.with(|l| if l.is_empty() { None } else { label_problem(l) });
    let p2 = parent.clone();
    let submit = move || {
        touched.set(true);
        let l = label.get_untracked();
        if let Some(p) = label_problem(&l) {
            err.set(Some(p.into()));
            return;
        }
        err.set(None);
        phase.set(Claim::Working(ClaimStage::Signing));
        let progress: Rc<dyn Fn(ClaimStage)> = Rc::new(move |s| {
            let _ = phase.try_set(Claim::Working(s));
        });
        spawn_local(async move {
            match backend().claim_name(&l, progress).await {
                Ok(st) => {
                    c.say(format!("{} is yours", st.name.clone().unwrap_or_default()));
                    let _ = status.try_set(Some(Ok(st)));
                }
                Err(WalletError::Network) => {
                    phase.set(Claim::Idle);
                    err.set(Some("Can't reach Lowki right now. Try again.".into()));
                }
                Err(e) => {
                    phase.set(Claim::Idle);
                    err.set(Some(e.message()));
                }
            }
        });
    };
    view! {
        {move || match phase.get() {
            Claim::Working(stage) => {
                let full = format!("{}.{}", label.get_untracked(), p2);
                view! { <ClaimSteps name=full stage=stage /> }.into_any()
            }
            Claim::Idle => {
                let parent = p2.clone();
                view! {
                    <form class="name-card claim" on:submit=move |ev| { ev.prevent_default(); submit(); }>
                        <div class="claim-head">
                            <span class="row-icon accent"><Icon i=Ic::At size=18 /></span>
                            <div>
                                <p class="claim-title">{if resuming { "Finish setting up your name" } else { "Claim a name people can pay" }}</p>
                                <p class="row-sub">"Every payment lands at a fresh address that only you can find. Lowki pays the network fee."</p>
                            </div>
                        </div>
                        <label class="sr-only" for="claim-label">"Name"</label>
                        <div class="name-input" data-invalid=move || (touched.get() && problem().is_some()).to_string()>
                            <input id="claim-label" class="input mono" type="text" inputmode="text" autocomplete="off"
                                autocapitalize="none" spellcheck="false" maxlength="32" placeholder="yourname"
                                data-test="claim-label" disabled=resuming
                                aria-describedby="claim-hint" aria-invalid=move || (touched.get() && problem().is_some()).to_string()
                                prop:value=move || label.get()
                                on:input=move |ev| {
                                    let v: String = event_target_value(&ev).trim().to_lowercase();
                                    label.set(v);
                                    err.set(None);
                                }
                                on:blur=move |_| touched.set(!label.get_untracked().is_empty()) />
                            <span class="name-suffix mono">{format!(".{parent}")}</span>
                        </div>
                        <p id="claim-hint" class=move || if touched.get() && problem().is_some() { "fine left error-text" } else { "fine left" }>
                            {move || match (touched.get(), problem()) {
                                (true, Some(p)) => p.to_string(),
                                _ => "3 to 32 lowercase letters, digits or hyphens.".to_string(),
                            }}
                        </p>
                        <p class="error-text" role="alert">{move || err.get()}</p>
                        <button class="btn primary" type="submit" data-test="claim"
                            disabled=move || label.with(|l| l.is_empty()) || problem().is_some()>
                            <Icon i=Ic::At />
                            <span>{
                                let parent = parent.clone();
                                move || {
                                    let l = label.get();
                                    if resuming {
                                        "Finish setup".to_string()
                                    } else if l.is_empty() {
                                        format!("Claim yourname.{parent}")
                                    } else {
                                        format!("Claim {l}.{parent}")
                                    }
                                }
                            }</span>
                        </button>
                    </form>
                }.into_any()
            }
        }}
    }
}

#[component]
fn ClaimSteps(name: String, stage: ClaimStage) -> impl IntoView {
    let order = [ClaimStage::Signing, ClaimStage::Registering, ClaimStage::SettingUp];
    let at = order.iter().position(|s| *s == stage).unwrap_or(0);
    let text = |s: ClaimStage| match s {
        ClaimStage::Signing => "Your wallet signs for the name",
        ClaimStage::Registering => "Registering on Sepolia",
        ClaimStage::SettingUp => "Setting up private payments",
    };
    view! {
        <div class="name-card claiming" data-test="claim-progress" aria-live="polite">
            <p class="claim-title">"Claiming " <span class="mono">{name}</span></p>
            <ol class="steps">
                {order.into_iter().enumerate().map(|(i, s)| {
                    let cls = if i < at { "step done" } else if i == at { "step active" } else { "step todo" };
                    view! {
                        <li class=cls>
                            <span class="step-mark">{if i < at {
                                view! { <Icon i=Ic::Check size=16 /> }.into_any()
                            } else if i == at {
                                view! { <Spinner /> }.into_any()
                            } else {
                                view! { <span class="step-dot"></span> }.into_any()
                            }}</span>
                            <span>{text(s)}</span>
                            <span class="t"></span>
                        </li>
                    }
                }).collect_view()}
            </ol>
            <p class="fine left">"Takes about half a minute. The key that owns the name never holds money: Lowki sponsors both transactions."</p>
        </div>
    }
}

#[component]
fn NameReady(name: String) -> impl IntoView {
    let c = ctx();
    let tick = RwSignal::new(0u32);
    let n2 = name.clone();
    let payments = LocalResource::new(move || {
        tick.track();
        c.refresh.track();
        let n = n2.clone();
        async move { backend().name_payments(&n).await }
    });
    let refreshing = move || payments.get().is_none();
    view! {
        <div class="name-card ready">
            <div class="name-line">
                <span class="row-icon accent"><Icon i=Ic::At size=18 /></span>
                <span class="name-main">
                    <span class="name-text mono" data-test="my-name">{name.clone()}</span>
                    <span class="row-sub">"Each lookup gives a new address. Payers can't link them."</span>
                </span>
                <CopyButton text=name.clone() what="name" />
            </div>
        </div>
        <div class="label-row pay-head">
            <h3 class="label">{format!("Payments to {name}")}</h3>
            <button class="link sm" type="button" data-test="payments-refresh" disabled=refreshing
                on:click=move |_| tick.update(|t| *t += 1)>
                {move || if refreshing() { "Checking…" } else { "Check again" }}
            </button>
        </div>
        {move || match payments.get() {
            None => view! {
                <ul class="list boxed">{(0..2).map(|_| view! { <li class="row"><span class="skel w-60"></span></li> }).collect_view()}</ul>
            }.into_any(),
            Some(Err(e)) => view! { <p class="muted sm">{e.message()}</p> }.into_any(),
            Some(Ok(list)) if list.is_empty() => view! {
                <div class="empty small" data-test="payments-empty">
                    <Icon i=Ic::Clock size=22 />
                    <p><strong>"No payments yet"</strong></p>
                    <p>"Share your name. Whoever pays it gets a fresh address, and the payment shows up here."</p>
                </div>
            }.into_any(),
            Some(Ok(list)) => view! {
                <ul class="list boxed" data-test="payments">
                    {list.into_iter().map(|p| view! { <PaymentRow p=p /> }).collect_view()}
                </ul>
            }.into_any(),
        }}
    }
}

#[component]
fn PaymentRow(p: StealthPayment) -> impl IntoView {
    let c = ctx();
    let sub = {
        let when = ago(p.created_at);
        if when.is_empty() { short_addr(&p.address) } else { format!("{} · {when}", short_addr(&p.address)) }
    };
    let can = p.can_sweep;
    let p2 = p.clone();
    view! {
        <li class="row" data-test="payment" data-address=p.address.to_lowercase()>
            <span class="row-icon"><Icon i=Ic::ArrowDownLeft size=18 /></span>
            <span class="row-main">
                <span class="row-title mono">{format!("+{}", p.balance.fmt())}</span>
                <span class="row-sub" title=p.address.clone()>{sub}</span>
            </span>
            {if can {
                view! {
                    <button class="btn secondary sm" type="button" data-test="sweep"
                        on:click=move |_| c.go(Screen::Sweep(p2.clone()))>
                        <Icon i=Ic::Keyhole size=16 /><span>"Move to private"</span>
                    </button>
                }.into_any()
            } else {
                view! { <span class="pill" title="Below the private pool's 0.001 ETH minimum, so it stays here">"Found"</span> }.into_any()
            }}
        </li>
    }
}

// ---- your apps ------------------------------------------------------------------------------

#[component]
fn AppsSection(apps: AppsRes, suggestions: LocalResource<Vec<crate::backend::Suggestion>>) -> impl IntoView {
    view! {
        <section class="section" aria-labelledby="ap-h">
            <h2 id="ap-h" class="label">"Your apps"</h2>
            <p class="section-sub"><Icon i=Ic::Eye size=15 />"Only you can see this list. It's stored encrypted, so Lowki's server can't read it either."</p>
            {move || match apps.get() {
                None => view! {
                    <ul class="list boxed">{(0..2).map(|_| view! { <li class="row"><span class="skel w-60"></span></li> }).collect_view()}</ul>
                }.into_any(),
                Some(Err(e)) => view! { <p class="muted sm">{e.message()}</p> }.into_any(),
                Some(Ok(list)) if list.is_empty() => view! {
                    <div class="empty small" data-test="apps-empty">
                        <Icon i=Ic::Grid size=22 />
                        <p><strong>"No apps yet"</strong></p>
                        <p>"Apps you sign in to with Lowki show up here, each with its own address."</p>
                    </div>
                }.into_any(),
                Some(Ok(list)) => view! {
                    <ul class="list boxed" data-test="home-apps">
                        {list.into_iter().map(|a| view! { <AppRow a=a /> }).collect_view()}
                    </ul>
                }.into_any(),
            }}
            {move || suggestions.get().filter(|s| !s.is_empty()).map(|s| view! {
                <h3 class="label try-h">"Try an app"</h3>
                <ul class="list boxed" data-test="suggestions">
                    {s.into_iter().map(|s| {
                        let tile = AppInfo { id: String::new(), name: s.name.clone(), origin: s.url.clone(), hue: 90 };
                        let host = s.url.split('/').nth(2).unwrap_or_default().to_string();
                        view! {
                            <li class="row">
                                <AppTile app=tile small=false />
                                <span class="row-main">
                                    <span class="row-title">{s.name.clone()}</span>
                                    <span class="row-sub">{s.blurb.clone().unwrap_or(host)}</span>
                                </span>
                                <a class="btn secondary sm" href=s.url.clone() target="_blank" rel="noopener noreferrer">
                                    <span>"Open"</span><Icon i=Ic::External size=16 />
                                </a>
                            </li>
                        }
                    }).collect_view()}
                </ul>
            })}
        </section>
    }
}

#[component]
fn AppRow(a: HomeApp) -> impl IntoView {
    let host = a.app.origin.trim_start_matches("https://").trim_start_matches("http://").to_string();
    view! {
        <li class="row app-row" data-test="home-app" data-origin=a.app.origin.clone() data-address=a.address.to_lowercase()>
            <AppTile app=a.app.clone() />
            <span class="row-main">
                <span class="row-title">{a.app.name.clone()}</span>
                <span class="row-sub mono">{host}</span>
            </span>
            <span class="app-side">
                <span class="row-amt mono">{a.balance.map(|b| b.fmt()).unwrap_or_else(|| "—".into())}</span>
                <Address addr=a.address.clone() what=format!("{} address", a.app.name) />
            </span>
        </li>
    }
}

// ---- move in / out through an app account ---------------------------------------------------

#[component]
pub fn PickAccount(direction: VaultDirection) -> impl IntoView {
    let c = ctx();
    let apps = LocalResource::new(move || async move { backend().home_apps().await });
    let inbound = direction == VaultDirection::In;
    let pick = move |a: HomeApp| {
        c.app.set(a.app.clone());
        c.go(Screen::Vault(direction));
    };
    view! {
        <Sheet back=true>
            <div class="screen-head">
                <span class="glyph private"><Icon i=if inbound { Ic::MoveIn } else { Ic::MoveOut } size=24 /></span>
                <h1 class="title" tabindex="-1">{if inbound { "Move in from which app?" } else { "Move out to which app?" }}</h1>
                <p class="sub">{if inbound {
                    "The deposit comes from that app's account, so it's visible on chain. What you do with it afterwards isn't."
                } else {
                    "A proof made on this device hides which deposit the money came from."
                }}</p>
            </div>
            {move || match apps.get() {
                None => view! { <div class="loading-block"><Spinner /><span>"Loading your apps…"</span></div> }.into_any(),
                Some(Err(e)) => view! { <p class="muted sm">{e.message()}</p> }.into_any(),
                Some(Ok(list)) => {
                    let list: Vec<HomeApp> = list.into_iter().filter(|a| !inbound || a.balance.is_some_and(|b| b.units > 0)).collect();
                    if list.is_empty() {
                        view! { <div class="empty small"><p>"None of your app accounts holds ETH yet."</p></div> }.into_any()
                    } else {
                        view! {
                            <ul class="list boxed pick">
                                {list.into_iter().map(|a| {
                                    let a2 = a.clone();
                                    view! {
                                        <li>
                                            <button class="row pick-row" type="button" data-test="pick-account" on:click=move |_| pick(a2.clone())>
                                                <AppTile app=a.app.clone() small=true />
                                                <span class="row-main">
                                                    <span class="row-title">{a.app.name.clone()}</span>
                                                    <span class="row-sub mono">{short_addr(&a.address)}</span>
                                                </span>
                                                <span class="row-amt mono">{a.balance.map(|b| b.fmt()).unwrap_or_default()}</span>
                                            </button>
                                        </li>
                                    }
                                }).collect_view()}
                            </ul>
                        }.into_any()
                    }
                }
            }}
        </Sheet>
    }
}

#[derive(Clone, PartialEq)]
enum SweepPhase {
    Confirm,
    Working,
    Done(crate::backend::SendReceipt),
}

#[component]
pub fn SweepScreen(payment: StealthPayment) -> impl IntoView {
    let c = ctx();
    let phase = RwSignal::new(SweepPhase::Confirm);
    let progress = RwSignal::new(None::<ProofProgress>);
    let err = RwSignal::new(None::<String>);
    let p = payment.clone();
    let go = move || {
        err.set(None);
        phase.set(SweepPhase::Working);
        let cb: Rc<dyn Fn(ProofProgress)> = Rc::new(move |x| {
            let _ = progress.try_set(Some(x));
        });
        let p = p.clone();
        spawn_local(async move {
            match backend().sweep_payment(&p, cb).await {
                Ok(r) => phase.set(SweepPhase::Done(r)),
                Err(e) => {
                    phase.set(SweepPhase::Confirm);
                    err.set(c.fail(e));
                }
            }
        });
    };
    let amount = payment.balance;
    let addr = payment.address.clone();
    view! {
        <Sheet back=true>
            {move || match phase.get() {
                SweepPhase::Confirm => {
                    let go = go.clone();
                    let addr = addr.clone();
                    view! {
                        <div class="screen-head">
                            <span class="glyph private"><Icon i=Ic::Keyhole size=24 /></span>
                            <h1 class="title" tabindex="-1">"Move this payment to private?"</h1>
                            <p class="sub">"It goes into your private balance straight from the address it was paid to. The network fee comes out of the payment."</p>
                        </div>
                        <dl class="kvs">
                            <Kv k="Payment"><span class="mono">{amount.fmt()}</span></Kv>
                            <Kv k="At"><Address addr=addr.clone() what="payment address" /></Kv>
                            <Kv k="Network fee"><span>"Paid from the payment"</span></Kv>
                        </dl>
                        <p class="error-text" role="alert">{move || err.get()}</p>
                        <Btn label="Move to private".to_string() icon=Ic::Keyhole test="sweep-confirm" on_click=move || go() />
                    }.into_any()
                }
                SweepPhase::Working => view! {
                    <div class="screen-head"><h1 class="title" tabindex="-1">"Moving to private"</h1></div>
                    <ProofSteps progress=progress labels=["Depositing", "ASP approval", "Syncing pool"]
                        fine="The deposit comes from the payment's own address, not from any of your apps." />
                }.into_any(),
                SweepPhase::Done(r) => view! {
                    <DoneView title="Now private".to_string() amount=amount
                        detail="It's in your private balance. Only you can see it.".to_string() receipt=r />
                }.into_any(),
            }}
        </Sheet>
    }
}
