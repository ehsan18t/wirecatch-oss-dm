//! The Wirecatch desktop app: one process that hosts the download manager (`wirecatch-core`),
//! serves the browser bridge, sits in the tray, and shows the interface when asked (D13).
//!
//! # Commands and events
//!
//! The interface calls the commands declared in `wirecatch_protocol::desktop`, each with its
//! arguments in one `args` parameter; `commands.rs` registers exactly those, and the interface
//! calls them through the `CommandMap` generated from the same declarations. Every command fails
//! with a `ProtocolError`: a code the interface acts on (never the wording) and a plain-language
//! message, the core's own error text or the shell's. Every command reaches the real core; none
//! is pending. The core checks for add-on updates by itself (once a day, while
//! `auto_update_addons` is on), so the desktop never schedules it.
//!
//! A window calls only the commands its capability in `capabilities/` grants: the build gives
//! every command of the contract its own permission. A download window gets only those it uses,
//! and acts only on what it shows (D43, see [`download_windows`]).
//!
//! - [`UI_EVENT`] carries every core `UiEvent`, in order.
//! - [`RESYNC_EVENT`] (no payload): events were missed; reload everything.
//! - [`LINKS_EVENT`] (no payload): links arrived from a second launch; fetch them with
//!   `take_launch_links`.
//! - `WINDOW_FRAME_EVENT`, to one window: the state its title bar shows changed (see [`frame`]).
//! - `SYSTEM_APPEARANCE_EVENT`: the system's accent colour changed (see [`appearance`]).
//!
//! Every event reaches every window: the main window and the download windows (D23, see
//! [`download_windows`]), which each keep only what concerns their download.
//!
//! # Starting
//!
//! - A second launch never starts a second app: it focuses the running one (opening its window
//!   if it was closed to the tray) and passes any links on its command line to the Add dialog.
//!   With `--background` and no links it does nothing visible.
//! - `--background` (the bridge and start-with-system pass it) starts with no window.
//! - On every start the app runs `wirecatch-bridge install` from its own folder (D19).
//! - Debug builds are a separate app, "Wirecatch Dev" ([`host::DEV_BUILD`]): their own
//!   identifier, data folder and endpoint, and they never register the bridge, so they run
//!   beside an installed Wirecatch without touching it.
//!
//! # Updates
//!
//! `tauri-plugin-updater` is compiled in but only switched on when `tauri.conf.json` has a
//! `plugins.updater` section (endpoints and the public signing key). Then [`updates`] checks a
//! minute after start and daily, downloads a verified update in the background and installs it
//! when the person quits. See `apps/desktop/PACKAGING.md`.

pub mod activity;
pub mod api;
pub mod appearance;
pub mod appimage;
pub mod autostart;
pub mod background;
pub mod bridge;
mod commands;
pub mod download_windows;
pub mod elevation;
mod files;
pub mod finished_window;
pub mod frame;
pub mod host;
pub mod lifecycle;
#[cfg(windows)]
mod maximize_overlay;
pub mod notices;
pub mod questions;
pub mod sleep;
pub mod system_notices;
pub mod tray;
pub mod updates;
pub mod window;

use tauri::Manager as _;
use tauri_plugin_dialog::{DialogExt as _, MessageDialogKind};
use wirecatch_core::ManagerConfig;
use wirecatch_ipc::IpcError;
use wirecatch_product::Channel;

pub use commands::handler;
pub use window::{BACKGROUND_FLAG, wants_background};
pub use wirecatch_protocol::desktop::{LINKS_EVENT, RESYNC_EVENT, UI_EVENT};

use crate::host::Core;
use crate::lifecycle::Lifecycle;
use crate::window::{Launch, LaunchLinks};

/// Shown by a copy of Wirecatch that finds another one already serving this user.
pub const ALREADY_RUNNING: &str = "Wirecatch is already running for you, so this copy will close. Look for its icon in the tray, or open it from the Start menu again.";

/// The start-at-sign-in entry's name for this build (the `Run` registry value on Windows): its
/// channel's name. The NSIS uninstall hook removes the release one by the same name.
#[must_use]
pub fn autostart_name() -> &'static str {
    host::CHANNEL.name()
}

/// Which lines the app logs unless the log filter variable chooses others (`wirecatch-logging`).
const LOG_FILTER: &str = "info";

