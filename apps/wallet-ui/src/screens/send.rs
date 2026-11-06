//! Send (public or private) and moving funds in/out of the private balance.

use std::rc::Rc;

use leptos::prelude::*;
use leptos::task::spawn_local;

use crate::backend::{
    Amount, ProofProgress, ProofStage, SendReceipt, SendRequest, Token, VaultDirection, VaultMove, WalletError, backend,
};
use crate::format::short_addr;
use crate::state::ctx;
use crate::ui::components::*;
use crate::ui::icons::{GOURD, GOURD_VIEWBOX, GourdGlyph, Ic, Icon, KEYHOLE, KEYHOLE_TRANSFORM};

#[derive(Clone, PartialEq)]
enum Phase {
    Form,
    Working,
    Done(SendReceipt, Amount, String),
}

fn stage_text(s: ProofStage) -> &'static str {
    match s {
        ProofStage::SyncingPool => "Syncing pool",
        ProofStage::BuildingProof => "Building proof",
        ProofStage::Submitting => "Submitting",
    }
}

fn reduced_motion() -> bool {
    web_sys::window()
        .and_then(|w| w.match_media("(prefers-reduced-motion: reduce)").ok().flatten())
        .is_some_and(|m| m.matches())
}

fn secs(ms: f64) -> String {
    format!("{:.1} s", (ms / 1000.0).max(0.0))
}

/// The signature moment: the gourd fills with lime from the bottom as the
/// proof moves through its stages. Reduced motion shows stepped fills.
/// `labels` renames the three stage slots (Move in: deposit, ASP approval, sync).
#[component]
pub fn ProofSteps(
    progress: RwSignal<Option<ProofProgress>>,
    #[prop(optional)] labels: Option<[&'static str; 3]>,
    #[prop(optional)] fine: Option<&'static str>,
) -> impl IntoView {
    let idx = |s: ProofStage| ProofStage::all().iter().position(|x| *x == s).unwrap_or(0);
    let label = move |s: ProofStage| labels.map_or_else(|| stage_text(s), |l| l[idx(s)]);
    let reduced = reduced_motion();
    // Per-stage start/end timestamps, for the times column.
    let starts = RwSignal::new([None::<f64>; 3]);
    let ends = RwSignal::new([None::<f64>; 3]);
    let now = RwSignal::new(0.0f64);
    // Tick the running stage's clock while it works (real stages can take seconds).
    if let Ok(ticker) = set_interval_with_handle(
        move || {
            if progress.try_get_untracked().flatten().is_some() {
                let _ = now.try_set(js_sys::Date::now());
            }
        },
        std::time::Duration::from_millis(100),
    ) {
        on_cleanup(move || ticker.clear());
    }
    Effect::new(move |_| {
        let Some(p) = progress.get() else { return };
        let t = js_sys::Date::now();
        let i = idx(p.stage);
        now.set(t);
        starts.update(|s| if s[i].is_none() { s[i] = Some(t) });
        ends.update(|e| {
            for slot in e.iter_mut().take(i) {
                if slot.is_none() {
                    *slot = Some(t);
                }
            }
            if p.fraction >= 1.0 && e[i].is_none() {
                e[i] = Some(t);
            }
        });
    });
    let overall = move || {
        progress.get().map_or(0.02, |p| {
            let bounds = [0.0f32, 0.30, 0.78, 1.0];
            let i = idx(p.stage);
            if reduced {
                if p.fraction >= 1.0 { bounds[i + 1] } else { bounds[i].max(0.02) }
            } else {
                (bounds[i] + (bounds[i + 1] - bounds[i]) * p.fraction).max(0.02)
            }
        })
    };
    view! {
        <div class="proof" data-test="proof-steps">
            <svg class="gourd-fill" viewBox=GOURD_VIEWBOX role="progressbar" aria-label="Private send progress"
                aria-valuemin="0" aria-valuemax="100" aria-valuenow=move || ((overall() * 100.0).round() as u32).to_string()>
                <defs><clipPath id="gourd-clip"><path d=GOURD /></clipPath></defs>
                <path class="gourd-shell" d=GOURD />
                <g clip-path="url(#gourd-clip)">
                    <rect class="fill" x="28.3" y="1.7" width="42.9" height="95.4" style=move || format!("transform:scaleY({:.3})", overall()) />
                </g>
                <path class="gourd-key" d=KEYHOLE transform=KEYHOLE_TRANSFORM />
            </svg>
            <ol class="steps">
                {ProofStage::all().into_iter().map(|s| {
                    let i = idx(s);
                    let state = move || match progress.get() {
                        None => "todo",
                        Some(p) if idx(p.stage) > i => "done",
                        Some(p) if p.stage == s && p.fraction >= 1.0 => "done",
                        Some(p) if p.stage == s => "active",
                        _ => "todo",
                    };
                    let time = move || {
                        let (st, en) = (starts.get()[i], ends.get()[i]);
                        match (st, en) {
                            (Some(a), Some(b)) => secs(b - a),
                            (Some(a), None) => secs(now.get() - a),
                            _ => "—".to_string(),
                        }
                    };
                    view! {
                        <li class=move || format!("step {}", state()) aria-current=move || (state() == "active").then_some("step")>
                            <span class="step-mark">{move || match state() {
                                "done" => view! { <Icon i=Ic::Check size=16 /> }.into_any(),
                                "active" => view! { <Spinner /> }.into_any(),
                                _ => view! { <span class="step-dot"></span> }.into_any(),
                            }}</span>
                            <span class="step-title">{label(s)}</span>
                            <span class="t">{time}</span>
                        </li>
                    }
                }).collect_view()}
            </ol>
            <p class="sr-only" aria-live="polite">{move || progress.get().map(|p| label(p.stage))}</p>
        </div>
        <p class="fine">{fine.unwrap_or("Your proof is made on this device. Nobody learns which deposit was yours.")}</p>
    }
}

