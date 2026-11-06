//! Wallet home for the current app account.

use leptos::prelude::*;

use crate::backend::types::group_thousands;
use crate::backend::{Activity, ActivityKind, Balances, Token, VaultDirection, backend};
use crate::format::short_addr;
use crate::state::{Screen, ctx};
use crate::ui::components::*;
use crate::ui::icons::{Ic, Icon};

/// "USDC or MON on Monad".
fn fund_hint(b: &Balances) -> String {
    let syms: Vec<&str> = b.tokens().into_iter().map(Token::symbol).collect();
    format!("{} on {}", syms.join(" or "), b.network)
}

fn usd(cents: u64) -> String {
    format!("${}.{:02}", group_thousands((cents / 100) as u128), cents % 100)
}

#[component]
pub fn Home() -> impl IntoView {
    let c = ctx();
    let session = c.session;
    let balances = LocalResource::new(move || {
        c.refresh.track();
        let app = c.app.get_untracked();
        async move { backend().balances(&app).await }
    });
    let activity = LocalResource::new(move || {
        c.refresh.track();
        let app = c.app.get_untracked();
        async move { backend().activity(&app).await }
    });
    // A failed fetch goes to the network error screen.
    Effect::new(move |_| {
        if let Some(Err(e)) = balances.get() {
            c.fail(e);
        }
    });
    let address = move || session.with(|s| s.as_ref().map(|s| s.address.clone()).unwrap_or_default());
    let app_name = c.app.get_untracked().name;

    view! {
        <Sheet action=TopAction::Settings class="home">
            <h1 class="sr-only" tabindex="-1">{format!("Your {app_name} wallet")}</h1>
            {move || c.recovery_reminder.get().then(|| view! {
                <div class="alert" role="note">
                    <Icon i=Ic::Alert />
                    <div><strong>"Recovery code not saved"</strong><span>"Without it, losing your passkey means losing this wallet."</span></div>
                    <button class="btn quiet sm" type="button" on:click=move |_| c.go(Screen::Settings)>"Review"</button>
                </div>
            })}
            <div class="account">
                <div class="account-head">
                    <span class="label">{format!("{app_name} account")}</span>
                    {move || view! { <Address addr=address() what="address" /> }}
                </div>
                <div class="balance" data-test="balance">
                    {move || match balances.get() {
                        Some(Ok(b)) => view! { <span>{b.public_of(Token::Usdc).fmt_plain()}</span><small>"USDC"</small> }.into_any(),
                        _ => view! { <span class="skel w-40" aria-label="Loading balance"></span> }.into_any(),
                    }}
                </div>
                <p class="meta-line">{move || balances.get().and_then(Result::ok).map(|b| format!("{} in total on {}", usd(b.usd_cents), b.network))}</p>
            </div>
            <div class="actions">
                <Btn label="Send".to_string() icon=Ic::ArrowUpRight test="send" on_click=move || c.go(Screen::Send) />
                <Btn label="Receive".to_string() kind="secondary" icon=Ic::ArrowDownLeft on_click=move || {
                    copy_text(&address());
                    let what = balances.get_untracked().and_then(Result::ok).map(|b| fund_hint(&b)).unwrap_or_else(|| "funds".into());
                    c.say(format!("Address copied. Send {what} to it."));
                } />
            </div>
            {move || balances.get().and_then(Result::ok).map(|b| view! { <Vault b=b.clone() /> <Tokens b=b /> })}
            <section class="section" aria-labelledby="act-h">
                <h2 id="act-h" class="label">"Recent activity"</h2>
                {move || match activity.get() {
                    None => view! {
                        <ul class="list">{(0..3).map(|_| view! { <li class="row"><span class="skel w-60"></span></li> }).collect_view()}</ul>
                    }.into_any(),
                    Some(Ok(items)) if items.is_empty() => view! {
                        <div class="empty">
                            <Icon i=Ic::Clock size=24 />
                            <p><strong>"Nothing here yet"</strong></p>
                            <p>"Money you send or receive with this account shows up here."</p>
                        </div>
                    }.into_any(),
                    Some(Ok(items)) => view! {
                        <ul class="list" data-test="activity">{items.into_iter().map(|a| view! { <ActivityRow a=a /> }).collect_view()}</ul>
                    }.into_any(),
                    Some(Err(_)) => view! { <p class="muted">"Activity couldn't load."</p> }.into_any(),
                }}
            </section>
        </Sheet>
    }
}

