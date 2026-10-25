# Build plan

How the rest of v1 gets built: what each part is, who builds it, the contracts between parts, and how quality is checked. Decisions referenced as D1 to D18 are in [DECISIONS.md](DECISIONS.md).

## Where the work stands

Waves 0 to 3 are built and merged. Since then the work follows the code quality audit and the interface revamp, not new work packages: [AUDIT](AUDIT.md) has the status of every workstream and [ROADMAP](ROADMAP.md) the order for the next session, which starts with the flaky tests under W6 and goes on to phase 2 of the revamp ([REVAMP](REVAMP.md)). The rules below still hold for any work package: its own folders only, the gates, an independent review, and a fix round before the merge.

## How the work runs

Work is split into packages that touch separate folders, so several agents can build in parallel without stepping on each other. Each package runs in its own git worktree. When a package is done, a different agent reviews it against its acceptance checklist (the author never verifies their own work), fixes go back to the author, and only then is it merged.

```
Wave 0  (done)   Workspace, shared protocol, engine APIs the others need
Wave 1  (parallel)
        WP1 Core          crates/core, crates/queues, crates/server, crates/sites, crates/files, crates/addons, crates/records, crates/store
        WP2 Bridge + IPC  crates/bridge, crates/ipc
        WP3 Extension     apps/extension
        WP4 Desktop UI    apps/desktop        (runs against a mock backend)
        WP5 Engine        crates/engine, CI
        each followed by an independent review and a fix round
Wave 2  Integration: desktop hosts core, extension to bridge to app end to end
Wave 3  Packaging, installers, Linux verification, streams (HLS/DASH), media add-on
```

## Rules every package follows

- **Stay in your folder.** A package edits only its own folder. Shared files (`Cargo.toml` at the root, `package.json` at the root, `crates/protocol`, `docs/`) belong to the lead. Lock files may change; the lead reconciles them at merge.
- **Shared identities** (names, ids, host and folder names) live only under `config/` and are generated or read from there, never copied (ENGINEERING.md, section 1).
- **Contracts are fixed.** Types that cross a process boundary live in `wirecatch-protocol` (D15). A package that needs a change describes it in its report instead of editing the crate; the lead applies it between waves. Only WP1 may make additive changes there, and must list them.
- **Dependencies** shared by more than one crate go in `[workspace.dependencies]`; shared JavaScript tools go in the pnpm catalog. Versions are explicit.
- **Definition of done** is D18, plus the package's acceptance checklist below.

## Contracts already in place (Wave 0)

- `wirecatch-protocol`: `Message`, `Call`, `Reply`, `Event`, `ExtensionSettings`, `NewDownload`, `DownloadSummary`, `Queue`, `Schedule`, `AppSettings`, `SettingsProfile` and the framing helpers in `frame`. TypeScript twins in `@wirecatch/protocol`.
- `wirecatch-engine`: `download()`, `DownloadRequest` (including `exact_path` for manager-owned naming and link refresh), `DownloadOptions` (including `rate_limiters`), `RateLimiter`, `Progress`, `Outcome`, `Error::is_retryable`.

## WP1: Core (`crates/core`, `wirecatch-core`)

The download manager the desktop app hosts.

Builds: download lifecycle (add, start, pause, resume, cancel, remove, retry, refresh link, move between queues, reorder); queues with concurrency limits, schedules and speed limits; a global concurrency and speed limit; SQLite persistence with migrations; settings store with change notifications; settings export and import with portable paths; category rules that pick the folder; speed smoothing and ETA; mapping engine errors to `UserError` with a fix action; the local socket server speaking the protocol; a confirmation flow for `ask_before_download`; a protocol server that serves any async stream (`serve_connection`), so it is tested over in-memory streams and plugged into the real socket from `crates/ipc` in Wave 2; a headless example binary that runs the manager without any UI.

Acceptance checklist:
1. After a restart, every download is back with its status and progress; ones that were running continue on their own.
2. A queue never runs more downloads than its limit, and the global limit holds across queues.
3. A scheduled queue starts and stops inside its window (tested with an injected clock, not real time).
4. Queue and global speed limits hold together (one download obeys both).
5. Export then import on a "different machine" (different home folder) restores settings and queues with folders resolved correctly.
6. A protocol client that skips `hello`, sends a wrong version, an unknown method or an oversized frame gets a clear error and cannot crash the server.
7. A settings change from one client reaches every other connected client as an event.
8. An expired link produces `FixAction::RefreshLink`; refreshing keeps the downloaded bytes.

