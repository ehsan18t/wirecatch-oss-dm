# Wirecatch: Decisions and Glossary

Running record of what has been settled. Each decision lists what it means in practice.

## Glossary

- **App**: the desktop program (engine plus UI) installed on the PC. Single source of truth for all settings and download state.
- **Engine**: the background Rust process that does the downloading. Keeps running when the window is closed.
- **Extension**: the browser add-on for Chromium-based and Firefox-based browsers. A remote control and capture layer for the App; it stores no settings of its own.
- **Bridge**: the small native messaging host the browser starts. Relays messages between the Extension and the Engine, and starts the Engine if it is not running.
- **Add-on**: an optional, separately installed component of the App (for example the yt-dlp media extractor, ffmpeg).
- **Extractor**: an Add-on that turns a page URL plus cookies into a list of downloadable formats (direct URLs plus required headers). The Engine downloads the bytes.
- **Queue**: a named group of downloads with its own schedule and concurrency limit. Shown in the sidebar; a default queue always exists.
- **Handoff**: the moment the Extension passes a captured browser download to the App.
- **Settings profile**: the exported file containing all portable settings.

## Decisions

### D1. Platforms and stack
Windows and Linux required; macOS later. Engine in Rust (tokio, hyper/reqwest, rustls), state in SQLite, UI in Tauri 2, Extension built with WXT (TypeScript) for Chromium MV3 and Firefox MV3.

### D2. Media extraction is an optional Add-on
The core App ships without yt-dlp, Deno or ffmpeg. The core handles normal files, direct media files and HLS/DASH streams the Extension detects. yt-dlp is installed in one click as an Extractor Add-on, verified by checksum and updated quietly. ffmpeg is fetched the first time a download needs muxing. When a site needs an Extractor that is not installed, the Extension says so and offers the install button.

### D3. UX principles
1. Every feature is visible on screen or at most two clicks away; nothing lives only in a context menu.
2. Queues are first-class in the sidebar with schedule and concurrency shown inline.
3. Smart defaults with no setup required; tuning knobs sit under Advanced.
4. Errors state the fix and offer it as a button.
5. Command palette (Ctrl+K) and searchable, plain-language settings.
6. Performance targets: window opens in under 200 ms, list stays smooth at 10,000 items, UI never blocks on the Engine.
7. Teach in context (empty states, one-time hints), no tours.

### D4. Handoff behavior
Default: start immediately and show a non-blocking toast with destination, Change and Open folder. "Ask before each download" is a setting, reachable from the toast, the Extension popup and the App settings.

### D5. Settings live in the App; the Extension edits them remotely
- The App is the only store. The Extension reads and writes settings through the Bridge; nothing is kept in browser storage except a cache for display.
- A change made in the Extension appears in the App immediately, and the reverse. Because every browser talks to the same App, a change made in Chrome also shows up in Firefox.
- The Bridge starts the Engine when needed, so the Extension does not have an "App not running" state in normal use.
- Settings exposed in the Extension popup (the relevant subset): capture on/off, ask before each download, show the media download button on videos, sites to ignore, file types to capture, minimum file size to capture, default queue, pause/resume all.
- Everything else (queues, schedules, connection tuning, folders, Add-ons) is App-only.

### D6. Settings import and export
- Export produces one versioned JSON file (Settings profile). Import applies it, migrating older versions forward.
- Included: all settings, queues and schedules, categories and folder rules, site rules, Extension settings, list of installed Add-ons (reinstalled on import).
- Paths are stored portably, for example `{Downloads}/Video`, so a profile exported on Windows imports cleanly on Linux.
- Excluded by default: download history and saved site credentials. Credentials live in the OS keychain and are only exported if the user ticks an option, in which case the file is encrypted with a passphrase.

### D7. Open source
The whole project (Engine, App, Bridge, Extension) is open source and developed in public. Build and release pipelines must be reproducible from the public repository. Add-ons we distribute (yt-dlp, ffmpeg) are fetched from their official sources rather than rebuilt and rehosted.

