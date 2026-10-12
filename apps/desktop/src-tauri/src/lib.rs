//! The Wirecatch desktop app: one process that hosts the download manager (`wirecatch-core`),
//! serves the browser bridge, sits in the tray, and shows the interface when asked (D13).
//!
//! # Command contract
//!
//! The interface (`apps/desktop/src/lib/backend/tauri.ts`) calls exactly these commands with
//! these parameter names (the webview sends the camelCase form, for example `deleteFile` for
//! `delete_file`; Tauri maps it). A test on the TypeScript side checks that both lists agree.
//! Every command fails with a `ProtocolError`: a code the interface acts on (never the wording)
//! and a plain-language message, the core's own error text or the shell's.
//!
//! | Command | Parameters | Returns |
//! |---|---|---|
//! | `list_downloads` | none | `Vec<DownloadSummary>`, in queue order |
//! | `add_download` | `request: AddRequest` | `DownloadId` |
//! | `pause` | `id: DownloadId` | nothing |
//! | `resume` | `id: DownloadId` | nothing |
//! | `cancel` | `id: DownloadId` | nothing |
//! | `remove` | `ids: Vec<DownloadId>`, `delete_file: bool` | nothing |
//! | `retry` | `id: DownloadId` | nothing |
//! | `refresh_link` | `id: DownloadId`, `url: String` | nothing |
//! | `keep_original` | `id: DownloadId` | nothing; a video whose conversion failed is saved as it was downloaded (D24) |
//! | `move_to_queue` | `ids: Vec<DownloadId>`, `queue: QueueId` | nothing |
//! | `reorder` | `queue: QueueId`, `ids_in_order: Vec<DownloadId>` | nothing |
//! | `pause_all` | none | nothing |
//! | `resume_all` | none | nothing |
//! | `list_queues` | none | `Vec<Queue>` |
//! | `create_queue` | `name: String` | `Queue` |
//! | `update_queue` | `queue: Queue` | nothing |
//! | `delete_queue` | `id: QueueId` | nothing |
//! | `set_queue_paused` | `id: QueueId`, `paused: bool` | nothing |
//! | `start_queue_now` | `id: QueueId` | nothing |
//! | `get_settings` | none | `AppSettings` |
//! | `change_settings` | `patch: AppSettingsPatch` | nothing; only the settings in the patch change |
//! | `export_settings` | `path: String` | nothing |
//! | `preview_import` | `path: String` | `ImportPreview`: what importing would change |
//! | `import_settings` | `path: String` | nothing |
//! | `pending_confirmations` | none | `Vec<PendingCapture>` |
//! | `confirm_capture` | `id: u64`, `confirmation: CaptureConfirmation` | `DownloadId` of the new download; a download window that asked becomes its progress window |
//! | `reject_capture` | `id: u64` | nothing |
//! | `open_file` | `id: DownloadId`, `allow_program: Option<bool>` | nothing; a program is refused unless `allow_program` |
//! | `is_program` | `id: DownloadId` | `bool`: opening it would run code, so ask first |
//! | `open_folder` | `id: DownloadId` | nothing |
//! | `set_download_folder` | `id: DownloadId`, `folder: String` | nothing |
//! | `take_launch_links` | none | `Vec<String>`: links passed on the command line, handed over once |
//! | `choose_media` | `id: u64`, `selection: MediaSelection` | `DownloadId`, like `confirm_capture`; fails while an option that needs the media add-on lacks it (the core then sends `AddonRequired` and keeps the choice) |
//! | `dismiss_media_choice` | `id: u64` | nothing |
//! | `pending_media_choices` | none | `Vec<MediaChoice>`: choices still open |
//! | `pending_addon_requests` | none | `Vec<{ id: u64, addon: AddonInfo, url: String }>`: pages waiting for the add-on, oldest first; `dismiss_media_choice(id)` drops one |
//! | `list_addons` | none | `Vec<AddonInfo>` |
//! | `install_addon` | `id: String` | nothing; progress and the result arrive as `AddonsChanged` |
//! | `update_addon` | `id: String` | nothing; like `install_addon` |
//! | `remove_addon` | `id: String` | nothing |
//! | `cancel_addon_operation` | `id: String` | nothing; stops an install or update, keeping an installed version working |
//! | `check_addon_updates` | none | `Vec<AddonInfo>` after looking for a newer media add-on now; fails when the releases cannot be reached |
//! | `get_question` | `id: u64` | `Option<Question>` (see [`questions::Question`]): the open question a download window shows |
//! | `get_download` | `id: DownloadId` | `DownloadSummary` of one download, for its progress window |
//! | `earlier_download` | `id: u64` | `Option<DownloadSummary>`: the download in the list that question `id` is about again |
//! | `show_question_windows` | none | nothing; every open question's window comes to the front or opens again |
//! | `show_progress_window` | `id: DownloadId` | nothing; opens (or brings forward) that download's window |
//! | `close_download_window` | none | nothing; closes the download window that calls it |
//! | `main_window_ready` | none | nothing; shows the main window once its first view is drawn (it opens hidden, so it never flashes blank) |
//! | `fit_download_window` | `height: f64` | nothing; makes the download window that calls it as tall as its content (`height`, logical pixels), within its screen |
//!
//! Every command reaches the real core; none is pending. The core checks for add-on updates
//! by itself (once a day, while `auto_update_addons` is on), so the desktop never schedules it.
//!
//! # Events
//!
//! - [`EVENT_NAME`] carries every core `UiEvent`, in order.
//! - [`RESYNC_EVENT`] (no payload): events were missed; reload everything.
//! - [`window::LINKS_EVENT`] (no payload): links arrived from a second launch; fetch them with
//!   `take_launch_links`.
//!
//! Both events reach every window: the main window and the download windows (D23, see
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
pub mod appimage;
pub mod autostart;
pub mod background;
pub mod bridge;
mod commands;
pub mod download_windows;
pub mod elevation;
pub mod host;
pub mod lifecycle;
pub mod questions;
pub mod sleep;
pub mod tray;
pub mod updates;
pub mod window;

