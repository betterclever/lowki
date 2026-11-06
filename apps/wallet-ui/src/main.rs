//! Lowki wallet UI: the app that runs inside the Lowki iframe/popup.

mod app;
mod backend;
mod dev;
mod format;
mod platform;
mod protocol;
mod screens;
mod state;
mod ui;

use std::rc::Rc;

use leptos::prelude::*;
use wasm_bindgen::JsCast;

/// The embedding app's origin as the browser reports it (`location.ancestorOrigins[0]`),
/// when it does. The `origin=` flag is set by the embedding page, so with the real backend
/// the browser's answer wins: an app can't claim another app's origin (and account).
fn browser_parent_origin() -> Option<String> {
    let loc = web_sys::window()?.location();
    let list = js_sys::Reflect::get(&loc, &"ancestorOrigins".into()).ok()?;
    let list: web_sys::DomStringList = list.dyn_into().ok()?;
    list.item(0).filter(|o| o != "null" && !o.is_empty())
}

fn main() {
    console_error_panic_hook::set_once();
    let mut flags = dev::flags();
    // The only place the implementation is chosen. The UI only sees the traits.
    let mock = backend::mock::new_shared();
    dev::init(mock.clone(), &flags);
    if flags.mock {
        backend::install(mock as Rc<dyn backend::Backend>);
    } else {
        let origin = browser_parent_origin()
            .or_else(|| (flags.origin != "*").then(|| flags.origin.clone()))
            .or_else(|| web_sys::window().and_then(|w| w.location().origin().ok()))
            .unwrap_or_default();
        if flags.origin != "*" && flags.origin != origin {
            web_sys::console::warn_1(&format!("[lowki] origin flag {} ignored; embedded by {origin}", flags.origin).into());
        }
        flags.origin = origin.clone();
        flags.app.origin = origin;
        // Private send and delegation still run on the mock, behind the same traits.
        let real = backend::real::new_shared(mock);
        backend::install(real as Rc<dyn backend::Backend>);
    }
    mount_to_body(move || view! { <app::App flags=flags.clone() /> });
}
