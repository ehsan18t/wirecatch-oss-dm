# Cross-module flow: feature-behaviour

Every feature in `FEATURES.md`, followed from its control to the code that does the work. Connections were followed, not every file read whole: "works" means the connection exists and does what it claims by reading the code. Nothing here ran in the app. "Works (contract)" means the last steps rest on `contracts/`, with only the call site checked here. A public-function search over `crates` (script in `%TEMP%\wirecatch-audit\2026-11-desktop\uncalled.py`) looked for code nothing calls.

## Files read

Whole unless marked.

crates/desktop/src/main.rs (from `window_options`)
crates/desktop/src/backend.rs (from `start`)
crates/desktop/src/add_dialog.rs
crates/desktop/src/app_update.rs (part)
crates/desktop/src/library.rs (part)
crates/desktop/src/lifecycle.rs
crates/desktop/src/notices.rs
crates/desktop/src/sign_in.rs
crates/desktop/src/tray.rs
crates/desktop/src/download_windows/view.rs (part)
crates/desktop/src/download_windows/finished.rs (part)
crates/desktop/src/download_windows/media_form.rs (part)
crates/desktop/src/download_windows/media_output.rs (part)
crates/desktop/src/main_window/mod.rs
crates/desktop/src/main_window/keymap.rs
crates/desktop/src/main_window/palette.rs
crates/desktop/src/main_window/fixes.rs
crates/desktop/src/main_window/backup_dialog.rs (part)
crates/desktop/src/main_window/notices_bar.rs (part)
crates/desktop/src/main_window/list.rs (part)
crates/desktop/src/main_window/details.rs (part)
crates/desktop/src/main_window/queue_dialog.rs (part)
crates/desktop/src/main_window/queue_header.rs (part)
crates/desktop/src/main_window/settings/about.rs
crates/desktop/src/main_window/settings/addon.rs
crates/desktop/src/main_window/settings/browser.rs
crates/desktop/src/main_window/settings/files.rs
crates/desktop/src/main_window/settings/general.rs
crates/desktop/src/main_window/settings/media.rs
crates/desktop/src/main_window/settings/mod.rs
crates/desktop/src/main_window/settings/network.rs
crates/desktop/src/main_window/settings/notifications.rs
crates/desktop/src/main_window/settings/speed.rs
crates/shell/src/api.rs (part)
crates/shell/src/teller.rs
crates/core/src/manager/added_pages.rs
crates/core/src/manager/app_update.rs (part)
crates/core/src/manager/downloads.rs (part)
crates/core/src/manager/media.rs (part)
crates/core/src/manager/mod.rs (part)
crates/core/src/manager/placement.rs (part)
crates/core/src/manager/policy.rs
crates/core/src/manager/preferences.rs
crates/core/src/manager/proxy.rs
crates/core/src/manager/queues.rs (part)
crates/core/src/manager/relocate.rs (part)
crates/core/src/manager/scheduling.rs (part)
crates/protocol/src/app.rs (part)
crates/store/src/lib.rs (part)
Searches only: crates/server/src/lib.rs, apps/extension/src, .github/workflows/release.yml, scripts/package.mjs, scripts/release/manifest.mjs, docs/DECISIONS.md, docs/ROADMAP.md

## Features traced

### Downloading

- Segmented download: works (contract); connections reach the engine from `run_policy`.
- Server connection limits: works (contract).
- Resume: works (contract).
- Retries: works (contract).
- Protocols: works. HTTP/3 exists only behind the `http3` cargo feature (CLI `--http3`); the app does not offer it, which matches README's "as an option".
- Speed limits: works; global limiter and each queue's limiter change during a run, and tray and Settings reach them.
- Disk writes and finishing: works (contract).
- Local network guard: works; `local_sites` is read by captures and rules.
- Proxy in the engine: works; `queue_proxy` passes proxy, exceptions and FTP bypass.
- Command line: works (contract).

### Download list and actions

- Add a download: works, except a typed name for a video page: Xfeature-behaviour-6.
- Paste a link: works.
- Add from a second launch: works.
- Pause, resume: works.
- Pause all, resume all: works.
- Cancel, remove: works.
- Try again: works in the list. Download windows offer only this: Xfeature-behaviour-5.
- Refresh an expired link: works from the main window; not offered in download windows: Xfeature-behaviour-5.
- Move to another folder: works, including Change folder followed by retry.
- Open, Show in folder: works.
- Row menu, details, Copy link: works.
- List, sort, search, select, keyboard: works; every key in `keymap` has a handler.
- Command palette: works.
- Reorder within a queue: works.
- File already in the folder: works.
- Category folders: works.
- Saved list and repair: works, except the banner's Try again: Xfeature-behaviour-3.
- Sign-ins forgotten after a week: works; the task is started in `spawn_background_tasks`.

### Queues and scheduling

