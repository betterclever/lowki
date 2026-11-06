//! Inline SVG icons on the Lowki grid: 20 px, 1.75 px stroke, round caps and
//! joins, `currentColor`. Glyphs that appear in `design/index.html` (check,
//! minus, copy, keyhole, passkey, mail, G, shield) are copied from the spec.
//! The keyhole and gourd glyphs are reserved for privacy states.

use leptos::prelude::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ic {
    Back,
    Close,
    Copy,
    Check,
    Minus,
    /// Privacy only: shielded balance, private send, sealed key.
    Keyhole,
    Passkey,
    Mail,
    Google,
    /// Verified signer.
    Shield,
    ArrowUpRight,
    ArrowDownLeft,
    Sliders,
    Alert,
    CloudOff,
    MoveIn,
    MoveOut,
    Clock,
    Not,
    Grid,
    Refresh,
    Upload,
    Eye,
    Info,
    Exit,
    /// A name (`@`).
    At,
    /// Opens in a new tab.
    External,
}

fn paths(i: Ic) -> &'static [&'static str] {
    match i {
        Ic::Back => &["M12.5 4.5 7 10l5.5 5.5"],
        Ic::Close => &["M5.5 5.5l9 9M14.5 5.5l-9 9"],
        Ic::Copy => &[
            "M9 7h5a2 2 0 0 1 2 2v5a2 2 0 0 1-2 2H9a2 2 0 0 1-2-2V9a2 2 0 0 1 2-2Z",
            "M13 7V5.5A1.5 1.5 0 0 0 11.5 4h-6A1.5 1.5 0 0 0 4 5.5v6A1.5 1.5 0 0 0 5.5 13H7",
        ],
        Ic::Check => &["M4.5 10.5l3.5 3.5 7.5-8"],
        Ic::Minus => &["M5 10h10"],
        Ic::Keyhole => &["M7 8.5a3 3 0 1 0 6 0 3 3 0 1 0-6 0", "M8.6 11.2l-1 5h4.8l-1-5"],
        Ic::Passkey => &[
            "M4.8 7a3.2 3.2 0 1 0 6.4 0 3.2 3.2 0 1 0-6.4 0",
            "M2.8 16.5c.6-2.8 2.7-4.4 5.2-4.4 1.3 0 2.4.4 3.3 1.1",
            "M14.5 10.5v6M14.5 13.5h2M14.5 15.5h1.5",
            "M12.9 9.5a1.6 1.6 0 1 0 3.2 0 1.6 1.6 0 1 0-3.2 0",
        ],
        Ic::Mail => &["M5 5h10a2 2 0 0 1 2 2v6.5a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V7a2 2 0 0 1 2-2Z", "M3.5 6l6.5 5 6.5-5"],
        Ic::Google => &[
            "M2.75 10a7.25 7.25 0 1 0 14.5 0 7.25 7.25 0 1 0-14.5 0",
            "M13.5 9.6H10v1.8h1.7c-.3.8-1 1.3-1.7 1.3a2.7 2.7 0 1 1 1.8-4.7",
        ],
        Ic::Shield => &["M10 2.8l5.8 2.2v4.4c0 3.6-2.5 6.3-5.8 7.8-3.3-1.5-5.8-4.2-5.8-7.8V5z", "M7.4 10l1.8 1.8 3.5-3.6"],
        Ic::ArrowUpRight => &["M6 14l8-8M7.5 6H14v6.5"],
        Ic::ArrowDownLeft => &["M14 6l-8 8M12.5 14H6V7.5"],
        Ic::Sliders => &[
            "M3.5 6.5h7M14.5 6.5h2M3.5 13.5h2M9.5 13.5h7",
            "M10.5 6.5a2 2 0 1 0 4 0 2 2 0 1 0-4 0",
            "M5.5 13.5a2 2 0 1 0 4 0 2 2 0 1 0-4 0",
        ],
        Ic::Alert => &["M10 3.5l7 12.5H3z", "M10 8.5v3.2M10 14v.1"],
        Ic::CloudOff => &["M3 3l14 14", "M7.6 5.3A4.5 4.5 0 0 1 14.2 8a3.3 3.3 0 0 1 1.9 5.9M13 15H6a3.3 3.3 0 0 1-.8-6.5"],
        Ic::MoveIn => &["M10 3.5v8M6.8 8.5 10 11.7l3.2-3.2", "M4 16h12"],
        Ic::MoveOut => &["M10 12V4M6.8 7.2 10 4l3.2 3.2", "M4 16h12"],
        Ic::Clock => &["M3 10a7 7 0 1 0 14 0 7 7 0 1 0-14 0", "M10 6.5V10l2.5 1.5"],
        Ic::Not => &["M3 10a7 7 0 1 0 14 0 7 7 0 1 0-14 0", "M5.1 5.1l9.8 9.8"],
        Ic::Grid => &["M4 4h4.5v4.5H4ZM11.5 4H16v4.5h-4.5ZM4 11.5h4.5V16H4ZM11.5 11.5H16V16h-4.5Z"],
        Ic::Refresh => &["M16 10a6 6 0 1 1-1.8-4.3", "M16 4v3.5h-3.5"],
        Ic::Upload => &["M10 12.5V3.5M6.8 6.7 10 3.5l3.2 3.2", "M4 12v3.5a1 1 0 0 0 1 1h10a1 1 0 0 0 1-1V12"],
        Ic::Eye => &["M2 10s3-5.5 8-5.5S18 10 18 10s-3 5.5-8 5.5S2 10 2 10Z", "M8 10a2 2 0 1 0 4 0 2 2 0 1 0-4 0"],
        Ic::Info => &["M3 10a7 7 0 1 0 14 0 7 7 0 1 0-14 0", "M10 9v4.5M10 6.6v.1"],
        Ic::At => &["M13.2 10a3.2 3.2 0 1 1-6.4 0 3.2 3.2 0 0 1 6.4 0Z", "M13.2 10v1.3a2.2 2.2 0 0 0 4.3.6A7.5 7.5 0 1 0 14 16.4"],
        Ic::External => &["M11.5 4H16v4.5", "M16 4l-7 7", "M14 11.5V15a1 1 0 0 1-1 1H5a1 1 0 0 1-1-1V7a1 1 0 0 1 1-1h3.5"],
        Ic::Exit => &["M11.5 3.5H5a1 1 0 0 0-1 1v11a1 1 0 0 0 1 1h6.5", "M9 10h8M14 7l3 3-3 3"],
    }
}