### D8. License: GPL-3.0 for everything
Engine, App, Bridge and Extension are all GPL-3.0. Forks that ship must publish source. The project name will be protected separately (trademark) so official builds can be told apart from repackaged ones. yt-dlp and ffmpeg run as separate processes, so no license conflict.

### D9. Name: Wirecatch
Checked 2026-10-10. Free on crates.io, npm and as a GitHub name; wirecatch.com, .app and .dev unregistered. WIPO Global Brand Database (about 77 million records, 89 sources, includes US, EU, UK): the only exact match is a Toshiba mark in Japan, class 11, expired 1995. Similar live marks:
- WIREKASH, UK00004271032, class 9 (collaboration software, software applications, e-commerce software), registered 2025-12-19. Closest in sound and spelling; a possible objection if we file in the UK.
- VERICATCH, registered in US, EU, UK and Canada, classes 9, 35, 42, but for seafood traceability software. Unrelated goods.
- WIREWATCH, Spain, class 9 (electrical equipment), plus a pending Brazil class 42 application. Different word.

Not covered: Bangladesh (DPDT is not in the WIPO database). Needs a separate search before filing there. This was a screening search, not a legal clearance opinion.

### D10. Protocol scope for v1
In scope: HTTP and HTTPS (HTTP/1.1, HTTP/2, HTTP/3), FTP and FTPS, HLS and DASH streams. Out of scope: BitTorrent and magnet links. Build order inside v1 follows the roadmap: HTTP(S) first, since it carries almost all browser downloads.

### D11. Build order
Phase 1 is the Engine plus a CLI, with no UI or Extension yet. See [ROADMAP.md](ROADMAP.md) for the phases and the Phase 1 acceptance checklist.

### D12. Monorepo layout
One repository, two workspaces side by side: a Cargo workspace (`crates/*`, plus `apps/desktop/src-tauri`) and a pnpm workspace (`apps/*`, `packages/*`).

| Path | Package | What it is |
|---|---|---|
| `crates/engine` | `wirecatch-engine` | Download engine library. Published to crates.io. |
| `crates/protocol` | `wirecatch-protocol` | Every message and data type shared across components. |
| `crates/core` | `wirecatch-core` | Download manager: queues, scheduling, persistence, settings, local socket server. |
| `crates/bridge` | `wirecatch-bridge` | Native messaging host the browser starts. |
| `crates/cli` | `wirecatch-cli` | The `wirecatch` command-line tool. |
| `apps/desktop` | `@wirecatch/desktop` | Tauri 2 desktop app. |
| `apps/extension` | `@wirecatch/extension` | Browser extension for Chromium and Firefox. |
| `packages/protocol` | `@wirecatch/protocol` | TypeScript types generated from `wirecatch-protocol`. |

### D13. One app process hosts everything
The desktop app process hosts `wirecatch-core` and the Engine. Closing the window destroys the webview (freeing its memory) and leaves the process in the tray; `--background` starts it with no window. There is no separate daemon, so there is one source of truth and nothing to keep in sync. A webview crash cannot take downloads down, because WebView2 and WebKitGTK render in separate processes. The CLI stays standalone and uses the Engine directly.

### D14. Local connections
The Bridge talks to the app over a local socket: a named pipe on Windows, a Unix socket in `$XDG_RUNTIME_DIR` on Linux. Only the current user can connect (explicit pipe security descriptor on Windows, mode 0600 on Linux). Frames are a 4-byte little-endian length plus UTF-8 JSON, capped at 1 MiB, the same as browser native messaging, so the Bridge uses one codec on both sides. The first call on a connection must be `hello`; a protocol version mismatch is refused with a clear error. The Bridge only accepts callers listed in its allowed extension origins.

### D15. Shared types
`wirecatch-protocol` is the single source of truth for every type that crosses a process boundary. TypeScript types are generated from it with `ts-rs` into `packages/protocol` (`pnpm protocol:generate`). Generated files are committed, and CI fails if they are stale.

