//! The Wirecatch desktop app on GPUI (D95, D99). It is built beside the Tauri app until it shows
//! every screen; for now it opens the main window on the download list.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod add_dialog;
mod backend;
mod library;
mod links;
mod main_window;
mod text;
mod theme;

use wirecatch_shell::{elevation, host};

/// Which lines the app logs unless the log filter variable chooses others (`wirecatch-logging`).
const LOG_FILTER: &str = "info";

fn main() {
    let logging = wirecatch_logging::start(wirecatch_product::APP_EXECUTABLE, LOG_FILTER);
    logging.write_to(host::log_folder(std::env::var_os(host::DATA_DIR_VAR)).as_deref());
    if elevation::decide(wirecatch_ipc::elevation()) == elevation::Start::Refuse {
        if host::DEV_BUILD {
            tracing::warn!("running as administrator; a release build would refuse to start");
        } else {
            elevation::explain_refusal();
            return;
        }
    }
    let backend = match backend::Backend::start() {
        Ok(backend) => backend,
        Err(e) => {
            tracing::error!("could not start the download manager's threads: {e}");
            return;
        }
    };
    gpui_kit::application()
        .with_assets(gpui_kit::assets::Assets)
        .run(move |cx| {
            gpui_kit::init(cx);
            main_window::bind_keys(cx);
            theme::init(cx);
            cx.set_global(backend);
            let library = library::Library::new(cx);
            if let Err(e) = main_window::MainWindow::open(library, cx) {
                tracing::error!("could not open the main window: {e}");
                cx.quit();
            }
        });
}