/// Decorative by default (`aria-hidden`). Give the button an aria-label.
#[component]
pub fn Icon(i: Ic, #[prop(optional)] size: Option<u32>) -> impl IntoView {
    let s = size.unwrap_or(20);
    view! {
        <svg class="ic" width=s height=s viewBox="0 0 20 20" fill="none" stroke="currentColor"
            stroke-width="1.75" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true" focusable="false">
            {paths(i).iter().map(|d| view! { <path d=*d /> }).collect_view()}
        </svg>
    }
}

pub use super::gourd::{GOURD, KEYHOLE, KEYHOLE_TRANSFORM, VIEWBOX as GOURD_VIEWBOX};

/// One-colour gourd with the keyhole cut out (privacy states only).
#[component]
pub fn GourdGlyph(#[prop(optional)] size: Option<u32>) -> impl IntoView {
    let s = size.unwrap_or(20);
    view! {
        <svg class="ic gourd" width=s height=s viewBox=GOURD_VIEWBOX aria-hidden="true" focusable="false">
            <path fill="currentColor" d=GOURD />
            <path class="gourd-glyph-key" d=KEYHOLE transform=KEYHOLE_TRANSFORM />
        </svg>
    }
}

/// The Lowki lockup, swapped for the dark version by theme (CSS).
#[component]
pub fn Lockup() -> impl IntoView {
    view! {
        <span class="lockup" role="img" aria-label="Lowki">
            <img class="lockup-light" src="lowki-lockup.svg" alt="" height="32" />
            <img class="lockup-dark" src="lowki-lockup-dark.svg" alt="" height="32" />
        </span>
    }
}