## WP2: Bridge and IPC (`crates/bridge`, `wirecatch-bridge`; `crates/ipc`, `wirecatch-ipc`)

The native messaging host the browser starts, and the local socket transport both the bridge and the app use.

Builds `wirecatch-ipc`: the per-user endpoint name, a listener that only the current user can connect to, and a connector that refuses a server owned by anyone else (protection against another local user squatting the pipe name). Builds the bridge: native messaging on stdin and stdout; caller origin check; connect to the app socket, starting the app with `--background` if needed; relay frames both ways; `install` and `uninstall` subcommands that register the host for Chrome, Chromium, Edge, Brave, Vivaldi, Opera and Firefox on Windows (per-user registry) and Linux (per-browser manifest folders).

Acceptance checklist:
1. A message from the extension reaches the app and the answer comes back unchanged; app events reach the extension.
2. If the app is not running, the bridge starts it and the first message still succeeds within 5 seconds; if it cannot, the extension gets `AppUnavailable` instead of silence.
3. A caller whose origin is not allowed is refused.
4. `install` writes correct manifests and registry entries for every listed browser; `uninstall` removes exactly what `install` wrote. Tested against a temporary home folder and a mocked registry.
5. Uses less than 10 MB of memory while idle and exits when the browser closes the connection.
6. The listener refuses other users (named pipe security descriptor on Windows, 0600 socket in a 0700 folder on Linux), and the connector refuses a pipe or socket owned by another user. Verified by reading the security descriptor back in a test.

## WP3: Extension (`apps/extension`, `@wirecatch/extension`)

WXT and Svelte 5, built for Chromium MV3 and Firefox MV3.

Builds: capturing browser downloads according to the settings (on/off, ignored sites, file types, minimum size); handing over URL, cookies, referrer and user agent; the popup with connection status, every D5 setting, "ignore this site", pause and resume all; live sync of settings through events, with a read-only cached copy when the app is unreachable; a "Download with Wirecatch" context menu item.

Acceptance checklist:
1. **No download is ever lost**: if the app does not confirm the handover, the browser keeps the download.
2. The decision of whether to capture a download is a pure function with unit tests covering every setting.
3. A setting changed in the popup shows up in the app, and a change made in the app shows up in an open popup without reopening it.
4. `pnpm build` produces loadable Chromium and Firefox builds; `pnpm check`, `pnpm lint` and `pnpm test` pass.
5. The popup works with the keyboard alone and reads correctly with a screen reader.

## WP4: Desktop UI (`apps/desktop`, `@wirecatch/desktop`)

Tauri 2, Svelte 5 with Vite (D36). In Wave 1 the UI talks to a mock backend behind one interface, so it can be designed, reviewed and tested in a normal browser; Wave 2 swaps in the real core.

Builds the D3 principles: queues in the sidebar with schedule and limits visible, drag downloads between queues; a virtualized list; paste a link anywhere to add it; the toast after a capture (D4) and the confirmation dialog when asked; a command palette (Ctrl+K at the time; Ctrl+Shift+P since D47); searchable plain-language settings with an Advanced section; errors with fix buttons; empty states that teach; light, dark and system themes; settings export and import screens.

Acceptance checklist:
1. Every feature is reachable from visible UI along the path that feels natural to the person, and from the command palette (D36 retired the earlier two-click limit). The report includes the feature-to-path table that proves it.
2. The list stays smooth at 10,000 downloads (measured in the report).
3. Everything works with the keyboard alone, with visible focus; contrast meets WCAG AA in both themes.
4. `pnpm dev:mock` runs the full UI in a browser with realistic fake downloads.
5. `pnpm check`, `pnpm lint`, `pnpm test` and `pnpm build` pass; the Tauri shell builds.

## WP5: Engine hardening and publishing (`crates/engine`, CI)

Builds: crates.io readiness (metadata, README, docs on every public item, examples, `cargo package` passing, no publish); honoring `Retry-After`; backing off from opening more connections when a server refuses them; sharing TLS session resumption across a download's connections; property tests for the segment table; a CI workflow running every D18 check on Windows and Ubuntu.

