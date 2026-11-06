//! Shared building blocks: the sheet frame, app tiles, address chips, copy.

use leptos::prelude::*;

use crate::backend::AppInfo;
use crate::format::short_addr;
use crate::state::{Screen, ctx};
use crate::ui::icons::{Ic, Icon, Lockup};

/// The right-hand action in the sheet's top bar.
#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub enum TopAction {
    #[default]
    None,
    Settings,
    Close,
}

/// The sheet frame every screen renders into.
#[component]
pub fn Sheet(
    /// Show a Back button instead of the lockup.
    #[prop(optional)]
    back: bool,
    #[prop(optional)] action: TopAction,
    /// Called by the Close button (defaults to Back).
    #[prop(optional)]
    on_close: Option<Callback<()>>,
    /// Hide the app chip (sign-in shows the app in the body instead).
    #[prop(optional)]
    bare: bool,
    #[prop(optional, into)] class: String,
    children: Children,
) -> impl IntoView {
    let c = ctx();
    let app = c.app;
    view! {
        <section class=format!("sheet {class}")>
            <header class="bar">
                <div class="bar-left">
                    {if back {
                        view! {
                            <button class="icon-btn" type="button" aria-label="Back" data-test="back" on:click=move |_| c.back()>
                                <Icon i=Ic::Back />
                            </button>
                        }.into_any()
                    } else {
                        view! { <Lockup /> }.into_any()
                    }}
                </div>
                {if !bare && !c.home.get_untracked() { view! {
                    <div class="bar-app" title=move || app.with(|a| a.origin.clone())>
                        {move || view! { <AppTile app=app.get() small=true /> }}
                        <span class="bar-app-name">{move || app.with(|a| a.name.clone())}</span>
                    </div>
                }.into_any() } else { view! { <span class="bar-mid"></span> }.into_any() }}
                <div class="bar-right">
                    {match action {
                        TopAction::Settings => view! {
                            <button class="icon-btn" type="button" aria-label="Settings" data-test="open-settings"
                                on:click=move |_| c.go(Screen::Settings)>
                                <Icon i=Ic::Sliders />
                            </button>
                        }.into_any(),
                        TopAction::Close => view! {
                            <button class="icon-btn" type="button" aria-label="Close"
                                on:click=move |_| match on_close { Some(cb) => cb.run(()), None => c.back() }>
                                <Icon i=Ic::Close />
                            </button>
                        }.into_any(),
                        TopAction::None => view! { <span class="icon-btn-spacer"></span> }.into_any(),
                    }}
                </div>
            </header>
            <div class="sheet-body">{children()}</div>
        </section>
    }
}

/// App tile: the app's monogram on ink.
#[component]
pub fn AppTile(app: AppInfo, #[prop(optional)] small: bool, #[prop(optional)] large: bool) -> impl IntoView {
    let cls = if small {
        "app-tile sm"
    } else if large {
        "app-tile lg"
    } else {
        "app-tile"
    };
    view! { <span class=cls aria-hidden="true">{app.monogram()}</span> }
}

pub fn copy_text(s: &str) {
    if let Some(w) = web_sys::window() {
        let _ = w.navigator().clipboard().write_text(s);
    }
}

fn capitalize(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
        None => String::new(),
    }
}

/// Copies, flips its icon to a check, and announces it.
fn copy_action(text: String, what: String, copied: RwSignal<bool>) -> impl Fn() + Clone {
    let c = ctx();
    move || {
        copy_text(&text);
        copied.set(true);
        c.say(format!("{} copied", capitalize(&what)));
        leptos::task::spawn_local(async move {
            gloo_timers::future::TimeoutFuture::new(1600).await;
            let _ = copied.try_set(false);
        });
    }
}

/// Labelled copy button ("Copy").
#[component]
pub fn CopyButton(#[prop(into)] text: String, #[prop(into)] what: String) -> impl IntoView {
    let copied = RwSignal::new(false);
    let aria = format!("Copy {what}");
    let act = copy_action(text, what, copied);
    view! {
        <button class="btn secondary sm" type="button" aria-label=aria on:click=move |_| act()>
            {move || if copied.get() { view! { <Icon i=Ic::Check size=16 /> }.into_any() } else { view! { <Icon i=Ic::Copy size=16 /> }.into_any() }}
            <span>{move || if copied.get() { "Copied" } else { "Copy" }}</span>
        </button>
    }
}

/// Address chip: pill, mono, truncated, copies the full value.
#[component]
pub fn Address(#[prop(into)] addr: String, #[prop(optional, into)] what: Option<String>) -> impl IntoView {
    let short = short_addr(&addr);
    let what = what.unwrap_or_else(|| "address".into());
    let copied = RwSignal::new(false);
    let aria = format!("Copy {what} {short}");
    let act = copy_action(addr.clone(), what, copied);
    view! {
        <button class="chip-addr" type="button" title=addr aria-label=aria on:click=move |_| act()>
            <span>{short}</span>
            {move || if copied.get() { view! { <Icon i=Ic::Check size=16 /> }.into_any() } else { view! { <Icon i=Ic::Copy size=16 /> }.into_any() }}
        </button>
    }
}

/// Pill marking something as shielded.
#[component]
pub fn PrivateBadge() -> impl IntoView {
    view! { <span class="badge-private"><Icon i=Ic::Keyhole size=14 />"Private"</span> }
}

#[component]
pub fn Spinner(#[prop(optional)] large: bool) -> impl IntoView {
    view! { <span class=if large { "spinner lg" } else { "spinner" } aria-hidden="true"></span> }
}

/// Button with a busy state. `kind`: primary (default), secondary, quiet,
/// danger, danger-outline; add "sm" for the compact size.
#[component]
pub fn Btn(
    #[prop(into)] label: Signal<String>,
    #[prop(optional, into)] busy_label: Option<String>,
    #[prop(optional, into)] busy: Signal<bool>,
    #[prop(optional, into)] disabled: Signal<bool>,
    #[prop(optional, into)] kind: String,
    #[prop(optional, into)] test: String,
    on_click: impl Fn() + 'static,
    #[prop(optional)] icon: Option<Ic>,
) -> impl IntoView {
    let cls = format!("btn {}", if kind.is_empty() { "primary" } else { &kind });
    let busy_label = busy_label.unwrap_or_default();
    view! {
        <button class=cls type="button" data-test=test
            disabled=move || busy.get() || disabled.get()
            aria-busy=move || busy.get().to_string()
            on:click=move |_| on_click()>
            {move || if busy.get() {
                view! { <Spinner /><span>{if busy_label.is_empty() { label.get() } else { busy_label.clone() }}</span> }.into_any()
            } else {
                view! { {icon.map(|i| view! { <Icon i=i />})}<span>{label.get()}</span> }.into_any()
            }}
        </button>
    }
}

/// Key/value row used by confirmation sheets.
#[component]
pub fn Kv(#[prop(into)] k: String, children: Children) -> impl IntoView {
    view! {
        <div class="kv">
            <dt>{k}</dt>
            <dd>{children()}</dd>
        </div>
    }
}
