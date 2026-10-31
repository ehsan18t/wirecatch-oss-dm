//! The Wirecatch desktop app on GPUI (D95, D99). It is built beside the Tauri app until it shows
//! every screen. One copy runs per user: a second launch hands its command line to it (D103).

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod add_dialog;
mod app_update;
mod backend;
mod closing;
mod download_windows;
mod files;
mod library;
mod lifecycle;
mod links;
mod main_window;
mod notices;
mod sign_in;
mod text;
mod theme;
mod tray;
mod ui;

use gpui_kit::component::TitleBar;
use gpui_kit::{QuitMode, WindowOptions};
use wirecatch_ipc::IpcError;
use wirecatch_shell::launch::{self, Launch};
use wirecatch_shell::message_box::{self, Tone};
use wirecatch_shell::{bridge, elevation, host};

use crate::library::Asked;

/// Says why the app is closing, in a system message box: it has no window to say it in.
pub fn say_before_exit(text: &str) {
    message_box::show(Tone::Warning, host::CHANNEL.name(), text);
}

/// What every window opens with: the app's own title bar, and the app id that Linux desktops
/// match to its `.desktop` file for the icon and the taskbar entry.
#[must_use]
pub fn window_options() -> WindowOptions {
    WindowOptions {
        app_id: Some(host::CHANNEL.identifier().to_owned()),
        ..TitleBar::window_options()
    }
}

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
    let args: Vec<String> = std::env::args().collect();
    let launch = Launch::from_args(&args);
    let runtime = match backend::Backend::runtime() {
        Ok(runtime) => runtime,
        Err(e) => {
            tracing::error!("could not start the download manager's threads: {e}");
            say_before_exit(&format!(
                "{} could not start because the system would not give it the threads it needs ({e}). Restart your computer and try again.",
                host::CHANNEL.name()
            ));
            return;
        }
    };
    // Claimed before the database opens: a second manager on the same list must not start.
    let listener = match runtime.block_on(async { bridge::bind() }) {
        Ok(listener) => Some(listener),
        Err(IpcError::AlreadyInUse { endpoint }) => {
            tracing::info!("another copy holds {endpoint}; handing this launch to it");
            if let Err(e) = runtime.block_on(launch::hand_over(&launch)) {
                tracing::error!("could not hand this launch to the running copy: {e}");
                launch::say_already_running();
            }
            return;
        }
        Err(e) => {
            tracing::error!("browser downloads cannot reach Wirecatch: {e}");
            None
        }
    };
    let backend = backend::Backend::start(runtime, listener);
    gpui_kit::application()
        .with_assets(ui::icon::Assets)
        .with_quit_mode(QuitMode::Explicit)
        .run(move |cx| {
            gpui_kit::init(cx);
            main_window::bind_keys(cx);
            theme::init(cx);
            cx.set_global(backend);
            let library = library::Library::new(cx);
            tray::start(&library, cx);
            notices::start(cx);
            download_windows::init(&library, cx);
            sign_in::start(cx);
            app_update::start(cx);
            main_window::run(library, cx);
            if launch.shows_window() {
                let asked = if launch.links.is_empty() {
                    Asked::Window
                } else {
                    Asked::Links(launch.links.clone())
                };
                main_window::bring_up(&asked, cx);
            }
        });
}
