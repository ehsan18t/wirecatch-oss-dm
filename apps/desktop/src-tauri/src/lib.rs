//! The Wirecatch desktop app: one process that hosts the download manager (`wirecatch-core`),
//! serves the browser bridge, sits in the tray, and shows the interface when asked (D13).
//!
//! # Command contract
//!
//! The interface (`apps/desktop/src/lib/backend/tauri.ts`) calls exactly these commands with
//! these parameter names (the webview sends the camelCase form, for example `deleteFile` for
//! `delete_file`; Tauri maps it). A test on the TypeScript side checks that both lists agree.
//! Every command fails with a plain-language `String`, the core's own error text.
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
//! | `update_settings` | `settings: AppSettings` | nothing |
//! | `export_settings` | `path: String` | nothing |
//! | `import_settings` | `path: String` | nothing |
//! | `pending_confirmations` | none | `Vec<PendingCapture>` |
//! | `confirm_capture` | `id: u64`, `confirmation: CaptureConfirmation` | nothing |
//! | `reject_capture` | `id: u64` | nothing |
//! | `open_file` | `id: DownloadId` | nothing |
//! | `open_folder` | `id: DownloadId` | nothing |
//! | `set_download_folder` | `id: DownloadId`, `folder: String` | nothing |
//! | `take_launch_links` | none | `Vec<String>`: links passed on the command line, handed over once |
//! | `choose_media` | `id: u64`, `selection: MediaSelection` | nothing (pending, WP11) |
//! | `dismiss_media_choice` | `id: u64` | nothing (pending, WP11) |
//! | `list_addons` | none | `Vec<AddonInfo>` (pending, WP11) |
//! | `install_addon` | `id: String` | nothing (pending, WP11) |
//! | `update_addon` | `id: String` | nothing (pending, WP11) |
//! | `remove_addon` | `id: String` | nothing (pending, WP11) |
//!
//! "Pending" commands exist with their final names and types but answer
//! [`api::NEXT_UPDATE`] until the core implements media and add-ons.
//!
//! # Events
//!
//! - [`EVENT_NAME`] carries every core `UiEvent`, in order.
//! - [`RESYNC_EVENT`] (no payload): events were missed; reload everything.
//! - [`window::LINKS_EVENT`] (no payload): links arrived from a second launch; fetch them with
//!   `take_launch_links`.
//!
//! # Starting
//!
//! - A second launch never starts a second app: it focuses the running one (opening its window
//!   if it was closed to the tray) and passes any links on its command line to the Add dialog.
//!   With `--background` and no links it does nothing visible.
//! - `--background` (the bridge and start-with-system pass it) starts with no window.
//! - On every start the app runs `wirecatch-bridge install` from its own folder (D19). Debug
//!   builds skip that unless `WIRECATCH_REGISTER_BRIDGE=1`.
//!
//! # Updates
//!
//! `tauri-plugin-updater` is compiled in but only switched on when `tauri.conf.json` has a
//! `plugins.updater` section (endpoints and the public signing key). See `apps/desktop/PACKAGING.md`.

pub mod activity;
pub mod api;
mod background;
pub mod bridge;
mod commands;
pub mod host;
pub mod lifecycle;
pub mod tray;
pub mod window;

use tauri::Manager as _;
use wirecatch_core::ManagerConfig;
use wirecatch_ipc::IpcError;

pub use window::{BACKGROUND_FLAG, wants_background};

use crate::host::Core;
use crate::lifecycle::Lifecycle;
use crate::window::{Launch, LaunchLinks};

/// The one Tauri event that carries every `UiEvent` to the webview.
pub const EVENT_NAME: &str = "wirecatch://event";

/// Tells the webview it missed events and must reload everything.
pub const RESYNC_EVENT: &str = "wirecatch://resync";

fn init_logging() {
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));
    let _ = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(std::io::stderr)
        .try_init();
}