#[component]
fn DoneView(title: String, amount: Amount, detail: String, receipt: SendReceipt) -> impl IntoView {
    let c = ctx();
    // What really happened (stage times, proof backend and time, fee), when the backend knows.
    let report = if receipt.private { backend().private_report() } else { vec![] };
    view! {
        <div class="screen-head center done" data-test="send-done">
            {if receipt.private {
                view! { <span class="glyph lg private"><GourdGlyph size=32 /></span> }.into_any()
            } else {
                view! { <span class="glyph lg accent"><Icon i=Ic::Check size=28 /></span> }.into_any()
            }}
            <h1 class="title" tabindex="-1">{title}</h1>
            <p class="amount-line">{amount.fmt()}</p>
            <p class="sub">{detail}</p>
        </div>
        <dl class="kvs">
            <Kv k="Transaction"><Address addr=receipt.hash.clone() what="transaction hash" /></Kv>
        </dl>
        {(!report.is_empty()).then(|| view! {
            <dl class="kvs" data-test="private-report">
                {report.into_iter().map(|(k, v)| view! {
                    <div class="kv"><dt>{k}</dt><dd class="mono">{v}</dd></div>
                }).collect_view()}
            </dl>
        })}
        <div class="actions-col">
            <Btn label="Done".to_string() test="done" on_click=move || c.done() />
        </div>
    }
}