- Queues: works; each form field reaches the plan, policy, proxy, folder or teller.
- Scheduling: works (contract).
- Why a download waits: works.
- Start now, pause a queue: works.
- Move between queues: works.
- Order of queues: works.
- Delete a queue: works.
- Auto queues and rules: works.
- Test a link: works.
- Queue proxy: works.
- Told when a queue finishes: works.

### Browser capture

- Bridge: works (contract).
- Protocol server: works; each call reaches its manager method.
- Capture of a browser download: works (contract).
- Download window: works, except fixes: Xfeature-behaviour-5.
- One window per link: works (contract).
- Without a window: works.
- Settings for the browser: works; capture switch, private windows, media button, ignored sites, file types and minimum size are read by the extension, the queue and asking by core.
- Cookie scope: works (contract).
- Browser registration: works (contract); called from `Backend::start`.

### Video and audio

- HLS and DASH: works (contract).
- Media capture from pages: works (contract).
- Quality choice: works; saved choices reach `MediaDefaults::start`.
- Formats and conversion: works (contract).
- Subtitles, chapters, cover art: works; settings reach the selection and the job.
- Finished video placement: works (contract).
- Media add-on: works; Install, Update, Stop, Remove, Check and automatic updates all connect.
- Child processes: works (contract).

### Failure handling

- Errors with a fix: all seven fixes work in the main window; Xfeature-behaviour-5 for download windows.
- Keep the original: works from the main window.
- Recovery at start: works (contract).
- Logs: works; the Open button and the logger use the same folder.

### Proxies

- App proxy: works.
- Copy from system: works. Connection test: Xfeature-behaviour-2.
- Passwords: works (contract).
- Tray switch: works.

### Settings

- Pages: every field of `AppSettings` has a reader and a control, except as in Xfeature-behaviour-4.
- Search in settings: works; gpui-kit's search over each item's keywords.
- Export and import: works; a leftover event: Xfeature-behaviour-7.
- Appearance: works.

### App shell

- One process, single instance: works.
- Tray: works.
- Close to tray, quit: works.
- Start at sign-in: works.
- Not run as administrator: works.
- Notifications: works; every setting on the page is read by `teller` or `notices`.
- A notice opens its download: works.
- Download complete window: works, except fixes: Xfeature-behaviour-5.
- Mark of the Web, private files: works (contract).
- Windows and themes: works.

### Updates and release tooling

- App updates: Xfeature-behaviour-1.
- Public Suffix List updates: the weekly check works; the promised controls: Xfeature-behaviour-4.
- Installers: names from `package.mjs` match what `manifest.mjs` lists; packaged output not run.
- Product identity: works (contract).
- Gates and CI: the release workflow signs the manifest and uploads it with the installers; not run.

## Findings

### Xfeature-behaviour-1 [P1] The desktop app never looks for updates and always says it has the newest version

- Where: `crates/desktop/src/backend.rs`, `config`; `crates/core/src/manager/mod.rs`, `ManagerConfig::new`, `with_official_app_releases`; `crates/core/src/manager/app_update.rs`, `check_app_update`
- Category: broken feature
- Failure: an official release build checks for updates, by the daily timer or Check for updates. Whatever is published, the check answers "You have the newest version." No update is ever offered, so no installed copy can update itself.
- Evidence: `config` builds `ManagerConfig::new(..).with_official_suffix_list()` only. `new` sets `app_releases: None`. `check_app_update` starts with `let Some(releases) = self.inner.app_releases.clone() else { return Ok(None) }`, and `app_update::check` maps `Ok(None)` to `Stage::Current`. A search of `crates` and `scripts` finds no caller of `with_official_app_releases`. The doc on `app_releases` says the app uses it. Read from the code; not run, since a real update check is outside the audit.
- Confidence: certain

### Xfeature-behaviour-2 [P1] Every proxy connection test reports the proxy as unreachable

- Where: `crates/desktop/src/backend.rs`, `config`; `crates/core/src/manager/mod.rs`, `with_official_proxy_test`; `crates/core/src/manager/proxy.rs`, `run_test`
- Category: broken feature
- Failure: a person presses Test on a working proxy, or saves or copies one, which tests it in the background. The result is always `Unreachable`, shown as "Could not be reached" in red, without any request being sent.
- Evidence: `new` sets `proxy_test_url: None`, and `run_test` answers `ProxyTestResult::Unreachable { reason: "this build has no address to test the proxy with" }` when it is `None`. A search of `crates` finds no caller of `with_official_proxy_test`; only a core test sets a URL, with `with_proxy_test_url`. D71.5 promises the test.
- Confidence: certain

### Xfeature-behaviour-3 [P2] Try again on the storage banner saves nothing and hides a failure that continues

