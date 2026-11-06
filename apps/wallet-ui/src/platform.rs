//! Platform-specific wording for passkeys and backups.

use crate::backend::BackupTarget;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Platform {
    Iphone,
    Mac,
    Windows,
    Android,
    Other,
}

impl Platform {
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "iphone" | "ios" => Platform::Iphone,
            "mac" | "macos" => Platform::Mac,
            "windows" | "win" => Platform::Windows,
            "android" => Platform::Android,
            "other" | "linux" => Platform::Other,
            _ => return None,
        })
    }

    pub fn detect() -> Self {
        let ua = web_sys::window().and_then(|w| w.navigator().user_agent().ok()).unwrap_or_default();
        if ua.contains("iPhone") || ua.contains("iPad") {
            Platform::Iphone
        } else if ua.contains("Android") {
            Platform::Android
        } else if ua.contains("Windows") {
            Platform::Windows
        } else if ua.contains("Mac OS X") || ua.contains("Macintosh") {
            Platform::Mac
        } else {
            Platform::Other
        }
    }

    /// What unlocks the passkey, as the user knows it.
    pub fn unlock_name(self) -> &'static str {
        match self {
            Platform::Iphone => "Face ID",
            Platform::Mac => "Touch ID",
            Platform::Windows => "Windows Hello",
            Platform::Android => "your fingerprint or screen lock",
            Platform::Other => "your device's screen lock",
        }
    }

    /// Short button label: "Use Face ID".
    pub fn create_label(self) -> String {
        match self {
            Platform::Android | Platform::Other => "Create passkey".into(),
            _ => format!("Use {}", self.unlock_name()),
        }
    }

    /// Where the passkey itself syncs.
    pub fn passkey_home(self) -> &'static str {
        match self {
            Platform::Iphone | Platform::Mac => "iCloud Keychain",
            Platform::Android => "Google Password Manager",
            Platform::Windows => "this Windows device",
            Platform::Other => "your password manager",
        }
    }

    /// One line on where the passkey lives.
    pub fn passkey_fact(self) -> String {
        match self {
            Platform::Windows => "Your passkey locks your wallet and stays on this Windows device.".into(),
            Platform::Other => "Your passkey locks your wallet and lives in your password manager.".into(),
            p => format!("Your passkey locks your wallet and syncs with {}.", p.passkey_home()),
        }
    }

    pub fn backup_target(self) -> BackupTarget {
        match self {
            Platform::Iphone | Platform::Mac => BackupTarget::ICloud,
            _ => BackupTarget::GoogleDrive,
        }
    }
}

pub fn backup_name(t: BackupTarget) -> &'static str {
    match t {
        BackupTarget::ICloud => "iCloud",
        BackupTarget::GoogleDrive => "Google Drive",
    }
}
