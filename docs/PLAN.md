# Build plan

How the rest of v1 gets built: what each part is, who builds it, the contracts between parts, and how quality is checked. Decisions referenced as D1 to D18 are in [DECISIONS.md](DECISIONS.md).

## How the work runs

Work is split into packages that touch separate folders, so several agents can build in parallel without stepping on each other. Each package runs in its own git worktree. When a package is done, a different agent reviews it against its acceptance checklist (the author never verifies their own work), fixes go back to the author, and only then is it merged.

```
Wave 0  (done)   Workspace, shared protocol, engine APIs the others need
Wave 1  (parallel)
        WP1 Core          crates/core
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
- **Shared identities** (extension ids, native host name) live in `config/extension-ids.json`; both the bridge and the extension read them from there.
- **Contracts are fixed.** Types that cross a process boundary live in `wirecatch-protocol` (D15). A package that needs a change describes it in its report instead of editing the crate; the lead applies it between waves. Only WP1 may make additive changes there, and must list them.
- **Dependencies** go in the package's own `Cargo.toml` or `package.json` with explicit versions.
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

Tauri 2, SvelteKit (static adapter), Svelte 5. In Wave 1 the UI talks to a mock backend behind one interface, so it can be designed, reviewed and tested in a normal browser; Wave 2 swaps in the real core.

Builds the D3 principles: queues in the sidebar with schedule and limits visible, drag downloads between queues; a virtualized list; paste a link anywhere to add it; the toast after a capture (D4) and the confirmation dialog when asked; a command palette (Ctrl+K); searchable plain-language settings with an Advanced section; errors with fix buttons; empty states that teach; light, dark and system themes; settings export and import screens.

Acceptance checklist:
1. Every feature is reachable from visible UI in at most two clicks, and from the command palette. The report includes the feature-to-path table that proves it.
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

## Wave 2: Integration

Desktop app hosts `wirecatch-core`, tray behavior and `--background`, single instance, start with system. End-to-end test: extension captures a download, bridge relays, app downloads, popup and app stay in sync. Reviewed against D3 to D6 as a whole.

## Wave 3

Installers (Windows NSIS or MSI, Linux formats per the open question) that register the bridge; Linux verification on a real distribution; HLS and DASH (Phase 4); the yt-dlp add-on (Phase 5); FTP and HTTP/3 (Phase 6).
