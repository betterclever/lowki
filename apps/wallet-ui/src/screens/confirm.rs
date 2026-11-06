//! Confirmation sheets for app requests: message, typed data, transaction.

use leptos::prelude::*;
use leptos::task::spawn_local;

use std::rc::Rc;

use crate::backend::{Amount, GasQuote, ProgressFn, ProofProgress, ProofStage, SendRequest, TxRequest, backend};
use crate::format::{Field, decode_typed, short_addr};
use crate::protocol::{RejectReason, WalletToDapp};
use crate::state::ctx;
use crate::ui::components::*;
use crate::ui::icons::{Ic, Icon};

/// Request header: who is asking, for which account.
#[component]
fn Requester(#[prop(into)] verb: String) -> impl IntoView {
    let c = ctx();
    let app = c.app.get_untracked();
    let addr = c.session.get_untracked().map(|s| s.address).unwrap_or_default();
    view! {
        <div class="requester">
            <AppTile app=app.clone() />
            <div>
                <p class="eyebrow">{format!("{} · {}", app.name, app.origin.trim_start_matches("https://"))}</p>
                <p class="requester-line">{verb}</p>
                <p class="row-sub">"From your " {app.name.clone()} " account " <span class="mono">{short_addr(&addr)}</span></p>
            </div>
        </div>
    }
}

/// Decline / approve footer shared by all request sheets.
#[component]
fn Decide(
    #[prop(into)] approve: String,
    busy: RwSignal<bool>,
    #[prop(optional, into)] disabled: Signal<bool>,
    on_approve: impl Fn() + 'static,
    on_reject: impl Fn() + 'static,
    #[prop(optional, into)] reject: Option<String>,
) -> impl IntoView {
    view! {
        <div class="actions decide">
            <button class="btn secondary" type="button" data-test="reject" disabled=move || busy.get() on:click=move |_| on_reject()>
                {reject.unwrap_or_else(|| "Reject".into())}
            </button>
            <Btn label=approve busy=busy disabled=disabled test="approve" on_click=on_approve />
        </div>
    }
}

#[component]
pub fn ConfirmMessage(id: String, message: String) -> impl IntoView {
    let c = ctx();
    let busy = RwSignal::new(false);
    let (id_a, id_r, msg) = (id.clone(), id.clone(), message.clone());
    let approve = move || {
        busy.set(true);
        let (id, msg) = (id_a.clone(), msg.clone());
        spawn_local(async move {
            let r = backend().sign_message(&c.app.get_untracked(), &msg).await;
            busy.set(false);
            match r {
                Ok(sig) => {
                    c.post(WalletToDapp::MessageSigned { id, signature: sig });
                    c.say("Signed");
                    c.done();
                }
                Err(e) => {
                    c.reject(&id, RejectReason::NetworkError, &e.message());
                    c.fail(e);
                }
            }
        });
    };
    let reject = move || {
        c.reject(&id_r, RejectReason::UserRejected, "User cancelled");
        c.done();
    };
    let reject_x = reject.clone();
    view! {
        <Sheet class="request" action=TopAction::Close on_close=Callback::new(move |_| reject_x())>
            <h1 class="title" tabindex="-1">"Approve this message?"</h1>
            <Requester verb="wants you to sign this message" />
            <pre class="message" tabindex="0" aria-label="Message to sign">{message}</pre>
            <p class="note"><Icon i=Ic::Info size=16 />"Signing proves this account agrees. It doesn't move money or cost anything."</p>
            <Decide approve="Approve" busy=busy on_approve=approve on_reject=reject />
        </Sheet>
    }
}

#[component]
fn FieldRows(fields: Vec<Field>) -> impl IntoView {
    view! {
        <dl class="kvs">
            {fields.into_iter().map(|f| {
                let pad = format!("padding-left:{}px", f.depth as u32 * 14);
                if f.value.is_empty() {
                    view! { <div class="kv group" style=pad><dt>{f.label}</dt><dd></dd></div> }.into_any()
                } else {
                    let what = f.label.to_lowercase();
                    view! {
                        <div class="kv" style=pad>
                            <dt>{f.label}</dt>
                            <dd>{match f.full {
                                Some(full) if full.starts_with("0x") && full.len() > 14 && f.value.starts_with("0x") => view! { <Address addr=full what=what /> }.into_any(),
                                Some(full) if full.starts_with("0x") && full.len() > 14 => {
                                    let name = f.value.split(" (").next().unwrap_or_default().to_string();
                                    view! { <span class="stacked"><span>{name}</span><Address addr=full what=what /></span> }.into_any()
                                }
                                Some(full) => view! { <span class="mono" title=full>{f.value}</span> }.into_any(),
                                None => view! { <span>{f.value}</span> }.into_any(),
                            }}</dd>
                        </div>
                    }.into_any()
                }
            }).collect_view()}
        </dl>
    }
}