- Where: `crates/desktop/src/main_window/notices_bar.rs`, `storage_banner`; `crates/core/src/manager/preferences.rs`, `apply_settings`; `crates/store/src/lib.rs`, `write_until_closed`
- Category: no-op code
- Failure: the list cannot be saved and the banner shows. Pressing Try again clears the banner at once and tries nothing. If the writes still fail, the banner does not return, so the person believes the list was saved while recent changes may be lost on closing.
- Evidence: the button sends `change_settings(AppSettingsPatch::default())`, under the comment "An empty change goes through the save like any other". The settings do not change, so `apply_settings` returns at `if state.settings == new` before `save_settings`. The store retries kept writes by itself and calls `on_failure` only when `retried == 0`, so no new `StorageFailed` comes during the same spell of failures.
- Confidence: certain

### Xfeature-behaviour-4 [P2] The Public Suffix List's date, last check and Check now do not exist in Settings

- Where: `crates/desktop/src/main_window/settings/general.rs`, `updates`; `crates/shell/src/api.rs`, `suffix_list`, `check_suffix_list`
- Category: broken feature
- Failure: with "Update the Public Suffix List automatically" off, a person has no way to update the list, and cannot see which list is in use or whether the last check failed.
- Evidence: D44.4 promises the day the list was published, when it was last checked, and Check now, and the doc on `AppSettings::update_suffix_list` says "the list in use stays until the person checks". Settings has only the switch. `api::suffix_list` and `api::check_suffix_list` have no callers in `crates/desktop`, and nothing consumes `UiEvent::SuffixListChecked`. `docs/ROADMAP.md` does not list this as open.
- Confidence: certain

### Xfeature-behaviour-5 [P2] Download windows offer only Try again, whatever fix the failure needs

- Where: `crates/desktop/src/download_windows/finished.rs`, `row_buttons`; `crates/desktop/src/download_windows/view.rs`, the progress window's buttons; `crates/protocol/src/app.rs`, `RETRY_FIXES`
- Category: wrong implementation
- Failure: a download fails with an expired link, an unusable folder, a failed conversion or a proxy problem. Its progress window and the download complete window show the message and Try again, which repeats the same failure. Get a new link, Change folder, Keep the original, Proxy settings and Change queue are absent; the person must find the row in the main window.
- Evidence: neither file reads `error.fix`, `FixAction` or `RETRY_FIXES`. `finished::row_buttons` receives only the status and gives every `Failed` row Try again. The doc on `RETRY_FIXES` says a notification and the download complete window offer Try again only for those fixes, because "the others need the person's answer first"; `notifications.rs` obeys it. `main_window::fixes::offered` and `run` are the shared logic these windows do not use.
- Confidence: certain

### Xfeature-behaviour-6 [P2] A file name typed in the Add dialog is dropped for a link to a video site

- Where: `crates/desktop/src/add_dialog.rs`, `submit`; `crates/core/src/manager/added_pages.rs`, `add_video_page`
- Category: wrong implementation
- Failure: a person adds one link to a video site and types a name in File name. The form sends it, and the saved file is named from the video's title. With saved choices (no quality window) nothing shows that the name was ignored.
- Evidence: `submit` puts `file_name` in every `AddRequest`, and shows the field for any single link. `add_video_page` calls `naming::file_name(None, &url)`, and its doc says "`request.file_name` is not used". `Held` has no name field. The field's note says only "Leave empty for the name the server gives."
- Confidence: certain

### Xfeature-behaviour-7 [P3] An import's summary event and the "every video" answer have nothing on the other end

- Where: `crates/core/src/manager/preferences.rs`, `import_settings`; `crates/desktop/src/download_windows/media_form.rs`, `answer`; `crates/core/src/manager/media.rs`, `remember_media_choice`
- Category: dead code
- Failure: `UiEvent::SettingsImported` is built and sent on each import and no code receives it, so its summary, including `addons_to_offer`, goes nowhere; the import dialog shows the preview's copy instead. `MediaSelection::every_video` is always `None` from the only form that sends it, so the branch in `remember_media_choice` never runs; the window's switch changes `ask_each_time` directly.
- Evidence: a search of `crates/desktop`, `crates/shell` and `crates/server` finds no `UiEvent::SettingsImported`. `media_form::answer` sets `selection.every_video = None`, and no other non-test code sets it.
- Confidence: certain

## Suggestions

### Xfeature-behaviour-S1 Test that the app's manager configuration names its official sources

- Today: `ManagerConfig::new` leaves update releases and the proxy test address empty so tests stay offline, and each `with_official_*` must be remembered in `backend::config`. Two were missed, and each failure looks like a normal answer. No test covers `config`.
- Better: a single `ManagerConfig::official` used by the app, or a test in `crates/desktop` asserting that `config()` has `app_releases`, `proxy_test_url` and `suffix_list_source` set. Xfeature-behaviour-1 and 2 could then not come back.
- Value: an update or test connection cannot silently stop working again.
