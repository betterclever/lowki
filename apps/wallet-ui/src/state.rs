//! App-wide reactive state and navigation.

use leptos::prelude::*;

use crate::backend::{
    AppInfo, DelegationPolicy, RecoveryCode, Session, TxRequest, VaultDirection, WalletError,
};
use crate::platform::Platform;
use crate::protocol::{self, RejectReason, WalletToDapp};

/// What the user is looking at.
#[derive(Clone, Debug, PartialEq)]
pub enum Screen {
    SignIn,
    EmailCode { email: String },
    /// Known user after Google/email: unlock with the passkey.
    Unlock,
    SecureWallet,
    Recovery,
    /// Lost passkey: enter the recovery code (after an email sign-in).
    RecoverCode,
    Home,
    Send,
    Vault(VaultDirection),
    Settings,
    ConfirmMessage { id: String, message: String },
    ConfirmTyped { id: String, data: serde_json::Value },
    ConfirmTx { id: String, tx: TxRequest },
    Delegation { id: String, policy: DelegationPolicy },
    Error(ErrorKind),
}

#[derive(Clone, Debug, PartialEq)]
pub enum ErrorKind {
    PasskeyCancelled,
    PrfUnsupported,
    Network,
    PolicyRejected { id: String, reason: String },
}

#[derive(Clone, Copy)]
pub struct Ctx {
    pub app: RwSignal<AppInfo>,
    pub screen: RwSignal<Screen>,
    history: RwSignal<Vec<Screen>>,
    pub session: RwSignal<Option<Session>>,
    pub recovery_code: RwSignal<Option<RecoveryCode>>,
    pub platform: RwSignal<Platform>,
    /// Polite announcement (copied, saved, revoked...).
    pub toast: RwSignal<Option<String>>,
    toast_seq: RwSignal<u32>,
    /// Origin replies are posted to.
    pub parent_origin: RwSignal<String>,
    /// Open sign-in request from the app, answered on success.
    pub sign_in_request: RwSignal<Option<String>>,
    /// Bumped to make data views refetch.
    pub refresh: RwSignal<u32>,
    /// Recovery code not yet saved: show the reminder on Home.
    pub recovery_reminder: RwSignal<bool>,
    /// Rebuilt from the recovery code: the next passkey re-wraps the same wallet.
    pub recovering: RwSignal<bool>,
    /// Show "Continue with Google" (off unless the backend has it, or `google=1`).
    pub show_google: RwSignal<bool>,
}

impl Ctx {
    pub fn new(app: AppInfo, platform: Platform, parent_origin: String) -> Self {
        Self {
            app: RwSignal::new(app),
            screen: RwSignal::new(Screen::SignIn),
            history: RwSignal::new(vec![]),
            session: RwSignal::new(None),
            recovery_code: RwSignal::new(None),
            platform: RwSignal::new(platform),
            toast: RwSignal::new(None),
            toast_seq: RwSignal::new(0),
            parent_origin: RwSignal::new(parent_origin),
            sign_in_request: RwSignal::new(None),
            refresh: RwSignal::new(0),
            recovery_reminder: RwSignal::new(false),
            recovering: RwSignal::new(false),
            show_google: RwSignal::new(true),
        }
    }

    /// Navigate forward (Back returns here).
    pub fn go(&self, s: Screen) {
        let cur = self.screen.get_untracked();
        if cur != s {
            self.history.update(|h| h.push(cur));
            self.screen.set(s);
        }
    }
    /// Replace the current screen without growing history.
    pub fn replace(&self, s: Screen) {
        self.screen.set(s);
    }
    /// Start a fresh stack (after sign-in, after finishing a flow).
    pub fn reset(&self, s: Screen) {
        self.history.set(vec![]);
        self.screen.set(s);
    }
    pub fn back(&self) {
        let prev = self.history.try_update(|h| h.pop()).flatten();
        match prev {
            Some(p) => self.screen.set(p),
            None => self.screen.set(self.home_or_sign_in()),
        }
    }
    pub fn home_or_sign_in(&self) -> Screen {
        if self.session.get_untracked().is_some() { Screen::Home } else { Screen::SignIn }
    }
    /// Leave a flow and land on Home (or sign-in).
    pub fn done(&self) {
        self.reset(self.home_or_sign_in());
        self.refresh.update(|n| *n += 1);
    }

    pub fn say(&self, msg: impl Into<String>) {
        self.toast.set(Some(msg.into()));
        self.toast_seq.update(|n| *n += 1);
        let seq = self.toast_seq.get_untracked();
        let toast = self.toast;
        let toast_seq = self.toast_seq;
        leptos::task::spawn_local(async move {
            gloo_timers::future::TimeoutFuture::new(2600).await;
            if toast_seq.get_untracked() == seq {
                toast.set(None);
            }
        });
    }

    pub fn post(&self, msg: WalletToDapp) {
        protocol::post(&msg, &self.parent_origin.get_untracked());
    }

    pub fn reject(&self, id: &str, reason: RejectReason, message: &str) {
        self.post(WalletToDapp::Rejected { id: id.into(), reason, message: message.into() });
    }

    /// Route a backend error to its full-screen state. Returns the inline
    /// message for errors that belong next to a field instead.
    pub fn fail(&self, e: WalletError) -> Option<String> {

        match e {
            WalletError::PasskeyCancelled => self.go(Screen::Error(ErrorKind::PasskeyCancelled)),
            WalletError::PrfUnsupported => self.go(Screen::Error(ErrorKind::PrfUnsupported)),
            WalletError::Network => self.go(Screen::Error(ErrorKind::Network)),
            WalletError::PolicyRejected { reason } => {
                self.go(Screen::Error(ErrorKind::PolicyRejected { id: String::new(), reason }))
            }
            other => return Some(other.message()),
        }
        None
    }

    /// What, besides the recovery code, brings the wallet back ("Google account" or "email").
    pub fn social_name(&self) -> &'static str {
        if self.show_google.get_untracked() { "Google account" } else { "email" }
    }

    /// Publish the session to the app after sign-in.
    pub fn announce_session(&self, s: &Session) {
        let id = self.sign_in_request.get_untracked();
        self.sign_in_request.set(None);
        self.post(WalletToDapp::Authenticated {
            id,
            address: s.address.clone(),
            user_id: s.user_id.clone(),
            access_token: s.access_token.clone(),
        });
    }
}

pub fn ctx() -> Ctx {
    expect_context::<Ctx>()
}
