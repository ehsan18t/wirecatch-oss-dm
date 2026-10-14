//! Which commands each window may call, as the app's real capabilities decide, and what the
//! commands themselves let a window act on (DRS-4, D43): a download window shows text that
//! websites chose, so it gets only what it uses, never what writes or reads files, changes the
//! list or settings, or adds downloads, and acts only on the question or download it shows.

use serde_json::{Value, json};
use tauri::ipc::{CallbackFn, InvokeBody};
use tauri::test::{
    INVOKE_KEY, MockRuntime, get_ipc_response, mock_builder, mock_context, noop_assets,
};
use tauri::webview::InvokeRequest;
use tauri::{App, Manager as _, WebviewUrl, WebviewWindow, WebviewWindowBuilder};
use wirecatch_core::{KnownFolders, ManagerConfig};
use wirecatch_desktop_lib::download_windows::{
    self, DownloadWindows, NOT_ITS_OWN, NOT_ITS_SETTING, Shows,
};
use wirecatch_desktop_lib::host::Core;
use wirecatch_desktop_lib::window::MAIN_WINDOW;
use wirecatch_protocol::desktop::COMMAND_NAMES;
use wirecatch_protocol::{DownloadId, QuestionId};
use wirecatch_test_support::TestResult;

/// What a command a download window may call acts on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ActsOn {
    /// A download, by its id: the window's own only.
    Download,
    /// A question, by its id: the window's own only.
    Question,
    /// No question or download.
    Neither,
}

/// Every command a download window may call. Any other command is the main window's alone, so
/// granting a download window something more means adding it here, where a review sees it.
const DOWNLOAD_WINDOW: &[(&str, ActsOn)] = &[
    ("pause", ActsOn::Download),
    ("resume", ActsOn::Download),
    ("remove", ActsOn::Download),
    ("retry", ActsOn::Download),
    ("refresh_link", ActsOn::Download),
    ("keep_original", ActsOn::Download),
    ("open_file", ActsOn::Download),
    ("open_folder", ActsOn::Download),
    ("set_download_folder", ActsOn::Download),
    ("get_download", ActsOn::Download),
    ("confirm_capture", ActsOn::Question),
    ("reject_capture", ActsOn::Question),
    ("choose_media", ActsOn::Question),
    ("dismiss_media_choice", ActsOn::Question),
    ("get_question", ActsOn::Question),
    ("earlier_download", ActsOn::Question),
    ("list_queues", ActsOn::Neither),
    ("get_settings", ActsOn::Neither),
    ("change_settings", ActsOn::Neither),
    ("list_addons", ActsOn::Neither),
    ("install_addon", ActsOn::Neither),
    ("cancel_addon_operation", ActsOn::Neither),
    ("close_download_window", ActsOn::Neither),
    ("fit_download_window", ActsOn::Neither),
    ("report_unreadable_event", ActsOn::Neither),
];

/// Of [`DOWNLOAD_WINDOW`], the commands the main window has no use for. The main window may call
/// every other command.
const DOWNLOAD_WINDOW_ONLY: &[&str] = &[
    "get_question",
    "get_download",
    "earlier_download",
    "confirm_capture",
    "reject_capture",
    "choose_media",
    "dismiss_media_choice",
    "close_download_window",
    "fit_download_window",
];

fn download_window_may(command: &str) -> bool {
    DOWNLOAD_WINDOW.iter().any(|&(name, _)| name == command)
}

fn window(app: &App<MockRuntime>, label: &str) -> TestResult<WebviewWindow<MockRuntime>> {
    match app.get_webview_window(label) {
        Some(window) => Ok(window),
        None => Ok(WebviewWindowBuilder::new(app, label, WebviewUrl::default()).build()?),
    }
}

/// `window`'s page calling `command` with `body`, as the app answers it.
fn call(
    window: &WebviewWindow<MockRuntime>,
    command: &str,
    body: Value,
) -> TestResult<Result<Value, Value>> {
    let local = if cfg!(windows) {
        "http://tauri.localhost"
    } else {
        "tauri://localhost"
    };
    let request = InvokeRequest {
        cmd: command.into(),
        callback: CallbackFn(0),
        error: CallbackFn(1),
        url: local.parse()?,
        body: InvokeBody::Json(body),
        headers: tauri::http::HeaderMap::default(),
        invoke_key: INVOKE_KEY.to_owned(),
    };
    match get_ipc_response(window, request) {
        Ok(answer) => Ok(Ok(answer.deserialize()?)),
        Err(error) => Ok(Err(error)),
    }
}

/// Whether the capabilities let `window`'s page call `command`, in an app with no command
/// registered: a command they allow is then "not found", one they refuse is refused first.
fn may_call(window: &WebviewWindow<MockRuntime>, command: &str) -> TestResult<bool> {
    let unregistered = json!(format!("Command {command} not found"));
    match call(window, command, json!({}))? {
        Err(answer) => Ok(answer == unregistered),
        Ok(_) => Err(format!("{command} answered though no command is registered").into()),
    }
}

#[test]
fn each_window_may_call_only_the_commands_it_uses() -> TestResult {
    let app = mock_builder().build(tauri::generate_context!(test = true))?;
    let main = window(&app, MAIN_WINDOW)?;
    let download = window(&app, &Shows::Question(QuestionId(1)).label())?;
    let progress = window(&app, &Shows::Download(DownloadId(1)).label())?;
    for name in COMMAND_NAMES.iter().map(|name| name.0) {
        let main_uses = !DOWNLOAD_WINDOW_ONLY.contains(&name);
        assert_eq!(may_call(&main, name)?, main_uses, "main window, {name}");
        for window in [&download, &progress] {
            assert_eq!(
                may_call(window, name)?,
                download_window_may(name),
                "{}, {name}",
                window.label()
            );
        }
    }
    Ok(())
}

