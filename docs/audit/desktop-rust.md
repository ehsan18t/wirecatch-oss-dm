# Audit: apps/desktop/src-tauri (Rust side of the Tauri app)

> This audit is a snapshot: its paths and line numbers name the code as it was when the audit was made (it was recorded in commit 0bd101a). Some files have since moved or been split, so a path that no longer exists, or a line that no longer matches, refers to the code of that time; `git log -- <path>` shows the commit that moved or removed it.

Scope read in full: `src/*.rs` (lib, commands, api, host, window, download_windows, questions, background, lifecycle, sleep, tray, activity, bridge, appimage, autostart, updates), `tauri.conf.json`, `tauri.bundle.conf.json`, `capabilities/*.json`, `build.rs`, `Cargo.toml`, `windows/hooks.nsh`, `linux/prerm.sh`, `examples/send_capture.rs`, and the test file list. Paths below are relative to `apps/desktop/src-tauri` unless they start with another package.

## DRS-1 Every IPC error is a bare String, and the frontend tells errors apart by matching message prefixes
- severity: high
- category: design
- location: src/api.rs:19, src/api.rs:21-23, src/api.rs:511-519, src/host.rs:31, src/host.rs:45, src/commands.rs:187-192, apps/desktop/src/lib/addons.ts:13-14, apps/desktop/src/lib/window/download-window.svelte.ts:299
- problem: `CommandResult<T> = Result<T, String>` and `plain()` flatten every `CoreError` to its Display text, so the 49 commands give the webview no error kind, no `fix`, and no stable code. The UI already needs to branch on a kind: `isAddonRequired` checks `message.startsWith('This needs the media add-on')`, and a Rust unit test exists only to stop someone rewording that sentence. Any copy edit, translation, or new error that happens to share the prefix silently changes control flow. The protocol already has `UserError { message, fix }` and `ErrorCode`, so the shape exists and is just not used at this boundary. `Core` also caches the open failure as `String`, so `unavailable()` cannot distinguish "database from a newer version" from "folder not writable" to offer the right fix.
- fix: Add a `CommandError { code: CommandErrorCode, message: String, fix: Option<String> }` (serde + ts-rs) to `wirecatch-protocol`, `impl From<CoreError> for CommandError` in core, make `CommandResult<T> = Result<T, CommandError>`, keep `CoreError` in `Core`'s `OnceCell`, and switch the frontend to `err.code === 'addon_required'`. Delete the wording-lock test once nothing parses messages.
- cross-package: yes (protocol, core, packages/protocol, apps/desktop frontend)

## DRS-2 The IPC contract is hand-mirrored in TypeScript instead of generated (D15), in up to six places per item
- severity: high
- category: duplication
- location: src/lib.rs:11-62, src/lib.rs:122, src/lib.rs:141, src/lib.rs:190-241, src/window.rs:15, src/questions.rs:54-67, src/download_windows.rs:42-45, src/download_windows.rs:127-133, src/api.rs:187-195, crates/core/src/manager/media.rs:46, crates/core/src/manager/media.rs:59, apps/desktop/src/lib/backend/types.ts:33-52, apps/desktop/src/lib/backend/types.ts:229-235, apps/desktop/src/lib/window/registry.ts:9-12, apps/desktop/src/lib/filetypes.test.ts:9-10, apps/desktop/src/lib/backend/tauri.test.ts:66-84
- problem: D15 says `wirecatch-protocol` is the single source of truth for every type crossing a process boundary, but at the Tauri boundary: (a) `Question` is defined in the desktop crate and mirrored by hand as `Question` in types.ts; (b) its payloads `MediaReading` and `AddonRequestItem` live in `wirecatch-core` with no ts-rs export and are hand-written in types.ts; (c) the three event names `wirecatch://event`, `wirecatch://resync`, `wirecatch://links` are string literals on both sides with no test linking them, so a rename on either side breaks the UI silently; (d) every command name exists in the doc table, `generate_handler!`, `commands.rs`, `api.rs`, `COMMANDS` in tauri.ts and the `FIXED` list in a test, kept in sync by regex-scraping `lib.rs`; (e) `SAFE_EXTENSIONS` is copied to filetypes.ts and checked by a regex over `api.rs` source, which breaks on any reformat; (f) the route `window/download?question=` and `MAX_QUESTION_WINDOWS`/`CASCADE_STEP` are duplicated in registry.ts. Argument names and payload shapes of commands are not checked at all.
- fix: Move `Question`, `MediaReading`, `AddonRequestItem` (and a `QuestionId`, see DRS-11) into `wirecatch-protocol::app` with ts-rs derives. Generate commands and events with `tauri-specta` (typed `invoke` wrappers, typed events, typed errors from DRS-1), or at minimum export the event names, `SAFE_EXTENSIONS` and window limits from the protocol crate through `pnpm protocol:generate` as a generated `constants.ts`. Then delete the regex-scraping tests and the hand-written mirrors.
- cross-package: yes (protocol, core, packages/protocol, apps/desktop frontend)