/// Starts the app.
pub fn run() {
    init_logging();
    let args: Vec<String> = std::env::args().collect();
    let launch = Launch::from_args(&args);

    let app = tauri::Builder::default()
        // First, so a second launch hands over its command line and exits before anything else.
        .plugin(tauri_plugin_single_instance::init(|app, argv, _cwd| {
            window::handle_launch(app, &Launch::from_args(&argv));
        }))
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(
            tauri_plugin_autostart::Builder::new()
                .args([BACKGROUND_FLAG])
                .build(),
        )
        .manage(LaunchLinks::default())
        .manage(Lifecycle::default())
        .setup(move |app| setup(app, launch))
        .invoke_handler(tauri::generate_handler![
            commands::list_downloads,
            commands::add_download,
            commands::pause,
            commands::resume,
            commands::cancel,
            commands::remove,
            commands::retry,
            commands::refresh_link,
            commands::move_to_queue,
            commands::reorder,
            commands::pause_all,
            commands::resume_all,
            commands::list_queues,
            commands::create_queue,
            commands::update_queue,
            commands::delete_queue,
            commands::set_queue_paused,
            commands::start_queue_now,
            commands::get_settings,
            commands::update_settings,
            commands::export_settings,
            commands::import_settings,
            commands::pending_confirmations,
            commands::confirm_capture,
            commands::reject_capture,
            commands::open_file,
            commands::open_folder,
            commands::set_download_folder,
            commands::take_launch_links,
            commands::choose_media,
            commands::dismiss_media_choice,
            commands::list_addons,
            commands::install_addon,
            commands::update_addon,
            commands::remove_addon,
        ])
        .build(tauri::generate_context!());
    match app {
        Ok(app) => app.run(lifecycle::on_run_event),
        Err(err) => {
            tracing::error!("Wirecatch could not start: {err}");
            std::process::exit(1);
        }
    }
}

fn setup(app: &mut tauri::App, launch: Launch) -> Result<(), Box<dyn std::error::Error>> {
    let handle = app.handle().clone();

    // Claim the browser endpoint before opening the database: if another Wirecatch already
    // serves this user, a second manager on the same download list must not start.
    let listener = match tauri::async_runtime::block_on(async { bridge::bind() }) {
        Ok(listener) => Some(listener),
        Err(IpcError::AlreadyInUse { endpoint }) => {
            tracing::error!(
                "another copy of Wirecatch is already running (it owns {endpoint}); this one exits"
            );
            std::process::exit(0);
        }
        Err(e) => {
            tracing::error!("browser downloads cannot reach Wirecatch: {e}");
            None
        }
    };

    if updater_configured(app) {
        handle.plugin(tauri_plugin_updater::Builder::new().build())?;
    }

    let data_dir = host::data_dir(&app.path().data_dir()?);
    let home = app.path().home_dir().unwrap_or_else(|_| data_dir.clone());
    let config = ManagerConfig::new(data_dir, host::known_folders(&home));
    app.manage(Core::new(config));

    tray::create(&handle)?;
    let starting = handle.clone();
    let task = tauri::async_runtime::spawn(async move { start(starting, listener).await });
    app.state::<Lifecycle>().keep(task);

    window::handle_launch(&handle, &launch);

    if bridge::registers_on_start() {
        match std::env::current_exe()
            .ok()
            .as_deref()
            .and_then(bridge::bridge_path)
        {
            Some(bridge) => bridge::register_in_background(bridge),
            None => tracing::warn!("could not find the browser bridge next to the app"),
        }
    } else {
        tracing::info!(
            "debug build: not registering the browser bridge (set {}=1 to do it)",
            bridge::REGISTER_VAR
        );
    }
    Ok(())
}

/// Opens the manager, then starts the work that runs alongside it.
async fn start(app: tauri::AppHandle, listener: Option<wirecatch_ipc::Listener>) {
    let core = app.state::<Core>();
    let Ok(manager) = core.manager().await.cloned() else {
        // The interface shows the reason on its first command.
        return;
    };
    let life = app.state::<Lifecycle>();
    life.keep(tauri::async_runtime::spawn(background::forward_events(
        app.clone(),
        manager.clone(),
    )));
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

/// The updater stays off until `plugins.updater` is configured with endpoints and a public key
/// (D19: that needs the GitHub organization and a signing key first).
fn updater_configured(app: &tauri::App) -> bool {
    app.config().plugins.0.get("updater").is_some_and(|c| {
        c.get("pubkey")
            .is_some_and(|k| k.as_str().is_some_and(|k| !k.is_empty()))
    })
}