#[component]
pub fn ConfirmTyped(id: String, data: serde_json::Value) -> impl IntoView {
    let c = ctx();
    let busy = RwSignal::new(false);
    let b = backend();
    let view_data = decode_typed(&data, &|a| b.token_at(a));
    let (id_a, id_r) = (id.clone(), id.clone());
    let approve = move || {
        busy.set(true);
        let (id, data) = (id_a.clone(), data.clone());
        spawn_local(async move {
            let r = backend().sign_typed_data(&c.app.get_untracked(), &data).await;
            busy.set(false);
            match r {
                Ok(sig) => {
                    c.post(WalletToDapp::TypedDataSigned { id, signature: sig });
                    c.say("Signed");
                    c.done();
                }
                Err(e) => {
                    c.reject(&id, RejectReason::NetworkError, &e.message());
                    c.fail(e);
                }
            }
        });
    };
    let reject = move || {
        c.reject(&id_r, RejectReason::UserRejected, "User cancelled");
        c.done();
    };
    let reject_x = reject.clone();
    let valid = view_data.is_some();
    view! {
        <Sheet class="request" action=TopAction::Close on_close=Callback::new(move |_| reject_x())>
            <h1 class="title" tabindex="-1">"Approve this signature?"</h1>
            <Requester verb="wants you to sign structured data" />
            {match view_data {
                Some(v) => view! {
                    <section class="section tight" aria-labelledby="td-msg">
                        <h2 id="td-msg" class="label">{v.primary}</h2>
                        <FieldRows fields=v.message />
                    </section>
                    <details class="details">
                        <summary>"Where this signature is valid"</summary>
                        <FieldRows fields=v.domain />
                    </details>
                }.into_any(),
                None => view! {
                    <div class="alert danger" role="alert"><Icon i=Ic::Alert /><div>"This request is malformed, so Lowki can't show what it says. Don't sign it."</div></div>
                }.into_any(),
            }}
            <p class="note"><Icon i=Ic::Info size=16 />"Off-chain signature. Nothing is sent now, but the app can use it later as shown above."</p>
            <Decide approve="Approve" busy=busy disabled=Signal::derive(move || !valid) on_approve=approve on_reject=reject />
        </Sheet>
    }
}

#[component]
pub fn ConfirmTx(id: String, tx: TxRequest) -> impl IntoView {
    let c = ctx();
    let busy = RwSignal::new(false);
    let tx_p = tx.clone();
    let tx_data = tx.data.clone();
    let preview = LocalResource::new(move || {
        let tx = tx_p.clone();
        let app = c.app.get_untracked();
        async move { backend().preview_transaction(&app, &tx).await }
    });
    let (id_a, id_r) = (id.clone(), id.clone());
    let approve = move || {
        busy.set(true);
        let (id, tx) = (id_a.clone(), tx.clone());
        let sponsored = matches!(preview.get_untracked(), Some(Ok(ref p)) if matches!(p.gas, GasQuote::Sponsored { .. }));
        spawn_local(async move {
            let r = backend().send_transaction(&c.app.get_untracked(), &tx).await;
            busy.set(false);
            match r {
                Ok(rc) => {
                    c.post(WalletToDapp::TransactionSent { id, hash: rc.hash, sponsored });
                    c.say("Transaction sent");
                    c.done();
                }
                Err(e) => {
                    c.reject(&id, RejectReason::NetworkError, &e.message());
                    c.fail(e);
                }
            }
        });
    };
    let id_f = id.clone();
    Effect::new(move |_| {
        if let Some(Err(e)) = preview.get() {
            c.reject(&id_f, RejectReason::NetworkError, &e.message());
            c.fail(e);
        }
    });
    let reject = move || {
        c.reject(&id_r, RejectReason::UserRejected, "User cancelled");
        c.done();
    };
    let reject_x = reject.clone();
    let loading = Signal::derive(move || !matches!(preview.get(), Some(Ok(_))));
    view! {
        <Sheet class="request" action=TopAction::Close on_close=Callback::new(move |_| reject_x())>
            <h1 class="title" tabindex="-1">"Approve this transaction?"</h1>
            <Requester verb="wants to send a transaction" />
            {move || match preview.get() {
                Some(Ok(p)) => { let to = p.to.clone(); view! {
                    <dl class="kvs" data-test="tx-preview">
                        <Kv k="To">
                            <span class="stacked">
                                {p.to_label.clone().map(|l| view! { <span>{l}</span> })}
                                <Address addr=p.to.clone() what="contract address" />
                            </span>
                        </Kv>
                        {p.call.clone().map(|call| view! { <Kv k="Action"><span>{call.function.clone()}</span></Kv> })}
                        <Kv k="Amount"><span class="mono">{p.value.fmt()}</span></Kv>
                        <Kv k="Network fee">{match p.gas.clone() {
                            GasQuote::Sponsored { by } => view! { <span>{format!("Covered by {by}")}</span> }.into_any(),
                            GasQuote::Paid { amount } => view! { <span class="mono">{format!("≈ {}", amount.fmt())}</span> }.into_any(),
                        }}</Kv>
                        <Kv k="Network"><span>{p.network.clone()}</span></Kv>
                    </dl>
                    {p.call.clone().map(|call| {
                        let args = call.args.iter().map(|(k, v)| format!("  {k}: {v}")).collect::<Vec<_>>().join(",\n");
                        view! {
                            <details class="details">
                                <summary>"Show call data"</summary>
                                <pre>{format!("{}(\n{args}\n)\n\nto   {}\ndata {}", call.function, to, tx_data.clone())}</pre>
                            </details>
                        }
                    })}
                }.into_any() }
                _ => view! {
                    <div class="loading-block" role="status"><Spinner /><span>"Reading the transaction…"</span></div>
                }.into_any(),
            }}
            <Decide approve="Approve" busy=busy disabled=loading on_approve=approve on_reject=reject />
        </Sheet>
    }
}