/// Starts the app.
pub fn run() {
    // A release build on Windows has no stderr, so the file is where lines are read: it is known before anything can fail, so a start that fails early leaves its reason there.
    let logging = wirecatch_logging::start(wirecatch_product::APP_EXECUTABLE, LOG_FILTER);
    logging.write_to(host::log_folder(std::env::var_os(host::DATA_DIR_VAR)).as_deref());
    if elevation::decide(wirecatch_ipc::elevation()) == elevation::Start::Refuse {
        // Developers often work in an elevated terminal, and a debug build has its own endpoint, still labelled at its integrity level.
        if host::DEV_BUILD {
            tracing::warn!("running as administrator; a release build would refuse to start");
        } else {
            elevation::explain_refusal();
            return;
        }
    }
    let args: Vec<String> = std::env::args().collect();
    let launch = Launch::from_args(&args);

    let mut context = tauri::generate_context!();
    if host::DEV_BUILD {
        dev_identity(context.config_mut());
    }
    // The name the interface shows (`getName`), so a debug build calls itself what its window does.
    host::CHANNEL
        .name()
        .clone_into(&mut context.package_info_mut().name);

    let app = with_plugins(tauri::Builder::default())
        .manage(LaunchLinks::default())
        .manage(window::PendingSelection::default())
        .manage(finished_window::FinishedWindow::default())
        .manage(Lifecycle::default())
        .manage(sleep::MainAsleep::default())
        .manage(download_windows::DownloadWindows::default())
        .manage(api::WarningsShown::default())
        .manage(frame::Frames::default())
        .manage(appearance::SystemAccent::read())
        .manage(appearance::AppliedTheme::default())
        .on_page_load(|webview, payload| {
            if payload.event() != tauri::webview::PageLoadEvent::Started {
                return;
            }
            // A new page (a reload, or recovering from a crash) reports its own button and
            // takes the selection it missed.
            if let Some(window) = webview.app_handle().get_webview_window(webview.label()) {
                frame::page_loading(&window);
            }
            if webview.label() == window::MAIN_WINDOW
                && let Some(selection) = webview.try_state::<window::PendingSelection>()
            {
                selection.page_gone();
            }
        })
        .setup(move |app| setup(app, &launch))
        .invoke_handler(commands::handler())
        .build(context);
    match app {
        Ok(app) => app.run(lifecycle::on_run_event),
        Err(err) => {
            tracing::error!("Wirecatch could not start: {err}");
            std::process::exit(1);
        }
    }
}

/// The plugins the app runs with.
fn with_plugins(builder: tauri::Builder<tauri::Wry>) -> tauri::Builder<tauri::Wry> {
    builder
        // First, so a second launch hands over its command line and exits before anything else.
        .plugin(tauri_plugin_single_instance::init(|app, argv, _cwd| {
            // A launch while quitting starts the app again once this one has exited, instead
            // of flashing a window that is about to close.
            let quitting = app
                .try_state::<Lifecycle>()
                .is_some_and(|life| life.relaunch_later(&argv));
            if !quitting {
                window::handle_launch(app, &Launch::from_args(&argv));
            }
        }))
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(
            tauri_plugin_autostart::Builder::new()
                .args([BACKGROUND_FLAG])
                .app_name(autostart_name())
                .build(),
        )
}

fn setup(app: &mut tauri::App, launch: &Launch) -> Result<(), Box<dyn std::error::Error>> {
    let handle = app.handle().clone();
    app.manage(system_notices::SystemNotices::new(
        &handle,
        host::CHANNEL.name(),
        &app.config().identifier,
    ));

    // Claim the browser endpoint before opening the database: if another Wirecatch already
    // serves this user, a second manager on the same download list must not start.
    let listener = match tauri::async_runtime::block_on(async { bridge::bind() }) {
        Ok(listener) => Some(listener),
        Err(IpcError::AlreadyInUse { endpoint }) => {
            tracing::error!(
                "another copy of Wirecatch is already running (it holds {endpoint}); this one exits"
            );
            // This copy opens nothing else.
            say_already_running(&handle);
            return Ok(());
        }
        Err(e) => {
            tracing::error!("browser downloads cannot reach Wirecatch: {e}");
            None
        }
    };

    if updater_configured(app) {
        handle.plugin(tauri_plugin_updater::Builder::new().build())?;
        app.manage(updates::PendingUpdate::default());
        let checking = tauri::async_runtime::spawn(updates::keep_checking(handle.clone()));
        app.state::<Lifecycle>().keep(checking);
    }

    // Moves a data folder an earlier version kept in the roaming profile, before anything opens it.
    let data_dir = host::data_dir(&app.path().local_data_dir()?, &app.path().data_dir()?);
    let home = app.path().home_dir().unwrap_or_else(|_| data_dir.clone());
    let config = ManagerConfig::new(data_dir.clone(), host::known_folders(&home))
        .with_official_suffix_list();
    app.manage(Core::new(config));

    if let Err(e) = tray::create(&handle) {
        // Some Linux desktops have no tray. Downloads matter more than the icon: keep going,
        // and let closing the window quit, since there would be no way back to it.
        tracing::warn!("could not put Wirecatch in the tray ({e}); closing the window will quit");
        app.state::<Lifecycle>().without_tray();
    }
    let starting = handle.clone();
    let task = tauri::async_runtime::spawn(async move { start(starting, listener).await });
    app.state::<Lifecycle>().keep(task);

    window::handle_launch(&handle, launch);

    if bridge::registers_on_start() {
        #[cfg(unix)]
        let private_dir = data_dir.join("bin");
        bridge::register_in_background(move || {
            let app_exe = std::env::current_exe()
                .map_err(|e| tracing::warn!("could not find the app's own file: {e}"))
                .ok()?;
            // Under an AppImage the bundled bridge sits in a mount that goes away when the app
            // exits, so a private copy is registered instead.
            #[cfg(unix)]
            let chosen = match std::env::var_os(appimage::APPIMAGE_VAR) {
                Some(appimage) => bridge::bundled_bridge(&app_exe).and_then(|bundled| {
                    appimage::install_private_bridge(
                        &bundled,
                        std::path::Path::new(&appimage),
                        &private_dir,
                    )
                }),
                None => bridge::bridge_to_register(&app_exe),
            };
            #[cfg(not(unix))]
            let chosen = bridge::bridge_to_register(&app_exe);
            chosen.map_err(|e| tracing::warn!("{e}")).ok()
        });
    } else {
        tracing::info!("debug build: not registering the browser bridge");
    }
    Ok(())
}

