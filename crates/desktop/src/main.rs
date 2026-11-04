//! The Wirecatch desktop app on GPUI (D95, D99). One copy runs per user: a second launch hands
//! its command line to it (D103).

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
use wirecatch_ipc::{IpcError, Listener};
use wirecatch_shell::launch::{self, Launch};
use wirecatch_shell::message_box::{self, Tone};
use wirecatch_shell::{autostart, bridge, elevation, host};

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

/// How many times a launch tries for the endpoint, when the copy holding it leaves meanwhile.
const CLAIM_TRIES: usize = 3;

/// What a launch does after looking for the endpoint.
enum Claim {
    /// Runs, serving browsers on the endpoint if it has it.
    Run(Option<Listener>),
    /// Another copy holds the endpoint, and this launch has been dealt with.
    Exit,
}

fn claim_or_hand_over(runtime: &tokio::runtime::Runtime, launch: &Launch) -> Claim {
    for _ in 0..CLAIM_TRIES {
        match runtime.block_on(async { bridge::bind() }) {
            Ok(listener) => return Claim::Run(Some(listener)),
            Err(IpcError::AlreadyInUse { endpoint }) => {
                tracing::info!("another copy holds {endpoint}; handing this launch to it");
                match runtime.block_on(launch::hand_over(launch)) {
                    Ok(()) => return Claim::Exit,
                    Err(e) if e.copy_has_gone() => {
                        tracing::info!("the copy holding {endpoint} has gone");
                    }
                    Err(e) => {
                        tracing::error!("could not hand this launch to the running copy: {e}");
                        break;
                    }
                }
            }
            Err(e) => {
                tracing::error!("browser downloads cannot reach Wirecatch: {e}");
                return Claim::Run(None);
            }
        }
    }
    launch::say_already_running();
    Claim::Exit
}

fn main() {
    let logging = wirecatch_logging::start(wirecatch_product::APP_EXECUTABLE, LOG_FILTER);
    logging.write_to(host::log_folder(std::env::var_os(host::DATA_DIR_VAR)).as_deref());
    let args = launch::command_line();
    // Before the elevation check: the uninstaller runs elevated.
    if args.iter().any(|a| a == autostart::REMOVE_ENTRY_FLAG) {
        if let Err(e) = autostart::remove(host::CHANNEL.name()) {
            tracing::warn!("could not remove the start-at-sign-in entry: {e}");
        }
        return;
    }
    if elevation::decide(wirecatch_ipc::elevation()) == elevation::Start::Refuse {
        if host::DEV_BUILD {
            tracing::warn!("running as administrator; a release build would refuse to start");
        } else {
            elevation::explain_refusal();
            return;
        }
    }
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
    let Claim::Run(listener) = claim_or_hand_over(&runtime, &launch) else {
        return;
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
            theme::follow(&library, cx);
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