/// `send_privately` from the app: a private transfer from the user's private balance.
/// The proof is built in this browser; the recipient can't see which account sent it.
#[component]
pub fn ConfirmPrivate(id: String, to: String, amount: Amount) -> impl IntoView {
    let c = ctx();
    let busy = RwSignal::new(false);
    let stage = RwSignal::new(None::<ProofStage>);
    let err = RwSignal::new(None::<String>);
    let (id_a, id_r, to_a) = (id.clone(), id.clone(), to.clone());
    let approve = move || {
        busy.set(true);
        err.set(None);
        let (id, req) = (id_a.clone(), SendRequest { to: to_a.clone(), amount, private: true });
        let progress: ProgressFn = Rc::new(move |p: ProofProgress| stage.set(Some(p.stage)));
        spawn_local(async move {
            let r = backend().send(&c.app.get_untracked(), req, progress).await;
            busy.set(false);
            match r {
                Ok(rc) => {
                    c.post(WalletToDapp::PrivateSent { id, hash: rc.hash });
                    c.say("Sent privately");
                    c.done();
                }
                // Field-level errors (bad address, not enough private balance) end the
                // request; the sheet stays to say why until the user closes it.
                Err(e) => {
                    c.reject(&id, RejectReason::NetworkError, &e.message());
                    if let Some(m) = c.fail(e) {
                        err.set(Some(m));
                    }
                }
            }
        });
    };
    let reject = move || {
        c.reject(&id_r, RejectReason::UserRejected, "User cancelled");
        c.done();
    };
    let reject_x = reject.clone();
    let stage_text = move || {
        stage.get().map(|s| match s {
            ProofStage::SyncingPool => "Syncing the private pool…",
            ProofStage::BuildingProof => "Building the proof on this device…",
            ProofStage::Submitting => "Submitting…",
        })
    };
    view! {
        <Sheet class="request" action=TopAction::Close on_close=Callback::new(move |_| reject_x())>
            <h1 class="title" tabindex="-1">"Send privately?"</h1>
            <Requester verb="wants you to send privately" />
            <dl class="kvs" data-test="private-preview">
                <Kv k="To"><Address addr=to.clone() what="recipient address" /></Kv>
                <Kv k="Amount"><span class="mono">{amount.fmt()}</span></Kv>
                <Kv k="From"><span>"Your private balance"</span></Kv>
            </dl>
            <p class="note"><Icon i=Ic::Info size=16 />"The recipient sees the money arrive, not which account sent it."</p>
            <p class="hint" role="status" aria-live="polite">{stage_text}</p>
            <p class="error-text" role="alert">{move || err.get()}</p>
            <Decide approve="Send privately" busy=busy disabled=Signal::derive(move || err.get().is_some()) on_approve=approve on_reject=reject />
        </Sheet>
    }
}