/// Says that another copy of Wirecatch is already running instead of vanishing, and exits once the person closes the message. The message shows once the event loop runs.
fn say_already_running(handle: &tauri::AppHandle) {
    let closing = handle.clone();
    handle
        .dialog()
        .message(ALREADY_RUNNING)
        .title(host::CHANNEL.name())
        .kind(MessageDialogKind::Info)
        .show(move |_| closing.exit(0));
}

/// Opens the manager, then starts the work that runs alongside it.
async fn start(app: tauri::AppHandle, listener: Option<wirecatch_ipc::Listener>) {
    let core = app.state::<Core>();
    let manager = match core.opened().await {
        Ok(manager) => manager.clone(),
        // A copy that got past the endpoint (an unelevated one beside an elevated one, whose pipe's label keeps its probe out) meets the list's lock instead (D41.8).
        Err(unopened) if unopened.another_copy => {
            tracing::error!("{}; this copy exits", unopened.reason);
            drop(listener);
            say_already_running(&app);
            return;
        }
        Err(unopened) => {
            unavailable(&app, listener, unopened.reason.message.clone());
            return;
        }
    };
    for notice in manager.startup_notices() {
        tracing::warn!("{}: {}", notice.title, notice.message);
        background::show_notification(&app, notice.title.clone(), notice.message.clone());
    }
    let life = app.state::<Lifecycle>();
    // Subscribed before looking at open questions, so none falls between the two.
    let events = manager.subscribe();
    life.keep(tauri::async_runtime::spawn(background::forward(
        app.clone(),
        events,
    )));
    // A window built while the download list was opening has the system's theme, not the
    // stored one (the main window also takes it when its page has drawn, in case it was only
    // queued then).
    appearance::follow_theme(&app, manager.get_settings().theme);
    // A browser download that arrived while the download list was opening gets its window.
    download_windows::restore(&app);
    life.keep(tauri::async_runtime::spawn(background::watch(
        app.clone(),
        manager.clone(),
    )));
    if let Some(listener) = listener {
        life.keep(tauri::async_runtime::spawn(bridge::serve(
            listener, manager,
        )));
    }
}

/// Gives a debug build its own identity, so it runs beside an installed Wirecatch: another
/// identifier (the single-instance guard keys on it) and a title that says which one it is.
pub fn dev_identity(config: &mut tauri::Config) {
    let dev = Channel::Dev;
    dev.identifier().clone_into(&mut config.identifier);
    config.product_name = Some(dev.name().to_owned());
    for window in &mut config.app.windows {
        dev.name().clone_into(&mut window.title);
    }
}

/// The manager could not open (for example a database from a newer Wirecatch, or a data folder
/// that cannot be written). The app keeps running rather than quitting: quitting would leave the
/// person with nothing to read, and the bridge would start it again on the next browser
/// download. Instead the reason is shown in a notification and in the tray tooltip, the window
/// shows it on its first command, and the extension gets it as a clear error on every request.
fn unavailable(app: &tauri::AppHandle, listener: Option<wirecatch_ipc::Listener>, reason: String) {
    background::show_notification(app, unavailable_title(), reason.clone());
    tray::set_tooltip(app, &tray::problem_tooltip(&reason));
    if let Some(listener) = listener {
        app.state::<Lifecycle>()
            .keep(tauri::async_runtime::spawn(bridge::serve_unavailable(
                listener, reason,
            )));
    }
}

