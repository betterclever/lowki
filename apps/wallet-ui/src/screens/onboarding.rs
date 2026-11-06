//! Sign-in, email code, unlock, passkey creation and recovery code.

use leptos::prelude::*;
use leptos::task::spawn_local;

use crate::backend::{AuthOutcome, CodeStatus, backend};
use crate::platform::backup_name;
use crate::state::{Ctx, Screen, ctx};
use crate::ui::components::*;
use crate::ui::icons::{Ic, Icon};

/// After the seed is unlocked: fetch the pairwise session, tell the app, go home.
pub fn finish_sign_in(c: Ctx) {
    spawn_local(async move {
        let app = c.app.get_untracked();
        match backend().session(&app).await {
            Ok(s) => {
                c.announce_session(&s);
                c.session.set(Some(s));
                c.reset(Screen::Home);
            }
            Err(e) => {
                c.fail(e);
            }
        }
    });
}

fn route_outcome(c: Ctx, o: AuthOutcome) {
    match o {
        AuthOutcome::NewUser => c.go(Screen::SecureWallet),
        AuthOutcome::NeedsPasskey => c.go(Screen::Unlock),
    }
}

#[component]
pub fn SignIn() -> impl IntoView {
    let c = ctx();
    let app = c.app.get_untracked();
    let google_busy = RwSignal::new(false);
    let passkey_busy = RwSignal::new(false);
    let email_busy = RwSignal::new(false);
    let email = RwSignal::new(String::new());
    let err = RwSignal::new(None::<String>);
    let any_busy = Signal::derive(move || google_busy.get() || passkey_busy.get() || email_busy.get());

    let google = move || {
        err.set(None);
        google_busy.set(true);
        spawn_local(async move {
            let r = backend().sign_in_google(&c.app.get_untracked()).await;
            google_busy.set(false);
            match r {
                Ok(o) => route_outcome(c, o),
                Err(e) => err.set(c.fail(e)),
            }
        });
    };
    let passkey = move || {
        err.set(None);
        passkey_busy.set(true);
        spawn_local(async move {
            let r = backend().sign_in_passkey(&c.app.get_untracked()).await;
            passkey_busy.set(false);
            match r {
                Ok(s) => {
                    c.announce_session(&s);
                    c.session.set(Some(s));
                    c.reset(Screen::Home);
                }
                Err(e) => err.set(c.fail(e)),
            }
        });
    };
    let send_code = move || {
        let addr = email.get_untracked().trim().to_string();
        if !(addr.contains('@') && addr.contains('.') && addr.len() > 5) {
            err.set(Some("Enter a full email address, like name@example.com.".into()));
            return;
        }
        err.set(None);
        email_busy.set(true);
        spawn_local(async move {
            let r = backend().send_email_code(&addr).await;
            email_busy.set(false);
            match r {
                Ok(()) => c.go(Screen::EmailCode { email: addr }),
                Err(e) => err.set(c.fail(e)),
            }
        });
    };

    let email_open = RwSignal::new(false);
    view! {
        <Sheet bare=true class="signin">
            <div class="hero">
                <AppTile app=app.clone() large=true />
                <h1 class="title" tabindex="-1">"Continue to " {app.name.clone()}</h1>
                <p class="origin">{app.origin.trim_start_matches("https://").to_string()}</p>
            </div>
            <div class="stack">
                <button class="btn primary" type="button" data-test="passkey-signin"
                    disabled=move || any_busy.get() aria-busy=move || passkey_busy.get().to_string()
                    on:click=move |_| passkey()>
                    {move || if passkey_busy.get() { view! { <Spinner /> }.into_any() } else { view! { <Icon i=Ic::Passkey /> }.into_any() }}
                    <span>{move || if passkey_busy.get() { "Waiting for your passkey…" } else { "Continue with passkey" }}</span>
                </button>
                {move || c.show_google.get().then(|| view! {
                    <button class="btn secondary" type="button" data-test="google"
                        disabled=move || any_busy.get() aria-busy=move || google_busy.get().to_string()
                        on:click=move |_| google()>
                        {move || if google_busy.get() { view! { <Spinner /> }.into_any() } else { view! { <Icon i=Ic::Google /> }.into_any() }}
                        <span>{move || if google_busy.get() { "Waiting for Google…" } else { "Continue with Google" }}</span>
                    </button>
                })}
                {move || google_busy.get().then(|| view! {
                    <p class="hint" role="status">"Finish in the Google window. Google confirms it's you; it can't open your wallet."</p>
                })}
                <div class="divider" role="separator"><span>"or"</span></div>
                {move || if email_open.get() {
                    view! {
                        <form class="email-form" on:submit=move |ev| { ev.prevent_default(); send_code(); }>
                            <label class="label" for="email">"Email"</label>
                            <div class="field-row">
                                <input id="email" class="input" type="email" inputmode="email" autocomplete="email" autofocus
                                    placeholder="you@example.com" prop:value=move || email.get()
                                    aria-invalid=move || err.get().is_some().to_string()
                                    aria-describedby="signin-err" data-test="email-input"
                                    on:input=move |ev| email.set(event_target_value(&ev)) />
                                <button class="btn secondary" type="submit" data-test="email-continue"
                                    disabled=move || any_busy.get() aria-busy=move || email_busy.get().to_string()>
                                    {move || email_busy.get().then(|| view! { <Spinner /> })}
                                    <span>"Send code"</span>
                                </button>
                            </div>
                        </form>
                    }.into_any()
                } else {
                    view! {
                        <button class="btn secondary" type="button" data-test="email-open" disabled=move || any_busy.get()
                            on:click=move |_| email_open.set(true)>
                            <Icon i=Ic::Mail /><span>"Continue with email"</span>
                        </button>
                    }.into_any()
                }}
                <p id="signin-err" class="error-text" role="alert">{move || err.get()}</p>
            </div>
            <p class="fine">
                {app.name.clone()} " gets its own address for you. It can't see your email or your other apps."
            </p>
        </Sheet>
    }
}

