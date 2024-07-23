use slint::ComponentHandle;
use tracing::{error, info};

use crate::structs::AppState;
use crate::MainWindow;

use crate::UpdateCheck;

pub fn helper(ui: &MainWindow, app_state: &mut AppState) {
    info!("Checking for updates...");
        app_state.add_to_self_update_log("Checking for updates...", ui);

    let current_version = env!("CARGO_PKG_VERSION");
    app_state.add_to_self_update_log(&format!("Current version 2: v{current_version}"), ui);

    let status = match self_update::backends::github::Update::configure()
        .repo_owner("ChaseCares")
        .repo_name("link_rustler")
        .bin_name("link_rustler")
        .bin_path_in_archive("{{ bin }}-{{ version }}-{{ target }}/{{ bin }}")
        .show_download_progress(true)
        .current_version(current_version)
        .no_confirm(true)
        .build()
    {
        Ok(status) => status,
        Err(e) => {
            error!("Error configuring update: {e}");
            app_state.add_to_self_update_log(&format!("Error configuring update: {e}"), ui);
            ui.global::<UpdateCheck>()
                .set_actively_checking_for_update(false);
            return;
        }
    };

    let latest = match status.get_latest_release() {
        Ok(latest) => latest,
        Err(e) => {
            error!("Error fetching latest release: {e}");
            app_state.add_to_self_update_log(&format!("Error fetching latest release: {e}"), ui);
            ui.global::<UpdateCheck>()
                .set_actively_checking_for_update(false);
            return;
        }
    };

    match self_update::version::bump_is_greater(current_version, &latest.version) {
        Ok(true) => {
            info!(
                "New update available: v{}, current version: v{}",
                latest.version, current_version
            );
            if !app_state.self_update_complete {
                app_state.add_to_self_update_log(
                    &format!("New update available: v{}", latest.version),
                    ui,
                );
            }

            if app_state.self_update_complete {
                match status.update() {
                    Ok(_) => {
                        info!("Update successful! Restart the application to apply the update.");
                        app_state.add_to_self_update_log(
                            "Update successful! Restart the application to apply the update.",
                            ui,
                        );
                        ui.global::<UpdateCheck>()
                            .set_self_update_button_text("Up to date".into());
                    }
                    Err(e) => {
                        error!("Error updating: {e}");
                        app_state.add_to_self_update_log(&format!("Error updating: {e}"), ui);
                    }
                }
            } else {
                ui.global::<UpdateCheck>()
                    .set_self_update_button_text(format!("Update to v{}", latest.version).into());
                info!("app_state.self_update: {}", app_state.self_update_complete);
                app_state.self_update_complete = true;
            }
        }
        Ok(false) if current_version == latest.version => {
            info!("You are already using the latest version.");
            app_state.add_to_self_update_log("You are already using the latest version.", ui);
            ui.global::<UpdateCheck>()
                .set_self_update_button_text("Up to date".into());
        }
        Ok(false) => {
            info!("You are using a newer version than the latest.");
            app_state.add_to_self_update_log("You are using a newer version than the latest.", ui);
        }
        Err(e) => {
            error!("Error comparing versions: {e}");
            app_state.add_to_self_update_log(&format!("Error comparing versions: {e}"), ui);
        }
    }

    info!("Checking for updates complete.");
    ui.global::<UpdateCheck>()
        .set_actively_checking_for_update(false);
}