/// The notification title when the manager could not open, naming the running app.
fn unavailable_title() -> String {
    format!("{} could not open its download list", host::CHANNEL.name())
}

/// The updater stays off until `plugins.updater` is configured with endpoints and a public key
/// (D19: that needs the GitHub organization and a signing key first).
fn updater_configured(app: &tauri::App) -> bool {
    app.config().plugins.0.get("updater").is_some_and(|c| {
        c.get("pubkey")
            .is_some_and(|k| k.as_str().is_some_and(|k| !k.is_empty()))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use wirecatch_product::BRIDGE_EXECUTABLE;

    /// The NSIS uninstall hook cannot run in a test, so this checks it keeps its promises.
    #[test]
    fn the_uninstall_hook_spares_updates_and_removes_start_at_sign_in() {
        let hook = include_str!("../windows/hooks.nsh");
        let guarded = hook
            .split("${If} $UpdateMode <> 1")
            .nth(1)
            .expect("the hook must skip update mode");
        // The bridge as the Windows installer ships it, whatever system the test runs on.
        assert!(guarded.contains(&format!("{BRIDGE_EXECUTABLE}.exe\" uninstall")));
        assert!(guarded.contains("Pop $0"), "the nsExec result is popped");
        let run_value = format!(r#"CurrentVersion\Run" "{}""#, Channel::Release.name());
        assert!(guarded.contains(&run_value), "removes the Run value");
    }

    /// Every window's interface asks for the running app's name (`getName`), which Tauri refuses
    /// without this permission; the interface would then fall back to the release name.
    #[test]
    fn every_capability_lets_its_windows_read_the_app_name() {
        let folder = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("capabilities");
        let mut checked = 0;
        for entry in std::fs::read_dir(&folder).unwrap() {
            let path = entry.unwrap().path();
            let capability: serde_json::Value =
                serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
            let permissions = capability["permissions"].as_array().unwrap();
            assert!(
                permissions.contains(&serde_json::json!("core:app:allow-name")),
                "{} does not grant core:app:allow-name",
                path.display()
            );
            checked += 1;
        }
        assert!(checked >= 2, "the main and download window capabilities");
    }

    /// The pages are the app's own and load nothing else, so the policy closes every way an
    /// injected tag could reach further: no other scripts, plugins, frames, base address or form
    /// target; and frozen prototypes, so an injected script cannot pollute what the IPC bridge
    /// reads (DRS-18).
    #[test]
    fn the_content_security_policy_closes_what_the_pages_do_not_use() {
        let config: serde_json::Value =
            serde_json::from_str(include_str!("../tauri.conf.json")).unwrap();
        let security = &config["app"]["security"];
        assert_eq!(security["freezePrototype"], serde_json::json!(true));
        let csp = security["csp"].as_str().expect("a content security policy");
        let directives: std::collections::HashMap<&str, Vec<&str>> = csp
            .split(';')
            .filter_map(|directive| {
                let mut words = directive.split_whitespace();
                Some((words.next()?, words.collect()))
            })
            .collect();
        for (name, sources) in [
            ("default-src", vec!["'self'"]),
            ("script-src", vec!["'self'"]),
            ("object-src", vec!["'none'"]),
            ("base-uri", vec!["'none'"]),
            ("form-action", vec!["'none'"]),
            ("frame-ancestors", vec!["'none'"]),
        ] {
            assert_eq!(directives.get(name), Some(&sources), "{name} in {csp}");
        }
        let bundle: serde_json::Value =
            serde_json::from_str(include_str!("../tauri.bundle.conf.json")).unwrap();
        assert!(
            bundle["app"]["security"].is_null(),
            "packaging builds keep the same policy"
        );
    }

    #[test]
    fn the_unavailable_notice_names_the_running_app() {
        assert_eq!(
            unavailable_title(),
            format!("{} could not open its download list", host::CHANNEL.name())
        );
    }

    #[test]
    fn the_already_running_message_is_plain() {
        assert!(ALREADY_RUNNING.contains("already running"));
        assert!(!ALREADY_RUNNING.contains('\u{2014}'));
    }

    #[test]
    fn a_debug_build_gets_its_own_identity() {
        let release = Channel::Release;
        let mut config: tauri::Config = serde_json::from_value(serde_json::json!({
            "identifier": release.identifier(),
            "app": { "windows": [{ "label": "main", "title": release.name() }] }
        }))
        .unwrap();
        dev_identity(&mut config);
        assert_eq!(config.identifier, Channel::Dev.identifier());
        assert_ne!(config.identifier, release.identifier());
        assert_eq!(config.app.windows[0].title, Channel::Dev.name());
    }
}