#[component]
pub fn Send() -> impl IntoView {
    let c = ctx();
    let to = RwSignal::new(String::new());
    let amount = RwSignal::new(String::new());
    let private_token = backend().private_token();
    let token = RwSignal::new(private_token);
    let private = RwSignal::new(true);
    let err = RwSignal::new(None::<String>);
    let phase = RwSignal::new(Phase::Form);
    let progress = RwSignal::new(None::<ProofProgress>);
    let balances = LocalResource::new(move || {
        let app = c.app.get_untracked();
        async move { backend().balances(&app).await.ok() }
    });
    let available = move || {
        balances.get().flatten().map(|b| if private.get() { b.private_of(token.get()) } else { b.public_of(token.get()) })
    };

    let submit = move || {
        let addr = to.get_untracked().trim().to_string();
        let Some(amt) = Amount::parse(token.get_untracked(), &amount.get_untracked()).filter(|a| a.units > 0) else {
            err.set(Some("Enter an amount, like 25 or 12.50.".into()));
            return;
        };
        err.set(None);
        let is_private = private.get_untracked();
        phase.set(Phase::Working);
        progress.set(None);
        let cb: Rc<dyn Fn(ProofProgress)> = Rc::new(move |p| progress.set(Some(p)));
        spawn_local(async move {
            let req = SendRequest { to: addr.clone(), amount: amt, private: is_private };
            match backend().send(&c.app.get_untracked(), req, cb).await {
                Ok(r) => phase.set(Phase::Done(r, amt, addr)),
                Err(e @ (WalletError::InvalidAddress | WalletError::InsufficientFunds)) => {
                    phase.set(Phase::Form);
                    err.set(Some(e.message()));
                }
                Err(e) => {
                    phase.set(Phase::Form);
                    c.fail(e);
                }
            }
        });
    };

    view! {
        <Sheet back=true>
            {move || match phase.get() {
                Phase::Form => view! {
                    <div class="screen-head">
                        <h1 class="title" tabindex="-1">"Send"</h1>
                    </div>
                    <form class="stack" on:submit=move |ev| { ev.prevent_default(); submit(); }>
                        <div class="field">
                            <label class="label" for="to">"To"</label>
                            <input id="to" class="input mono" type="text" placeholder="0x…" autocomplete="off" spellcheck="false"
                                data-test="send-to" prop:value=move || to.get() on:input=move |ev| to.set(event_target_value(&ev)) />
                        </div>
                        <div class="field">
                            <div class="label-row">
                                <label class="label" for="amount">"Amount"</label>
                                <span class="label-aside">{move || available().map(|a| format!("{} available", a.fmt()))}</span>
                            </div>
                            <div class="amount-row">
                                <input id="amount" class="input amount" type="text" inputmode="decimal" placeholder="0.00" autocomplete="off"
                                    data-test="send-amount" prop:value=move || amount.get() on:input=move |ev| amount.set(event_target_value(&ev)) />
                                <div class="seg" role="radiogroup" aria-label="Token">
                                    {move || {
                                        let tokens = balances.get().flatten().map(|b| b.tokens()).unwrap_or_else(|| Token::all().to_vec());
                                        tokens.into_iter().map(|t| view! {
                                            <button type="button" role="radio" class="seg-btn" aria-checked=move || (token.get() == t).to_string()
                                                on:click=move |_| token.set(t)>{t.symbol()}</button>
                                        }).collect_view()
                                    }}
                                </div>
                            </div>
                            <button class="link sm" type="button" on:click=move |_| if let Some(a) = available() { amount.set(a.fmt_plain().replace(',', "")) }>"Use max"</button>
                        </div>
                        <div class=move || if private.get() { "toggle-card on" } else { "toggle-card" }>
                            <div class="toggle-text">
                                <span class="toggle-title" id="priv-l"><Icon i=Ic::Keyhole size=18 />"Send privately"</span>
                                <span class="toggle-sub" id="priv-d">{move || if private.get() {
                                    "Pays from your private balance with a proof made on this device. Nobody can link it to this account."
                                } else {
                                    "Pays from this account. The recipient and anyone on chain will see this address."
                                }}</span>
                            </div>
                            <button type="button" role="switch" class="switch" data-test="private-toggle"
                                aria-checked=move || private.get().to_string() aria-labelledby="priv-l" aria-describedby="priv-d"
                                on:click=move |_| {
                                    private.update(|p| *p = !*p);
                                    if private.get_untracked() { token.set(private_token); }
                                }>
                                <span class="switch-knob"></span>
                            </button>
                        </div>
                        <p class="error-text" role="alert">{move || err.get()}</p>
                        <button class="btn primary wide" type="submit" data-test="send-submit">
                            {move || if private.get() { view! { <Icon i=Ic::Keyhole /><span>"Send privately"</span> }.into_any() }
                                else { view! { <Icon i=Ic::ArrowUpRight /><span>"Send"</span> }.into_any() }}
                        </button>
                    </form>
                }.into_any(),
                Phase::Working => {
                    if private.get_untracked() {
                        view! {
                            <div class="screen-head">
                                <h1 class="title" tabindex="-1">"Sending privately"</h1>
                                                            </div>
                            <ProofSteps progress=progress />
                        }.into_any()
                    } else {
                        view! {
                            <div class="screen-head center">
                                <span class="glyph lg"><Spinner large=true /></span>
                                <h1 class="title" tabindex="-1">"Sending…"</h1>
                                <p class="sub">{move || format!("Submitting to {}.", balances.get().flatten().map(|b| b.network).unwrap_or_else(|| "the network".into()))}</p>
                            </div>
                        }.into_any()
                    }
                }
                Phase::Done(r, amt, addr) => {
                    let detail = if r.private {
                        format!("To {}. They can't see where it came from.", short_addr(&addr))
                    } else {
                        format!("To {}.", short_addr(&addr))
                    };
                    let title = if r.private { "Sent privately." } else { "Sent." };
                    view! { <DoneView title=title.to_string() amount=amt detail=detail receipt=r /> }.into_any()
                }
            }}
        </Sheet>
    }
}