#[test]
fn every_named_command_is_one_of_the_contract() {
    let contract: Vec<&str> = COMMAND_NAMES.iter().map(|name| name.0).collect();
    for &(name, _) in DOWNLOAD_WINDOW {
        assert!(contract.contains(&name), "{name} is not a command");
    }
    for name in DOWNLOAD_WINDOW_ONLY {
        assert!(
            download_window_may(name),
            "{name} is not a download window's"
        );
    }
}

/// Within the commands it may call, a download window acts only on the question or download it
/// shows now; the main window acts on any.
#[test]
fn a_download_window_acts_only_on_what_it_shows() -> TestResult {
    let app = mock_builder()
        .manage(DownloadWindows::default())
        .build(mock_context(noop_assets()))?;
    let handle = app.handle();
    let shown = DownloadId(5);
    download_windows::show_download(handle, shown);
    let may = |label: &str, target| download_windows::may_act_on(handle, label, target);
    let label = Shows::Download(shown).label();
    assert!(may(&label, Shows::Download(shown)));
    assert!(!may(&label, Shows::Download(DownloadId(6))));
    assert!(!may(&label, Shows::Question(QuestionId(5))));
    let closed = Shows::Question(QuestionId(9));
    assert!(!may(&closed.label(), closed));
    assert!(may(MAIN_WINDOW, Shows::Download(DownloadId(6))));
    Ok(())
}

/// The arguments of `command` aimed at download or question `id`.
fn aimed_at(command: &str, id: &Value) -> TestResult<Value> {
    let args = match command {
        "remove" => json!({ "ids": [id], "delete_file": false }),
        "refresh_link" => json!({ "id": id, "url": "https://example.com/file.bin" }),
        "set_download_folder" => json!({ "id": id, "folder": "folder" }),
        "confirm_capture" => json!({ "id": id, "confirmation": {} }),
        "choose_media" => json!({ "id": id, "selection": { "option_id": "best" } }),
        "pause"
        | "resume"
        | "retry"
        | "keep_original"
        | "open_file"
        | "open_folder"
        | "get_download"
        | "reject_capture"
        | "dismiss_media_choice"
        | "get_question"
        | "earlier_download" => json!({ "id": id }),
        _ => return Err(format!("say how {command} names what it acts on").into()),
    };
    Ok(json!({ "args": args }))
}

fn change_settings(
    window: &WebviewWindow<MockRuntime>,
    patch: &Value,
) -> TestResult<Result<Value, Value>> {
    call(
        window,
        "change_settings",
        json!({ "args": { "patch": patch } }),
    )
}

/// The answer's message, when the command refused.
fn refusal(answer: &Result<Value, Value>) -> Option<&str> {
    answer.as_ref().err()?.get("message")?.as_str()
}

/// The capabilities let a download window call these commands for any id, so each command's own
/// check is all that keeps the window to what it shows. The commands run for real here, against
/// a manager with no downloads: a call that gets past the check fails later, for another reason.
#[test]
fn every_command_keeps_a_download_window_to_what_it_shows() -> TestResult {
    let folder = tempfile::tempdir()?;
    let config = ManagerConfig::new(
        folder.path().join("data"),
        KnownFolders::under_home(folder.path()),
    );
    let app = mock_builder()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .manage(Core::new(config))
        .manage(DownloadWindows::default())
        .invoke_handler(wirecatch_desktop_lib::handler())
        .build(tauri::generate_context!(test = true))?;
    let (shown_download, shown_question) = (DownloadId(1), QuestionId(1));
    download_windows::show_download(app.handle(), shown_download);
    download_windows::ask(app.handle(), shown_question);
    let main = window(&app, MAIN_WINDOW)?;
    let progress = window(&app, &Shows::Download(shown_download).label())?;
    let question = window(&app, &Shows::Question(shown_question).label())?;
    for &(name, acts) in DOWNLOAD_WINDOW {
        let (shows, own, another) = match acts {
            ActsOn::Download => (&progress, json!(shown_download), json!(DownloadId(2))),
            ActsOn::Question => (&question, json!(shown_question), json!(QuestionId(2))),
            ActsOn::Neither => continue,
        };
        let to_another = call(shows, name, aimed_at(name, &another)?)?;
        assert_eq!(
            refusal(&to_another),
            Some(NOT_ITS_OWN),
            "{name}: {to_another:?}"
        );
        let to_its_own = call(shows, name, aimed_at(name, &own)?)?;
        assert_ne!(refusal(&to_its_own), Some(NOT_ITS_OWN), "{name} on its own");
        if !DOWNLOAD_WINDOW_ONLY.contains(&name) {
            let from_main = call(&main, name, aimed_at(name, &another)?)?;
            assert_ne!(
                refusal(&from_main),
                Some(NOT_ITS_OWN),
                "main window, {name}"
            );
        }
    }

    let another_setting = json!({ "close_to_tray": false });
    let refused = change_settings(&progress, &another_setting)?;
    assert_eq!(refusal(&refused), Some(NOT_ITS_SETTING), "{refused:?}");
    let its_own = change_settings(&progress, &json!({ "show_progress_windows": true }))?;
    assert!(its_own.is_ok(), "its own setting: {its_own:?}");
    let from_main = change_settings(&main, &another_setting)?;
    assert!(from_main.is_ok(), "main window: {from_main:?}");
    Ok(())
}