#[component]
pub fn EmailCode(email: String) -> impl IntoView {
    let c = ctx();
    let code = RwSignal::new(String::new());
    let busy = RwSignal::new(false);
    let err = RwSignal::new(None::<String>);
    let resent = RwSignal::new(false);
    let email_v = email.clone();
    let verify = move || {
        let code_s: String = code.get_untracked().chars().filter(|c| c.is_ascii_digit()).collect();
        if code_s.len() != 6 {
            err.set(Some("Enter all 6 digits.".into()));
            return;
        }
        err.set(None);
        busy.set(true);
        let email = email_v.clone();
        spawn_local(async move {
            let r = backend().verify_email_code(&email, &code_s).await;
            busy.set(false);
            match r {
                Ok(o) => route_outcome(c, o),
                Err(e) => err.set(c.fail(e)),
            }
        });
    };
    let verify2 = verify.clone();
    let email_r = email.clone();
    let resend = move |_| {
        let email = email_r.clone();
        resent.set(true);
        spawn_local(async move {
            if backend().send_email_code(&email).await.is_ok() {
                c.say("New code sent");
            }
        });
    };
    view! {
        <Sheet back=true>
            <div class="screen-head">
                <span class="glyph"><Icon i=Ic::Mail size=24 /></span>
                <h1 class="title" tabindex="-1">"Check your email"</h1>
                <p class="sub">"We sent a 6-digit code to " <strong class="nowrap">{email.clone()}</strong> ". It expires in 10 minutes."</p>
            </div>
            <form class="stack" on:submit=move |ev| { ev.prevent_default(); verify(); }>
                <label class="label" for="code">"Code"</label>
                <input id="code" class="input code-input" type="text" inputmode="numeric" autocomplete="one-time-code"
                    maxlength="7" placeholder="000000" data-test="email-code"
                    aria-invalid=move || err.get().is_some().to_string() aria-describedby="code-err"
                    prop:value=move || code.get()
                    on:input=move |ev| {
                        let v: String = event_target_value(&ev).chars().filter(|c| c.is_ascii_digit()).take(6).collect();
                        code.set(v);
                    } />
                <p id="code-err" class="error-text" role="alert">{move || err.get()}</p>
                <Btn label="Continue".to_string() busy_label="Checking…" busy=busy test="email-verify" on_click=move || verify2() />
            </form>
            <div class="row-links">
                <button class="link" type="button" disabled=move || resent.get() on:click=resend>
                    {move || if resent.get() { "Code sent again" } else { "Send a new code" }}
                </button>
                <button class="link" type="button" on:click=move |_| c.back()>"Use a different email"</button>
            </div>
        </Sheet>
    }
}

