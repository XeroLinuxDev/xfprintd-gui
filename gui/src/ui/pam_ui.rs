//! PAM authentication switches UI functionality.

use crate::pam::{helper::PamHelper, switch as pam_switch};
use crate::ui::app::AppContext;
use log::info;

/// Set up PAM authentication switches.
pub fn setup_pam_switches(ctx: &AppContext) {
    info!("Checking current PAM configurations for switches initialization");

    let (login_configured, sudo_configured, polkit_configured) =
        PamHelper::check_all_configurations();

    info!(
        "PAM Login Authentication: {}",
        if login_configured {
            "ENABLED"
        } else {
            "DISABLED"
        }
    );
    info!(
        "PAM Sudo Authentication: {}",
        if sudo_configured {
            "ENABLED"
        } else {
            "DISABLED"
        }
    );
    info!(
        "PAM Polkit Authentication: {}",
        if polkit_configured {
            "ENABLED"
        } else {
            "DISABLED"
        }
    );

    ctx.fingerprint_ctx
        .ui
        .switches
        .login
        .set_active(login_configured);
    ctx.fingerprint_ctx
        .ui
        .switches
        .term
        .set_active(sudo_configured);
    ctx.fingerprint_ctx
        .ui
        .switches
        .prompt
        .set_active(polkit_configured);

    info!("Temporarily disabling PAM switches until fingerprint enrollment check");
    ctx.fingerprint_ctx.set_pam_switches_sensitive(false);

    setup_pam_switch_handlers(ctx);
}

/// Set up PAM switch event handlers using generic implementation.
fn setup_pam_switch_handlers(ctx: &AppContext) {
    pam_switch::setup_pam_switch(
        &ctx.fingerprint_ctx.ui.switches.login,
        pam_switch::services::login(),
    );

    pam_switch::setup_pam_switch(
        &ctx.fingerprint_ctx.ui.switches.term,
        pam_switch::services::SUDO,
    );

    pam_switch::setup_pam_switch(
        &ctx.fingerprint_ctx.ui.switches.prompt,
        pam_switch::services::POLKIT,
    );
}