#[component]
pub fn VaultMoveScreen(direction: VaultDirection) -> impl IntoView {
    let c = ctx();
    let amount = RwSignal::new(String::new());
    let err = RwSignal::new(None::<String>);
    let phase = RwSignal::new(Phase::Form);
    let progress = RwSignal::new(None::<ProofProgress>);
    let balances = LocalResource::new(move || {
        let app = c.app.get_untracked();
        async move { backend().balances(&app).await.ok() }
    });
    let inbound = direction == VaultDirection::In;
    let tok = backend().private_token();
    let available = move || balances.get().flatten().map(|b| if inbound { b.public_of(tok) } else { b.private_of(tok) });
    let app_name = c.app.get_untracked().name;
    let submit = move || {
        let Some(amt) = Amount::parse(tok, &amount.get_untracked()).filter(|a| a.units > 0) else {
            err.set(Some("Enter an amount, like 25 or 12.50.".into()));
            return;
        };
        if available().is_some_and(|a| a.units < amt.units) {
            err.set(Some(WalletError::InsufficientFunds.message()));
            return;
        }
        err.set(None);
        phase.set(Phase::Working);
        let cb: Rc<dyn Fn(ProofProgress)> = Rc::new(move |p| progress.set(Some(p)));
        spawn_local(async move {
            match backend().move_private(&c.app.get_untracked(), VaultMove { direction, amount: amt }, cb).await {
                Ok(r) => phase.set(Phase::Done(r, amt, String::new())),
                Err(WalletError::InsufficientFunds) => {
                    phase.set(Phase::Form);
                    err.set(Some(WalletError::InsufficientFunds.message()));
                }
                Err(e) => {
                    phase.set(Phase::Form);
                    c.fail(e);
                }
            }
        });
    };
    let title = if inbound { "Move to private balance" } else { "Move out of private balance" };
    let explain = if inbound {
        format!("Moves {} from your {app_name} account into your private balance. The deposit is visible on chain; what you do with it afterwards isn't.", tok.symbol())
    } else {
        format!("Moves {} back to your {app_name} account. A proof on this device hides which deposit it came from.", tok.symbol())
    };
    view! {
        <Sheet back=true>
            {move || match phase.get() {
                Phase::Form => view! {
                    <div class="screen-head">
                        <span class="glyph private"><Icon i=Ic::Keyhole size=24 /></span>
                        <h1 class="title" tabindex="-1">{title}</h1>
                        <p class="sub">{explain.clone()}</p>
                    </div>
                    <form class="stack" on:submit=move |ev| { ev.prevent_default(); submit(); }>
                        <div class="field">
                            <div class="label-row">
                                <label class="label" for="vamount">"Amount"</label>
                                <span class="label-aside">{move || available().map(|a| format!("{} available", a.fmt()))}</span>
                            </div>
                            <div class="amount-row">
                                <input id="vamount" class="input amount" type="text" inputmode="decimal" placeholder="0.00"
                                    data-test="vault-amount" prop:value=move || amount.get() on:input=move |ev| amount.set(event_target_value(&ev)) />
                                <span class="unit-tag">{tok.symbol()}</span>
                            </div>
                            <button class="link sm" type="button" on:click=move |_| if let Some(a) = available() { amount.set(a.fmt_plain().replace(',', "")) }>"Use max"</button>
                        </div>
                        <p class="error-text" role="alert">{move || err.get()}</p>
                        <button class="btn primary wide" type="submit" data-test="vault-submit">
                            <Icon i=if inbound { Ic::MoveIn } else { Ic::MoveOut } />
                            <span>{if inbound { "Move in" } else { "Move out" }}</span>
                        </button>
                    </form>
                }.into_any(),
                Phase::Working => view! {
                    <div class="screen-head">
                        <h1 class="title" tabindex="-1">{if inbound { "Moving in…" } else { "Moving out privately" }}</h1>
                                            </div>
                    {if inbound {
                        view! { <ProofSteps progress=progress labels=["Depositing", "ASP approval", "Syncing pool"]
                            fine="The deposit comes from this account. Once the pool's ASP approves it, it's yours to send privately." /> }.into_any()
                    } else {
                        view! { <ProofSteps progress=progress /> }.into_any()
                    }}
                }.into_any(),
                Phase::Done(r, amt, _) => view! {
                    <DoneView title=if inbound { "Now private".to_string() } else { "Moved out".to_string() } amount=amt
                        detail=if inbound { "It's in your private balance. Only you can see it.".to_string() } else { format!("It's back in your {app_name} account.") }
                        receipt=r />
                }.into_any(),
            }}
        </Sheet>
    }
}