use tauri::Manager as _;
use tauri_plugin_dialog::{DialogExt as _, MessageDialogKind};
use wirecatch_core::ManagerConfig;
use wirecatch_ipc::IpcError;
use wirecatch_product::Channel;

pub use window::{BACKGROUND_FLAG, wants_background};

use crate::host::Core;
use crate::lifecycle::Lifecycle;
use crate::window::{Launch, LaunchLinks};

/// The one Tauri event that carries every `UiEvent` to the webview.
pub const EVENT_NAME: &str = "wirecatch://event";

/// Shown by a copy of Wirecatch that finds another one already serving this user.
pub const ALREADY_RUNNING: &str = "Wirecatch is already running for you, so this copy will close. Look for its icon in the tray, or open it from the Start menu again.";

/// The start-at-sign-in entry's name for this build (the `Run` registry value on Windows): its
/// channel's name. The NSIS uninstall hook removes the release one by the same name.
#[must_use]
pub fn autostart_name() -> &'static str {
    host::CHANNEL.name()
}

/// Tells the webview it missed events and must reload everything.
pub const RESYNC_EVENT: &str = "wirecatch://resync";

fn init_logging() {
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));
    if let Err(e) = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(std::io::stderr)
        .try_init()
    {
        // Reaches only a logger set up before this one, if there is any.
        tracing::warn!(
            "Wirecatch's logger was not installed, so its lines do not reach stderr: {e}"
        );
    }
}

/// Starts the app.
pub fn run() {
    init_logging();
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
        .manage(Lifecycle::default())
        .manage(sleep::MainAsleep::default())
        .manage(download_windows::DownloadWindows::default())
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
        .plugin(tauri_plugin_notification::init())
        .plugin(
            tauri_plugin_autostart::Builder::new()
                .args([BACKGROUND_FLAG])
                .app_name(autostart_name())
                .build(),
        )
}

fn setup(app: &mut tauri::App, launch: &Launch) -> Result<(), Box<dyn std::error::Error>> {
    let handle = app.handle().clone();

    // Claim the browser endpoint before opening the database: if another Wirecatch already
    // serves this user, a second manager on the same download list must not start.
    let listener = match tauri::async_runtime::block_on(async { bridge::bind() }) {
        Ok(listener) => Some(listener),
        Err(IpcError::AlreadyInUse { endpoint }) => {
            tracing::error!(
                "another copy of Wirecatch is already running (it owns {endpoint}); this one exits"
            );
            // Say so instead of vanishing. The message shows once the event loop runs; this
            // copy opens nothing else and exits when it is closed.
            let closing = handle.clone();
            handle
                .dialog()
                .message(ALREADY_RUNNING)
                .title(host::CHANNEL.name())
                .kind(MessageDialogKind::Info)
                .show(move |_| closing.exit(0));
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
    let config = ManagerConfig::new(data_dir.clone(), host::known_folders(&home));
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

/// Opens the manager, then starts the work that runs alongside it.
async fn start(app: tauri::AppHandle, listener: Option<wirecatch_ipc::Listener>) {
    let core = app.state::<Core>();
    let manager = match core.manager().await.cloned() {
        Ok(manager) => manager,
        Err(reason) => {
            unavailable(&app, listener, reason.message);
            return;
        }
    };
    if let Some(notice) = manager.startup_notice() {
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