#[component]
pub fn Unlock() -> impl IntoView {
    let c = ctx();
    let busy = RwSignal::new(false);
    let unlock_name = c.platform.get_untracked().unlock_name();
    let confirmed = if c.show_google.get_untracked() { "Google confirmed it's you." } else { "Your email is confirmed." };
    let go = move || {
        busy.set(true);
        spawn_local(async move {
            let r = backend().sign_in_passkey(&c.app.get_untracked()).await;
            busy.set(false);
            match r {
                Ok(s) => {
                    c.announce_session(&s);
                    c.session.set(Some(s));
                    c.reset(Screen::Home);
                }
                Err(e) => {
                    c.fail(e);
                }
            }
        });
    };
    view! {
        <Sheet back=true>
            <div class="screen-head center">
                <span class="glyph lg"><Icon i=Ic::Passkey size=28 /></span>
                <h1 class="title" tabindex="-1">"Welcome back"</h1>
                <p class="sub">{confirmed} " Use " {unlock_name} " to open your wallet."</p>
            </div>
            <div class="actions-col">
                <Btn label=c.platform.get_untracked().create_label().replace("Create passkey", "Use passkey") busy_label="Waiting for your passkey…" busy=busy test="unlock" on_click=go />
                <button class="btn quiet" type="button" data-test="use-recovery-code" disabled=move || busy.get()
                    on:click=move |_| c.go(Screen::RecoverCode)>
                    "Lost your passkey? Use your recovery code"
                </button>
            </div>
        </Sheet>
    }
}

#[component]
pub fn SecureWallet() -> impl IntoView {
    let c = ctx();
    let p = c.platform.get_untracked();
    let busy = RwSignal::new(false);
    let err = RwSignal::new(None::<String>);
    let create = move || {
        err.set(None);
        busy.set(true);
        spawn_local(async move {
            let r = backend().create_passkey().await;
            busy.set(false);
            match r {
                Ok(_) if c.recovering.get_untracked() => {
                    // Same wallet, new passkey: the code the user just typed still works.
                    c.recovering.set(false);
                    c.recovery_reminder.set(false);
                    finish_sign_in(c);
                }
                Ok(code) => {
                    c.recovery_code.set(Some(code));
                    c.go(Screen::Recovery);
                }
                Err(e) => {
                    if let Some(m) = c.fail(e) {
                        err.set(Some(m));
                    }
                }
            }
        });
    };
    let recovering = c.recovering.get_untracked();
    view! {
        <Sheet back=true>
            <div class="screen-head center">
                <span class="glyph lg accent"><Icon i=Ic::Passkey size=28 /></span>
                {if recovering {
                    view! {
                        <h1 class="title" tabindex="-1">"Your wallet is back"</h1>
                        <p class="sub">"Create a new passkey so " {p.unlock_name()} " can open it from now on. Lowki never sees it."</p>
                    }.into_any()
                } else {
                    view! {
                        <h1 class="title" tabindex="-1">"Secure your wallet"</h1>
                        <p class="sub">"Create a passkey so only " {p.unlock_name()} " can open your wallet. Lowki never sees it."</p>
                    }.into_any()
                }}
            </div>
            <ul class="facts">
                <li><Icon i=Ic::Check /><span>{p.passkey_fact()}</span></li>
                <li><Icon i=Ic::Check /><span>{if c.show_google.get_untracked() { "Google" } else { "Your email" }} " alone can't open it. Losing a device doesn't lose your funds."</span></li>
            </ul>
            <p class="error-text" role="alert">{move || err.get()}</p>
            <div class="actions-col">
                <Btn label=p.create_label() busy_label=format!("Waiting for {}…", p.unlock_name()) busy=busy test="create-passkey" on_click=create />
            </div>
        </Sheet>
    }
}