### D16. Frontend stack
Svelte 5 (runes) and strict TypeScript everywhere. Desktop: SvelteKit with the static adapter inside Tauri 2. Extension: WXT with its Svelte module. pnpm workspaces, Vitest, svelte-check, ESLint and Prettier.

### D17. Persistence
SQLite (`rusqlite`, bundled) in the per-user app data folder, with versioned migrations. Cookies and headers needed to resume a download are stored with it; the database file is readable only by the current user.

### D18. Quality bar (definition of done)
A change is done only when all of these hold:
1. `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings` and `cargo test` pass for every touched crate.
2. `pnpm check`, `pnpm lint`, `pnpm test` and `pnpm build` pass for every touched app or package.
3. New behavior has tests that would fail without it; bug fixes add a regression test.
4. Every public Rust item has a doc comment. No `unwrap()` on input that comes from outside the process.
5. User-facing text follows the house style (no em dashes, plain language, errors say how to fix them).
6. An independent reviewer, not the author, has checked the work against its acceptance checklist.

### D19. Packaging, browser registration and updates
Decided by the lead while the user was away; easy to revisit before the first release.
- Windows: per-user NSIS installer from the Tauri bundler (no administrator rights needed).
- Linux: `.deb`, `.rpm` and AppImage from the Tauri bundler. Flatpak waits, because sandboxed browsers and native messaging do not mix well yet.
- Browser registration: the app runs the Bridge's per-user `install` on every start (it is idempotent), so installers, AppImages and portable copies all register themselves and repair a broken registration. Uninstall runs `uninstall`.
- Updates: `tauri-plugin-updater` with signed update manifests on GitHub Releases, switched on once the GitHub organization and signing key exist. Until then the updater is compiled in but disabled.

### D20. One-click media add-on (yt-dlp, ffmpeg and a JavaScript runtime)
Decided by the user on 2026-10-11. ffmpeg is not bundled with the app (the static LGPL build alone is 134 MB). It ships inside one optional "Media add-on" together with yt-dlp and the JavaScript runtime yt-dlp needs for some sites (Deno). One click installs all of it; the app offers it at the moment it is first needed (a stream whose video and audio must be joined, or a page such as YouTube that needs extraction), and it also lives in Settings, Add-ons. Every component is pinned to an exact release and verified by SHA-256 before use (A8 lists the sources). Plain files and single-track streams never need it. A later improvement replaces the general ffmpeg build with a small remux-only LGPL build made in CI, which cuts the add-on download from about 60 MB to a few MB.

## Assumptions (not yet confirmed)

- A1. No cloud sync of settings in v1; export/import covers moving PCs.
- A2. Target user is the mainstream user who wants it to just work, with power features discoverable rather than hidden.
- A3. License identifier is `GPL-3.0-only` (not "or later"). Easy to change before the first outside contribution.
- A4. SFTP is not part of "FTP" for v1; it can be added later.
- A5. Firefox add-on id is `extension@wirecatch.app` and the Chromium development build uses a fixed key so its extension id is stable. Store ids replace these at publication.
- A6. Saved site passwords are not exported in v1 (D6 encrypted export comes later).
- A7. HTTP/3 ships as an experimental option, off by default. For bulk downloads, several TCP connections already fill the link and QUIC costs more CPU; it turns on by default once the benchmark shows it winning. Decided by the lead.
- A8. The ffmpeg part of the media add-on is fetched from the builds the ffmpeg project links to (for example BtbN's GitHub releases for Windows and Linux), verified by checksum; yt-dlp from its official GitHub releases. Decided by the lead; the exact sources are recorded in the add-on manager.

## Open questions

- None blocking. GitHub organization, domains and code-signing certificate are the user's to set up (see D9, D19).

## Rejected names (for reference)

- Ferry: live USPTO registration FERRY (serial 97014250, class 9 downloadable software); crate, domains and GitHub name taken.
- Tidepull, Downpull, Haulstar: viable runners-up (Downpull too descriptive to protect well; Haulstar used by trucking companies).
- Plain English words (Braid, Magpie, Snag, Swoop, Riptide, Flotilla, Quiver and others): taken everywhere.