## DRS-3 Release builds on Windows write no logs anywhere
- severity: high
- category: error-handling
- location: src/main.rs:2, src/lib.rs:143-150, src/bridge.rs:146-167, src/background.rs:230-250, src/updates.rs:55-72
- problem: `init_logging` writes only to stderr, and release Windows builds use `windows_subsystem = "windows"`, so stderr is detached and every `tracing::warn!`/`error!` is discarded. Several failure paths are documented as "Failures are logged" and have no other outlet: bridge registration (`register_in_background`), start-at-sign-in sync, update check/download/install failures, accept errors on the browser endpoint, data folder migration failures. For a commercial product this makes field support of "browser downloads do not reach the app" impossible.
- fix: Add a rolling file sink in the data folder (`tracing-appender` non-blocking writer, `logs/wirecatch.log`, daily rotation, a few files kept) alongside stderr in debug builds, or adopt `tauri-plugin-log`. Expose "Open log folder" in Settings later. Keep the `EnvFilter` default at `info`.
- cross-package: no

## DRS-4 Every window can call all 49 commands; download windows that render site-supplied text get file-write, file-read and settings commands
- severity: medium
- category: security
- location: build.rs:2, capabilities/download-windows.json:4, capabilities/default.json:4, src/commands.rs:139-152, src/commands.rs:47-54, src/commands.rs:134-137, src/commands.rs:222-229, src/api.rs:131-144
- problem: `build.rs` calls `tauri_build::build()` with no `AppManifest`, so Tauri 2 allows every registered command from every window; both capability descriptions say so. Download windows show file names, page titles, subtitles and URLs that come from websites. If any of that ever reaches script execution (a future `{@html}`, a Svelte bug, a dependency), the attacker gets `export_settings(path)` (writes to any path the user can write, overwriting files), `import_settings`/`preview_import(path)` (reads any file the user can read), `set_download_folder` (moves files to any folder), `remove(ids, delete_file: true)`, `update_settings`, and `open_file(id, allow_program: true)` (runs a downloaded program, see DRS-5). None of the path-taking commands checks that the path came from a dialog. The CSP lowers the odds, but defense in depth is the point of capabilities.
- fix: Use `tauri_build::try_build(Attributes::new().app_manifest(AppManifest::new().commands(&[...])))` so each command needs an explicit `allow-<command>` permission, then grant the main window its set and give `download-*` only what a download window uses (get_question, get_download, earlier_download, confirm_capture, reject_capture, choose_media, dismiss_media_choice, pause, resume, cancel, open_file, open_folder, is_program, set_download_folder, list_addons, install_addon, close_download_window, fit_download_window, plus the settings read it needs). Add a test that parses both capability files and asserts download windows lack `export_settings`, `import_settings`, `preview_import`, `update_settings`, `remove`.
- cross-package: no

## DRS-5 The "this file can run code, open it anyway?" guard is a boolean the caller asserts
- severity: medium
- category: security
- location: src/commands.rs:177-193, src/api.rs:254-276, apps/desktop/src/lib/backend/tauri.ts:175-176
- problem: `open_file(id, allow_program: Option<bool>)` refuses programs unless the webview passes `allowProgram: true`. The confirmation exists only in the UI, so the Rust side cannot tell a confirmed open from any script calling `invoke('open_file', { id, allowProgram: true })`. Opening a downloaded executable is the most dangerous action this app has, and its only server-side check is a flag the untrusted side sets. It also duplicates the decision: `is_program` is computed in Rust, sent to the UI, and the UI sends back its permission.
- fix: Make the confirmation native: when `is_program(path)` is true, show a `tauri_plugin_dialog` warning (`MessageDialogKind::Warning`, buttons "Open" / "Cancel", naming the file and `site_of(url)`) from Rust and open only on "Open"; drop the `allow_program` parameter. If the in-app dialog must stay, have `is_program` return a one-shot nonce bound to the download id that `open_file` must present.
- cross-package: yes (apps/desktop frontend)

