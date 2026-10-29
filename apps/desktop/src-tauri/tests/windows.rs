//! Which commands each window may call, as the app's real capabilities decide, and what the
//! commands themselves let a window act on (DRS-4, D43): a download window shows text that
//! websites chose, so it gets only what it uses, never what writes or reads files, changes the
//! list or settings, or adds downloads, and acts only on the question or download it shows.

use std::path::Path;

use serde_json::{Value, json};
use tauri::ipc::{CallbackFn, InvokeBody};
use tauri::test::{
    INVOKE_KEY, MockRuntime, get_ipc_response, mock_builder, mock_context, noop_assets,
};
use tauri::webview::InvokeRequest;
use tauri::{App, Manager as _, WebviewUrl, WebviewWindow, WebviewWindowBuilder};
use tokio::sync::broadcast;
use wirecatch_core::{DEFAULT_QUEUE_ID, KnownFolders, Manager, ManagerConfig};
use wirecatch_desktop_lib::api::WarningsShown;
use wirecatch_desktop_lib::download_windows::{
    self, DownloadWindows, NOT_ITS_OWN, NOT_ITS_SETTING, Shows,
};
use wirecatch_desktop_lib::host::Core;
use wirecatch_desktop_lib::window::MAIN_WINDOW;
use wirecatch_protocol::app::{DownloadStatus, DownloadSummary, UiEvent};
use wirecatch_protocol::desktop::COMMAND_NAMES;
use wirecatch_protocol::extension::{CaptureSource, NewDownload};
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
    ("show_proxy_settings", ActsOn::Neither),
    ("show_download_details", ActsOn::Download),
    ("open_file", ActsOn::Download),
    ("open_folder", ActsOn::Download),
    ("set_download_folder", ActsOn::Download),
    ("get_download", ActsOn::Download),
    ("confirm_capture", ActsOn::Question),
    ("reject_capture", ActsOn::Question),
    ("choose_media", ActsOn::Question),
    ("dismiss_media_choice", ActsOn::Question),
    ("answer_file_exists", ActsOn::Question),
    ("get_question", ActsOn::Question),
    ("earlier_download", ActsOn::Question),
    ("list_queues", ActsOn::Neither),
    ("get_settings", ActsOn::Neither),
    ("change_settings", ActsOn::Neither),
    ("list_addons", ActsOn::Neither),
    ("install_addon", ActsOn::Neither),
    ("cancel_addon_operation", ActsOn::Neither),
    ("close_download_window", ActsOn::Neither),
    ("finished_list", ActsOn::Neither),
    ("fit_download_window", ActsOn::Neither),
    ("report_unreadable_event", ActsOn::Neither),
    ("system_appearance", ActsOn::Neither),
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
    "answer_file_exists",
    "close_download_window",
    "fit_download_window",
    "finished_list",
    "show_proxy_settings",
    "show_download_details",
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

