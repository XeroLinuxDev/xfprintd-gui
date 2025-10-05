use gtk4::prelude::*;
use gtk4::Application;
use log::info;

mod context;
mod fingerprints;
mod fprintd;
mod pam_helper;
mod pam_switch;
mod system;
mod ui;
mod util;

fn main() {
    // Initialize logger
    simple_logger::SimpleLogger::new().init().unwrap();

    info!(
        "Starting XFPrintD GUI Application v{}",
        env!("CARGO_PKG_VERSION")
    );
    info!("Application ID: xyz.xerolinux.xfprintd_gui");

    let app = Application::builder()
        .application_id("xyz.xerolinux.xfprintd_gui")
        .build();

    app.connect_activate(ui::setup_application_ui);

    app.run();
}
