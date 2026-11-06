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

fn main() {
    console_error_panic_hook::set_once();
    let flags = dev::flags();
    // The only place the implementation is chosen. Swap MockBackend for the
    // real worker/server client here; the UI only sees the traits.
    let mock = backend::mock::new_shared();
    dev::init(mock.clone(), &flags);
    backend::install(mock as Rc<dyn backend::Backend>);
    mount_to_body(move || view! { <app::App flags=flags.clone() /> });
}