#[component]
pub fn Recovery() -> impl IntoView {
    let c = ctx();
    let p = c.platform.get_untracked();
    let target = p.backup_target();
    let code = c.recovery_code.get_untracked();
    let backing = RwSignal::new(false);
    let backed = RwSignal::new(false);
    let finishing = RwSignal::new(false);
    let code_text = code.as_ref().map(|c| c.0.clone()).unwrap_or_default();
    let groups = code.map(|c| c.groups()).unwrap_or_default();

    let backup = move || {
        backing.set(true);
        spawn_local(async move {
            let r = backend().backup_recovery(target).await;
            backing.set(false);
            match r {
                Ok(()) => {
                    backed.set(true);
                    c.say(format!("Saved to {}", backup_name(target)));
                }
                Err(e) => {
                    if let Some(m) = c.fail(e) {
                        c.say(m);
                    }
                }
            }
        });
    };
    let finish = move |saved: bool| {
        finishing.set(true);
        spawn_local(async move {
            let _ = backend().acknowledge_recovery(saved).await;
            let status = backend().recovery_status().await.ok();
            c.recovery_reminder.set(status.map(|s| s.code == CodeStatus::NotSaved).unwrap_or(!saved));
            finish_sign_in(c);
        });
    };
    view! {
        <Sheet>
            <div class="screen-head">
                <h1 class="title" tabindex="-1">"Save your recovery code"</h1>
                <p class="sub">"If you lose your passkey, this code plus your " {c.social_name()} " brings your wallet back. We'll show it only once."</p>
            </div>
            <div class="code-card" role="group" aria-label="Recovery code">
                <ol class="code-grid" data-test="recovery-code">
                    {groups.into_iter().enumerate().map(|(i, g)| view! {
                        <li><span class="code-n">{i + 1}</span><span class="mono">{g}</span></li>
                    }).collect_view()}
                </ol>
                <div class="code-actions">
                    <CopyButton text=code_text what="recovery code" />
                    <button class="btn secondary sm" type="button" data-test="backup" disabled=move || backing.get() || backed.get()
                        aria-busy=move || backing.get().to_string() on:click=move |_| backup()>
                        {move || if backing.get() { view! { <Spinner /> }.into_any() }
                            else if backed.get() { view! { <Icon i=Ic::Check size=16 /> }.into_any() }
                            else { view! { <Icon i=Ic::Upload size=16 /> }.into_any() }}
                        <span>{move || if backed.get() { format!("Backed up to {}", backup_name(target)) } else { format!("Back up to {}", backup_name(target)) }}</span>
                    </button>
                </div>
            </div>
            <p class="fine left">"Keep this recovery code somewhere safe. It's the only way back in without this device. Anyone with it and your " {c.social_name()} " can open your wallet."</p>
            <div class="actions-col">
                <Btn label="I've saved it".to_string() busy=finishing test="recovery-done" on_click=move || finish(true) />
                <button class="btn quiet" type="button" data-test="recovery-skip" disabled=move || finishing.get() on:click=move |_| finish(false)>
                    "Remind me later"
                </button>
            </div>
        </Sheet>
    }
}

/// Lost passkey: email sign-in + recovery code rebuild the wallet, then a new passkey.
#[component]
pub fn RecoverCode() -> impl IntoView {
    let c = ctx();
    let code = RwSignal::new(String::new());
    let busy = RwSignal::new(false);
    let err = RwSignal::new(None::<String>);
    let submit = move || {
        let v = code.get_untracked();
        if v.chars().filter(|c| c.is_ascii_alphanumeric()).count() < 16 {
            err.set(Some("Enter the whole recovery code.".into()));
            return;
        }
        err.set(None);
        busy.set(true);
        spawn_local(async move {
            let r = backend().recover_with_code(&v).await;
            busy.set(false);
            match r {
                Ok(()) => {
                    code.set(String::new());
                    c.recovering.set(true);
                    c.go(Screen::SecureWallet);
                }
                Err(e) => err.set(c.fail(e)),
            }
        });
    };
    let submit2 = submit;
    view! {
        <Sheet back=true>
            <div class="screen-head">
                <span class="glyph"><Icon i=Ic::Keyhole size=24 /></span>
                <h1 class="title" tabindex="-1">"Enter your recovery code"</h1>
                <p class="sub">"With your " {c.social_name()} " confirmed, the code you saved when you set up the wallet brings it back. Then you'll create a new passkey."</p>
            </div>
            <form class="stack" on:submit=move |ev| { ev.prevent_default(); submit(); }>
                <label class="label" for="rcode">"Recovery code"</label>
                <textarea id="rcode" class="input mono" rows="3" autocomplete="off" spellcheck="false" autocapitalize="characters"
                    placeholder="K7QM-9XHD-…" data-test="recovery-input"
                    aria-invalid=move || err.get().is_some().to_string() aria-describedby="rcode-err"
                    prop:value=move || code.get() on:input=move |ev| code.set(event_target_value(&ev))></textarea>
                <p id="rcode-err" class="error-text" role="alert">{move || err.get()}</p>
                <Btn label="Recover wallet".to_string() busy_label="Checking…" busy=busy test="recover" on_click=move || submit2() />
            </form>
        </Sheet>
    }
}