#[component]
fn Tokens(b: Balances) -> impl IntoView {
    let empty = b.public.iter().all(|a| a.units == 0);
    let hint = fund_hint(&b);
    view! {
        <section class="section" aria-labelledby="tok-h">
            <h2 id="tok-h" class="label">"In this account"</h2>
            <ul class="list">
                {b.tokens().into_iter().map(|t| {
                    let a = b.public_of(t);
                    view! {
                        <li class="row">
                            <span class="tok" aria-hidden="true">{t.symbol().chars().next().unwrap_or('?').to_string()}</span>
                            <span class="row-main"><span class="row-title">{t.symbol()}</span><span class="row-sub">{t.name()}</span></span>
                            <span class="row-amt mono">{a.fmt_plain()}</span>
                        </li>
                    }
                }).collect_view()}
            </ul>
            {empty.then(|| view! {
                <p class="fine left">{format!("This account is empty. Copy the address above and send {hint} to add funds.")}</p>
            })}
        </section>
    }
}

#[component]
fn Vault(b: Balances) -> impl IntoView {
    let c = ctx();
    let p = b.private_of(Token::Usdc);
    let has_public = b.public_of(Token::Usdc).units > 0;
    let has_private = p.units > 0;
    view! {
        <section class="private-card" aria-labelledby="vault-h">
            <div class="private-row">
                <h2 id="vault-h" class="label">"Private balance"</h2>
                <PrivateBadge />
            </div>
            <div class="balance md" data-test="private-balance"><span>{p.fmt_plain()}</span><small>"USDC"</small></div>
            <p class="private-note">"Only you can see this. No app can, including this one."</p>
            <div class="private-actions">
                <button class="btn on-private sm" type="button" data-test="move-in" disabled=!has_public
                    on:click=move |_| c.go(Screen::Vault(VaultDirection::In))>
                    <Icon i=Ic::MoveIn size=18 /><span>"Move in"</span>
                </button>
                <button class="btn on-private sm" type="button" data-test="move-out" disabled=!has_private
                    on:click=move |_| c.go(Screen::Vault(VaultDirection::Out))>
                    <Icon i=Ic::MoveOut size=18 /><span>"Move out"</span>
                </button>
            </div>
        </section>
    }
}

#[component]
fn ActivityRow(a: Activity) -> impl IntoView {
    let (icon, title, sign, private) = match a.kind {
        ActivityKind::Received => (Ic::ArrowDownLeft, "Received".to_string(), "+", false),
        ActivityKind::Sent => (Ic::ArrowUpRight, "Sent".to_string(), "−", false),
        ActivityKind::SentPrivately => (Ic::Keyhole, "Sent privately".to_string(), "−", true),
        ActivityKind::MovedToPrivate => (Ic::Keyhole, "Moved to private".to_string(), "", true),
        ActivityKind::MovedFromPrivate => (Ic::MoveOut, "Moved out of private".to_string(), "", true),
        ActivityKind::AppAction => (Ic::Grid, a.note.clone().unwrap_or_else(|| "App action".into()), "−", false),
    };
    let who = a.counterparty.as_ref().map(|c| if c.starts_with("0x") { short_addr(c) } else { c.clone() });
    let sub = match (who, a.kind) {
        (Some(w), ActivityKind::Received) => format!("From {w} · {}", a.when),
        (Some(w), ActivityKind::AppAction) => format!("{w} · {}", a.when),
        (Some(w), _) => format!("To {w} · {}", a.when),
        (None, _) => a.when.clone(),
    };
    view! {
        <li class="row">
            <span class=if private { "row-icon private" } else { "row-icon" }><Icon i=icon size=18 /></span>
            <span class="row-main"><span class="row-title">{title}</span><span class="row-sub">{sub}</span></span>
            <span class=format!("row-amt mono {}", if sign == "+" { "pos" } else { "" })>{format!("{sign}{}", a.amount.fmt())}</span>
        </li>
    }
}