## DRS-6 Product identity is hardcoded in 30+ places with no single product config
- severity: medium
- category: magic-value
- location: tauri.conf.json:3-6, tauri.conf.json:18, tauri.conf.json:33-39, tauri.conf.json:51, tauri.bundle.conf.json:4, Cargo.toml:17, src/lib.rs:126, src/lib.rs:129, src/lib.rs:270, src/lib.rs:371, src/host.rs:14, src/host.rs:23, src/host.rs:26, src/host.rs:170-175, src/host.rs:189-192, src/download_windows.rs:467, src/tray.rs:46, src/tray.rs:75, src/tray.rs:147, src/activity.rs:132-134, src/questions.rs:47, src/updates.rs:46, src/updates.rs:63-64, src/bridge.rs:13-17, src/bridge.rs:83, src/bridge.rs:132, src/bridge.rs:173, src/appimage.rs:19, src/appimage.rs:60-106, windows/hooks.nsh:12-13, windows/hooks.nsh:18-19, linux/prerm.sh:14
- problem: Pattern: 34 non-test string literals in `src/*.rs` contain "Wirecatch", plus identity values in config and packaging scripts, none derived from one place. Every hardcoded identity value: app identifier `app.wirecatch.desktop` (tauri.conf.json:6, also as a literal in the lib.rs test at 419 and 425); dev identifier `app.wirecatch.desktop.dev` (host.rs:23); product name `Wirecatch` (tauri.conf.json:3, window title tauri.conf.json:18, NSIS `startMenuFolder` tauri.conf.json:51, `RELEASE_AUTOSTART_NAME` lib.rs:126, dialog title lib.rs:270, fallback title download_windows.rs:467, Windows data folder host.rs:171, NSIS registry value names hooks.nsh:18-19); dev product name `Wirecatch Dev` (host.rs:26); Linux data folder names `wirecatch` and `wirecatch-dev` (host.rs:173-174); main binary `wirecatch-app` (tauri.conf.json:4, Cargo.toml:17, appimage.rs:19, and crates/bridge/src/app.rs:24-26); bridge binary `wirecatch-bridge` (bridge.rs:13-17, tauri.bundle.conf.json:4, hooks.nsh:12-13, prerm.sh:14); endpoint names `wirecatch-app-` and `app.sock` (host.rs:189-192, copied from crates/ipc/src/paths.rs:12,20); env var `WIRECATCH_DATA_DIR` (host.rs:14); event prefix `wirecatch://` (lib.rs:122,141, window.rs:15); publisher and copyright `Wirecatch contributors` (tauri.conf.json:33-34); license `GPL-3.0-only` (tauri.conf.json:35); descriptions (tauri.conf.json:38-39); dev server port 1420 (tauri.conf.json:9). No browser extension id is hardcoded in this crate (good: that lives in config/extension-ids.json). Worst consequences: user-facing copy ignores the build identity, so a debug build's tray menu, tooltip, notifications and dialogs all say "Wirecatch" while its window says "Wirecatch Dev"; and a rename or white-label needs coordinated edits across Rust, JSON, NSIS and shell with only one test (the NSIS hook check) guarding any of it.
- fix: Introduce one product config (for example `config/product.json` read by a small `build.rs` step, or a `wirecatch-product` crate of consts) holding name, dev name, identifier, binary names, data folder names, publisher. Generate `tauri.conf.json` fields from it (or use Tauri's config merge with a generated overlay) and template `hooks.nsh`/`prerm.sh` at bundle time. In Rust, add `fn product_name(app) -> &str` from `app.config().product_name` and use it in every user-facing string (`format!("Open {name}")`).
- cross-package: yes (bridge, ipc, root config, packaging)

## DRS-7 The app version lives in tauri.conf.json separately from Cargo and package.json, and the updater compares against it
- severity: medium
- category: magic-value
- location: tauri.conf.json:5, Cargo.toml:4, apps/desktop/package.json:3, apps/extension/package.json:3
- problem: `tauri.conf.json` hardcodes `"version": "0.1.0"` while the crate uses `version.workspace = true` (root Cargo.toml:7) and the two package.json files carry their own copy. Tauri uses the conf version for the installer metadata and as the current version in `tauri-plugin-updater`'s comparison. A release that bumps the workspace but not this file ships a binary that reports the old version, so the updater offers the same release forever (or never, if bumped the other way), and the bridge's `client_version` and the app disagree.
- fix: Remove `version` from `tauri.conf.json` (Tauri 2 then uses the Cargo package version) or point it at `../package.json`, and add a CI check (or a release script) that the workspace version and both package.json versions match.
- cross-package: yes (root Cargo.toml, apps/extension, CI)

## DRS-8 Every core event, including every progress tick of every download, is sent to every open webview
- severity: medium
- category: performance
- location: src/background.rs:55-70
- problem: `forward` calls `app.emit(EVENT_NAME, &event)` (or `emit_filter` excluding only a sleeping main window) for every `UiEvent`. Download windows (up to five asking plus any number of progress windows, D23) each receive all `DownloadsChanged` progress events for all downloads and discard what is not theirs, so N windows times M events of JSON parse and script work, waking every renderer process even for downloads it does not show. It also calls `app.webview_windows()` per event, which builds a fresh `HashMap<String, WebviewWindow>` on every event in the hottest path of the shell, against the repo rule of no per-event allocations in hot paths.
- fix: Route by window: keep the main window on the full stream, and for each download window emit with `emit_to(EventTarget::webview_window(label), ...)` only events that mention its download or question (the `Registry` already knows what each label shows; filter `DownloadsChanged` rows to that id). Replace `webview_windows().is_empty()` with a cheap check (an `AtomicUsize` window count maintained in `on_run_event`, or the registry plus `get_webview_window(MAIN_WINDOW)`).
- cross-package: yes (apps/desktop frontend: download windows stop filtering)

## DRS-9 A downloaded update is held in RAM until the person quits, possibly for days
- severity: medium
- category: performance
- location: src/updates.rs:22, src/updates.rs:43-50, src/updates.rs:76-86
- problem: `PendingUpdate(Mutex<Option<(Update, Vec<u8>)>>)` keeps the whole installer or AppImage in memory from download until quit. A tray app that stays up for weeks (D13 measured its hidden footprint at about 25 MB and treats memory as a feature) would then hold tens of MB (NSIS installer) to about 100 MB (AppImage) of dead bytes. A crash or forced shutdown also loses the download, so it is fetched again next run.
- fix: Write the verified bytes to a file in the data folder (`updates/<version>.bin`, owner-only), keep only the path and `Update` metadata in state, and read the file in `install_pending`. Delete stale files on start.
- cross-package: no

## DRS-10 Bridge connection tasks are untracked, so quitting stops the accept loop but not the connections still calling the manager
- severity: medium
- category: concurrency
- location: src/bridge.rs:32-51, src/bridge.rs:54-71, src/lifecycle.rs:64-80, src/lifecycle.rs:114-145, src/lib.rs:346-350
- problem: `serve` and `serve_unavailable` `tokio::spawn` one task per connection and drop the handle. `Lifecycle::stop_tasks` aborts the accept loop on quit, but in-flight `serve_connection` tasks keep running and can add or start downloads while `Manager::shutdown` is stopping them, so a capture that arrives during quit can be half-applied. There is also no cap on concurrent connections. `Lifecycle::keep` does not check `quitting`, so a task registered after quit began is never aborted. The two serve functions duplicate the same accept-and-backoff loop.
- fix: Give `serve` a `CancellationToken` (or `JoinSet`) owned by `Lifecycle`: cancel it before `manager.shutdown()`, and have `serve_connection` stop between requests when cancelled. Make `keep` abort the handle at once when `is_quitting()`. Extract one `accept_loop(listener, token, handler)` used by both serve variants, with a semaphore bounding concurrent connections.
- cross-package: yes (core: `serve_connection` should accept a cancellation token)

## DRS-11 Question and capture ids cross the IPC boundary as raw u64 while downloads and queues have newtypes
- severity: medium
- category: design
- location: src/commands.rs:163, src/commands.rs:173, src/commands.rs:240, src/commands.rs:249, src/commands.rs:259, src/commands.rs:310, src/questions.rs:25-27, src/questions.rs:70-86, src/download_windows.rs:113, src/download_windows.rs:161-162
- problem: `DownloadId` and `QueueId` are newtypes, but the id shared by captures, media choices, add-on requests and media readings is a bare `u64` everywhere (`confirm_capture(id: u64)`, `dismiss_media_choice(id: u64)` which also drops add-on requests, `Shows::Question(u64)`, `Attention::Ask(u64)`). Nothing stops passing a `DownloadId.0` or a JSON-RPC request id where a question id is expected; the window labels `download-q`/`download-d` exist precisely because the spaces are easy to confuse.
- fix: Add `pub struct QuestionId(pub u64)` to `wirecatch-protocol` (serde transparent, ts-rs as a branded number) and use it in `PendingCapture.id`, `MediaChoice.id`, `MediaReading.id`, `AddonRequestItem.id`, the `UiEvent` variants, the commands, `Shows` and `Registry`.
- cross-package: yes (protocol, core, packages/protocol, apps/desktop frontend)

## DRS-12 Internal errors in this library crate are Strings, against the thiserror-in-libraries rule
- severity: medium
- category: convention
- location: src/host.rs:31, src/host.rs:45, src/background.rs:184-208, src/tray.rs:69-83, src/updates.rs:38-52, src/bridge.rs:115-137, src/bridge.rs:170-202, src/appimage.rs:45-67, src/appimage.rs:72-120
- problem: `wirecatch_desktop_lib` is a library (the binary only calls `run()`), and CLAUDE.md requires `thiserror` in libraries, yet `Core`, `Launcher`, `apply_autostart`, `tray::perform`, `check_and_download`, `bridge_to_register`, `run_bridge`, `launcher_script` and `install_private_bridge` all return `Result<_, String>`. Callers cannot match on cause (for example retrying a transient update network error versus a bad signature), source chains are lost (`e.to_string()` drops `io::Error` kinds), and the user-facing wording is fixed deep in helpers instead of at the edge.
- fix: Add a `DesktopError` enum (`thiserror`) with variants such as `Updater(#[from] tauri_plugin_updater::Error)`, `Autostart(String)`, `BridgeMissing(PathBuf)`, `BridgeFailed { status, stderr }`, `Io { what, source }`, and format user wording once where it is shown (notification, tooltip, command error from DRS-1).
- cross-package: no

## DRS-13 Key shell behaviour has no tests: the sleeping main window, the window-label guards, and capability scope
- severity: medium
- category: testing
- location: src/sleep.rs:31-55, src/background.rs:58-67, src/commands.rs:284-307, src/window.rs:150-160, capabilities/default.json:5, src/host.rs:187-195
- problem: D13's central behaviour (a hidden main window is skipped by `emit_filter`, and waking it emits `RESYNC_EVENT` exactly once) has no test; tests/shell.rs only covers the awake case. `close_download_window` and `fit_download_window` refuse non-download windows, but no test calls them from `main`. `reveal_main` ignoring other labels and a main window hidden on purpose is untested. Nothing checks that `capabilities/default.json`'s `"main"` matches `MAIN_WINDOW` and the `tauri.conf.json` window label (only the `download-*` capability has such a test). `dev_endpoint` is only tested on names that match its patterns, not on the fallthrough (DRS-14).
- fix: In tests/shell.rs, add a mock-runtime test that hides the main window with `sleep::hide_main`, sends events, asserts the main window got none and a download window got them, then `show_main_window` and assert one resync. Add command tests invoking the two guarded commands through `tauri::test::get_ipc_response` from a `main` webview. Add a config test tying `MAIN_WINDOW` to both JSON files. Add the DRS-4 capability assertions.
- cross-package: no

## DRS-14 The debug endpoint is derived by string surgery on ipc's private naming and silently falls back to the real endpoint
- severity: low
- category: magic-value
- location: src/host.rs:187-195, crates/ipc/src/paths.rs:12, crates/ipc/src/paths.rs:20
- problem: `dev_endpoint` rewrites `app.sock` and `wirecatch-app-` in the endpoint string, values that are `pub(crate)` constants inside `wirecatch-ipc`. If ipc renames either, `replacen` is a no-op and debug builds bind the release endpoint, so a debug build next to an installed copy exits with "already running" (the bind guard prevents worse), and nothing fails at compile time.
- fix: Add `wirecatch_ipc::endpoint_for(variant: &str)` (or `endpoint_with_suffix`) and call it here; return an error rather than the unchanged endpoint when a variant cannot be formed.
- cross-package: yes (ipc)

## DRS-15 download_windows.rs mixes four concerns, the crate exposes almost every module publicly for tests, and production code carries test seams and dead code
- severity: low
- category: design
- location: src/download_windows.rs:56-102, src/download_windows.rs:159-306, src/download_windows.rs:308-339, src/download_windows.rs:441-568, src/lib.rs:95-109, src/background.rs:30-36, src/background.rs:157-170, src/lifecycle.rs:39-50
- problem: `download_windows.rs` (about 570 non-test lines) holds the pure `Registry` state machine, cascade geometry, Tauri window creation and focus handling, and Win32 z-order code. `lib.rs` makes 13 of 14 modules `pub` only so integration tests reach them, which turns internals into public API that needs doc comments and stability. `background::forward_events` is public and called nowhere. `ShownNotifications` and `Lifecycle::exit_hook` are test hooks compiled into release and consulted on every notification and quit.
- fix: Split into `download_windows/{registry.rs, placement.rs, create.rs, win32.rs}`. Make modules `pub(crate)` and expose a narrow `#[doc(hidden)] pub mod testing` (or a `test-support` feature, like `wirecatch-bridge/test-overrides`) for integration tests. Delete `forward_events`. Gate `ShownNotifications` and `on_exit` behind that feature, or inject a `Notifier`/`Exiter` trait object once at build time.
- cross-package: no

## DRS-16 Managed state is fetched with try_state and silently ignored when missing
- severity: low
- category: design
- location: src/download_windows.rs:356-364, src/window.rs:165-167, src/sleep.rs:31-33, src/sleep.rs:36-39, src/background.rs:164-177, src/lifecycle.rs:200-215
- problem: Seven separately managed states (`Core`, `Lifecycle`, `LaunchLinks`, `MainAsleep`, `DownloadWindows`, `PendingUpdate`, `ShownNotifications`) are read through `try_state` with a no-op fallback: a missing `DownloadWindows` makes every question window silently never open, a missing `LaunchLinks` drops command-line links, a missing `MainAsleep` makes the sleeping window receive events. A wiring mistake in `run()` or in a test builder becomes silent misbehaviour, and tests can pass vacuously.
- fix: Group the always-present shell state into one `Shell` struct managed once in `run()` and read with `app.state::<Shell>()`; keep `try_state` only for genuinely optional state (`Core` before setup, `PendingUpdate`).
- cross-package: no

## DRS-17 Async commands stat files and launch the shell on Tokio workers
- severity: low
- category: performance
- location: src/api.rs:226-230, src/api.rs:261, src/api.rs:268, src/api.rs:290-295, src/commands.rs:184-193, src/commands.rs:201-219
- problem: `file_to_open`, `download_is_program` and `item_to_reveal` call `is_file`, `is_dir` and `metadata` synchronously inside async commands, and `open_path`/`reveal_item_in_dir` spawn processes or COM calls on the same worker. Download folders on network shares are common for a download manager; a stat against an unreachable SMB share can block for many seconds and starve the runtime that also serves the bridge.
- fix: Wrap the filesystem part of these three helpers and the opener calls in `tauri::async_runtime::spawn_blocking`.
- cross-package: no

## DRS-18 CSP lacks base-uri, form-action and object-src, and the prototype is not frozen
- severity: low
- category: security
- location: tauri.conf.json:26-28
- problem: `default-src 'self'` does not cover `base-uri` or `form-action`, so an injected `<base>` or form could redirect relative loads or submissions; `object-src` relies on the default. `app.security.freezePrototype` is off, leaving prototype pollution available to any injected script that reaches the IPC bridge.
- fix: Use `default-src 'self'; script-src 'self'; img-src 'self' data:; style-src 'self' 'unsafe-inline'; font-src 'self' data:; connect-src ipc: http://ipc.localhost; object-src 'none'; base-uri 'none'; form-action 'none'; frame-ancestors 'none'` and set `"freezePrototype": true`.
- cross-package: no

## DRS-19 The site named in the "only open it if you trust ..." warning is parsed by hand
- severity: low
- category: security
- location: src/api.rs:242-252
- problem: `site_of` splits on `://`, `/?#` and `@` itself to name the host in a security prompt. It currently works only because core stores URLs normalised by `url::Url`; a URL that reaches the row by another path (an import, a refresh, a future source) with a backslash or odd userinfo would be named differently from how the browser resolved it. `url` is already a workspace dependency.
- fix: `Url::parse(url).ok().and_then(|u| u.host_str().map(str::to_owned))` with the port appended when present, falling back to the current wording.
- cross-package: no

## DRS-20 Start-at-sign-in state is read by reimplementing the autostart plugin's storage, with registry paths duplicated in NSIS
- severity: low
- category: duplication
- location: src/autostart.rs:68, src/autostart.rs:76, src/autostart.rs:91-97, windows/hooks.nsh:18-19, Cargo.toml:42-43
- problem: `read_state` hardcodes the `Run` and `StartupApproved\Run` registry keys and the Linux `$XDG_CONFIG_HOME/autostart/<name>.desktop` path, i.e. the private layout of `auto-launch` inside `tauri-plugin-autostart`, and pins `winreg` to that crate's version by comment. The same two registry paths are repeated in hooks.nsh. A plugin update that changes layout makes the app turn the user's setting off (it reads `Missing`/`TurnedOff` wrongly) with no compile error.
- fix: Keep the paths as named consts in one place (and template hooks.nsh from them, see DRS-6); add an integration test on each OS that enables via the plugin and asserts `read_state` sees `On`; prefer the plugin's `is_enabled()` for presence and read only `StartupApproved` yourself.
- cross-package: no

## DRS-21 The WebView2 unsafe block wraps safe code and its safety note is generic
- severity: low
- category: unsafe
- location: src/sleep.rs:83-118
- problem: The single `unsafe { ... }` spans the whole function body, including `cast`, the closure creation and the branch logic, so a future edit inside it gets no compiler help distinguishing safe from unsafe operations. The SAFETY note asserts "made on the window's own thread" but nothing in `set_asleep` enforces or debug-asserts it; it relies on every caller going through `with_webview`.
- fix: Scope `unsafe` to each COM call (`CoreWebView2`, `SetMemoryUsageTargetLevel`, `SetIsVisible`, `TrySuspend`, `Resume`) with a one-line SAFETY note each, and make `set_asleep` private to the closure in `with_webview` so it cannot be called from another thread. The two other unsafe sites (download_windows.rs:551-567, appimage.rs:102-103) are sound and adequately justified.
- cross-package: no

## DRS-22 Paths are converted lossily before being handed to the opener
- severity: low
- category: error-handling
- location: src/commands.rs:185-186, src/commands.rs:211-212
- problem: `open_path(path.to_string_lossy(), ...)` replaces non-UTF-8 bytes with U+FFFD, so on Linux a file whose name is not valid UTF-8 (common with downloads from legacy servers) opens a different, nonexistent path and the user gets a misleading "Could not open" error. `reveal_item_in_dir` already takes a `Path`.
- fix: Refuse explicitly when `path.to_str()` is `None` with a message telling the person to use Show in folder, or open through an API that takes `&Path` (for example `open::that_detached(path)`).
- cross-package: no

## DRS-23 Moving the data folder across drives blocks the main thread before the first window
- severity: low
- category: performance
- location: src/lib.rs:289, src/host.rs:120-136, src/host.rs:152-167
- problem: On the first start after the D21.5 change, `host::data_dir` runs in `setup` on the main thread and may copy the whole roaming data folder (database plus the media add-on, tens of MB) when the local and roaming folders are on different drives. Nothing shows during the copy, so the app appears not to start, and a second launch during that time is handed to a process with no event loop yet.
- fix: Keep the rename on the main thread (it is instant) but run the copy fallback in `spawn_blocking` before `Core::new`, showing the tray tooltip "Moving your downloads list..." meanwhile, or do the migration in the manager's background open.
- cross-package: no

## DRS-24 The integration tests carry a copy of the engine's local test server
- severity: low
- category: testing
- location: tests/common/mod.rs:1-2, tests/common/mod.rs:26-145
- problem: CLAUDE.md says tests needing a server use the local test server in `crates/engine/tests/common`; this crate keeps its own cut-down copy (range handling, throttling), so fixes to range or throttling behaviour there do not reach these tests.
- fix: Move the server into a small dev-only workspace crate (for example `crates/test-support`, `publish = false`) used by engine, core and desktop tests.
- cross-package: yes (engine, core)

## Summary
- 24 findings: 3 high, 10 medium, 11 low; no critical issues, and the three unsafe sites are sound.
- The biggest themes are the Tauri IPC boundary (string errors, a hand-mirrored contract outside D15, raw question ids, every command open to every window) and operability (no logs in release Windows builds, product identity and version scattered).
- Highest-leverage fix: generate typed commands, events and errors from `wirecatch-protocol` (DRS-1, 2, 11) and scope commands per window (DRS-4, 5).