/// Every window's title bar drags it (Tauri's `data-tauri-drag-region`, which Linux needs); only
/// the main window maximizes, so only it may ask for a double-click's maximize.
#[test]
fn each_title_bar_drags_its_window_and_only_the_main_one_maximizes() -> TestResult {
    let app = mock_builder().build(tauri::generate_context!(test = true))?;
    let main = window(&app, MAIN_WINDOW)?;
    let download = window(&app, &Shows::Question(QuestionId(1)).label())?;
    let progress = window(&app, &Shows::Download(DownloadId(1)).label())?;
    let refused = |window: &WebviewWindow<MockRuntime>, command: &str| -> TestResult<bool> {
        Ok(match call(window, command, json!({}))? {
            Ok(_) => false,
            Err(answer) => answer
                .as_str()
                .is_some_and(|text| text.contains("not allowed")),
        })
    };
    for window in [&main, &download, &progress] {
        assert!(
            !refused(window, "plugin:window|start_dragging")?,
            "{} drags",
            window.label()
        );
    }
    assert!(!refused(&main, "plugin:window|internal_toggle_maximize")?);
    for window in [&download, &progress] {
        assert!(
            refused(window, "plugin:window|internal_toggle_maximize")?,
            "{} does not maximize",
            window.label()
        );
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
        "answer_file_exists" => {
            json!({ "id": id, "answer": { "choice": "keep_both", "remember": false } })
        }
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
        | "earlier_download"
        | "show_download_details" => json!({ "id": id }),
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

/// The app with its commands, plugins and state, its manager's data in `folder`.
fn app_with_commands(folder: &Path) -> TestResult<App<MockRuntime>> {
    let config = ManagerConfig::new(folder.join("data"), KnownFolders::under_home(folder));
    Ok(mock_builder()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .manage(Core::new(config))
        .manage(DownloadWindows::default())
        .manage(WarningsShown::default())
        .invoke_handler(wirecatch_desktop_lib::handler())
        .build(tauri::generate_context!(test = true))?)
}

/// The capabilities let a download window call these commands for any id, so each command's own
/// check is all that keeps the window to what it shows. The commands run for real here, against
/// a manager with no downloads: a call that gets past the check fails later, for another reason.
#[test]
fn every_command_keeps_a_download_window_to_what_it_shows() -> TestResult {
    let folder = tempfile::tempdir()?;
    let app = app_with_commands(folder.path())?;
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

/// A page may call `open_file` as often as it likes, with no click: while the shell's warning
/// for a download is up, opening it again raises no second warning and opens nothing.
#[test]
fn a_download_raises_one_program_warning_at_a_time() -> TestResult {
    let folder = tempfile::tempdir()?;
    let app = app_with_commands(folder.path())?;
    let shown = DownloadId(1);
    download_windows::show_download(app.handle(), shown);
    let progress = window(&app, &Shows::Download(shown).label())?;
    let open = || call(&progress, "open_file", json!({ "args": { "id": shown } }));

    let warnings = app.state::<WarningsShown>();
    let showing = warnings.show(shown);
    assert!(showing.is_some());
    assert_eq!(open()?, Ok(Value::Null), "while its warning is up");
    drop(showing);
    let answer = open()?;
    assert!(
        answer.is_err(),
        "once it is answered, it opens again: {answer:?}"
    );
    Ok(())
}

/// A website's download, captured into a paused queue so it waits without running.
fn website_download(manager: &Manager) -> TestResult<DownloadId> {
    manager.set_queue_paused(DEFAULT_QUEUE_ID, true)?;
    let mut settings = manager.get_settings();
    settings.extension.ask_before_download = false;
    manager.update_settings(settings)?;
    let capture = NewDownload {
        handover_id: "handover-1".into(),
        // An address on the internet, so no name is looked up first.
        url: "https://93.184.215.14/file.bin".into(),
        referrer: None,
        page_url: Some("https://example.com/".into()),
        frame_url: None,
        suggested_name: Some("file.bin".into()),
        mime: None,
        size_hint: None,
        cookies: None,
        user_agent: None,
        extra_headers: vec![],
        source: CaptureSource::BrowserDownload,
        queue: None,
        media: None,
    };
    let added = tauri::async_runtime::block_on(manager.add_capture(capture))?;
    Ok(added.id.ok_or("the capture added no download")?)
}

/// The download once a change on `events` shows it started or failed.
fn started_or_failed(
    events: &mut broadcast::Receiver<UiEvent>,
    id: DownloadId,
) -> TestResult<DownloadSummary> {
    let changed = async {
        loop {
            if let UiEvent::DownloadsChanged(rows) = events.recv().await? {
                let done = rows.into_iter().find(|row| {
                    row.id == id
                        && matches!(
                            row.status,
                            DownloadStatus::Failed
                                | DownloadStatus::Connecting
                                | DownloadStatus::Downloading
                        )
                });
                if let Some(row) = done {
                    return TestResult::Ok(row);
                }
            }
        }
    };
    tauri::async_runtime::block_on(async {
        tokio::time::timeout(wirecatch_test_support::PATIENCE, changed).await?
    })
}

/// Whether the download failed because a website's link may not reach the local network.
fn refused_as_a_websites(row: &DownloadSummary) -> bool {
    row.error.as_ref().is_some_and(|e| {
        e.message
            .contains("does not follow a link a website gave it there")
    })
}

// A download window's page could give its own download any link, which then counted as the
// person's and so reached the local network: a script in the page D43 distrusts lifted D39's
// guard on a website's links (W4 milestone review M3). Only the main window's link is the
// person's.
#[test]
fn a_link_from_a_download_window_never_becomes_the_persons() -> TestResult {
    let folder = tempfile::tempdir()?;
    let app = app_with_commands(folder.path())?;
    let manager = tauri::async_runtime::block_on(app.state::<Core>().manager())?.clone();
    let id = website_download(&manager)?;
    download_windows::show_download(app.handle(), id);
    let progress = window(&app, &Shows::Download(id).label())?;
    let local = json!({ "args": { "id": id, "url": "http://127.0.0.1:9/file.bin" } });
    let mut events = manager.subscribe();

    call(&progress, "refresh_link", local.clone())?.map_err(|e| e.to_string())?;
    manager.set_queue_paused(DEFAULT_QUEUE_ID, false)?;
    let row = started_or_failed(&mut events, id)?;
    assert!(refused_as_a_websites(&row), "{row:?}");

    let main = window(&app, MAIN_WINDOW)?;
    call(&main, "refresh_link", local)?.map_err(|e| e.to_string())?;
    let row = started_or_failed(&mut events, id)?;
    assert_eq!(
        row.status,
        DownloadStatus::Connecting,
        "the main window's link is the person's: {row:?}"
    );
    manager.pause(id)?;
    Ok(())
}