Acceptance checklist:
1. `cargo package -p wirecatch-engine` succeeds and `cargo doc` has no warnings with missing docs denied.
2. A server answering 429 or 503 with `Retry-After` is retried after that delay, not before.
3. A server that refuses connections beyond N still completes the download with N connections and no error.
4. Property tests show segments never overlap and always cover the file, across random split, claim and release sequences.
5. Benchmark numbers do not regress.

## Wave 2: Integration and the rest of v1

Wave 1 is merged (engine, protocol, core, ipc, bridge, extension, desktop UI). Wave 2 finishes v1 (D10) in two steps. Protocol additions for media and add-ons are made by the lead before 2a starts.

### Wave 2a (parallel)

**WP6 Desktop integration (`apps/desktop`).** The app hosts `wirecatch-core` (D13): every Tauri command calls the manager; `UiEvent`s reach the webview on `wirecatch://event`; the `wirecatch-ipc` listener serves the bridge with `serve_connection`. Tray icon with menu (open, pause all, resume all, quit) and live speed in the tooltip; closing the window destroys the webview and keeps the process in the tray when `close_to_tray` is on; `--background` starts with no window; single instance (a second launch focuses the window and passes a URL argument along); start with system; a notification when a download completes; native dialogs for folders and settings files; open file and open folder. On every start the app runs the bridge's per-user registration (D19). The bridge ships next to the app binary. Installer configuration: per-machine NSIS on Windows (D25, replacing per-user); deb, rpm and AppImage on Linux. UI for the media quality picker and the add-ons screen, built against the mock first. Polish: the queue header actions fit on one line, the toolbar shows labels when there is room.

**WP7 End-to-end tests (`crates/e2e`, test only).** Real bridge binary plus the core over a real `wirecatch-ipc` endpoint, driven by a scripted fake extension: a capture downloads from the local test server and completes; withdraw before and after the server answers; settings sync from extension to app and back; the bridge relaunching the app after it quits.

**WP8 Streams (`crates/streams`, `wirecatch-streams`).** HLS (master and media playlists, AES-128, byte ranges, live playlists refused with a clear message) and DASH (static MPD, segment templates and lists) downloaded with parallel segment fetching, the same origin-bound credential rules as the engine, resume after interruption, and an ffmpeg step (copy, never re-encode) that joins segments and audio and video tracks into one file. The ffmpeg binary path is passed in; finding or fetching it is the core's job.

**WP9 More protocols (`crates/engine`).** FTP and FTPS (explicit and implicit TLS, passive mode, resume with REST, parallel segments where the server allows several connections) and HTTP/3 used when a server advertises it, falling back to HTTP/2 or 1.1 without the user noticing. Same public API: the URL scheme picks the protocol.

**WP10 Extension media (`apps/extension`).** Detects HLS and DASH manifests and direct media on the page (observing requests, never modifying them), shows the download button over video and audio when `show_media_button` is on, and sends the media hint with the handover. On pages that need an extractor (for example YouTube), the button sends the page to the app, which explains that the media add-on is needed and offers to install it.

### Wave 2b (after 2a merges)

**WP11 Core media and add-ons (`crates/core`).** Add-on manager for the one media add-on (D20: yt-dlp, ffmpeg and Deno installed together with one click): install, update and remove from their official release sources, verified by checksum, kept in the per-user data folder, updated quietly (D2). Media flow: a stream or page capture produces a quality choice (`MediaChoice`) for the UI; the chosen option downloads through `wirecatch-streams` or, for yt-dlp pages, through yt-dlp's resolved format URLs with the engine doing the bytes. FTP downloads route through the engine. A `Processing` status while ffmpeg joins files.

**WP12 Wiring and polish (`apps/desktop`, `apps/extension`).** Connect the media picker and add-ons screen to the real core; final end-to-end pass of every D3 principle in the running app.

### Each package
Same rules as Wave 1: own folder only, D18, independent review, fix round, merge.

## Wave 3: Release readiness

Release build of the installers on Windows; Linux verification through CI (and a real distribution when available); benchmark rerun on a quiet machine; README, user guide and contributor guide; a final security review of everything that handles cookies, credentials or downloaded executables (add-ons). Commit history: done on 2026-10-11; every subject rewritten to Conventional Commits and every co-author trailer removed, with all 302 trees verified identical.
