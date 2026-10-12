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
Superseded in part by D23: a captured download now opens its own window first by default. With that window turned off ("Start downloads without asking"), a file starts immediately and shows a non-blocking toast with destination, Change and Open folder, as decided here. The setting is reachable from the download window, the toast, the Extension popup and the App settings.

### D5. Settings live in the App; the Extension edits them remotely
- The App is the only store. The Extension reads and writes settings through the Bridge; nothing is kept in browser storage except a cache for display.
- A change made in the Extension appears in the App immediately, and the reverse. Because every browser talks to the same App, a change made in Chrome also shows up in Firefox.
- The Bridge starts the Engine when needed, so the Extension does not have an "App not running" state in normal use.
- Settings exposed in the Extension popup (the relevant subset): capture on/off, show a window before each download (D23), show the media download button on videos, sites to ignore, file types to capture, minimum file size to capture, default queue, pause/resume all.
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
| `crates/core` | `wirecatch-core` | Download manager: queues, scheduling, persistence, settings, media, add-ons. |
| `crates/streams` | `wirecatch-streams` | HLS and DASH downloads. |
| `crates/ipc` | `wirecatch-ipc` | Per-user local endpoint between the app, bridge and CLI. |
| `crates/bridge` | `wirecatch-bridge` | Native messaging host the browser starts. |
| `crates/cli` | `wirecatch-cli` | The `wirecatch` command-line tool. |
| `crates/e2e` | `wirecatch-e2e` | End-to-end tests over the real bridge, endpoint and manager. |
| `apps/desktop` | `@wirecatch/desktop` | Tauri 2 desktop app. |
| `apps/extension` | `@wirecatch/extension` | Browser extension for Chromium and Firefox. |
| `packages/protocol` | `@wirecatch/protocol` | TypeScript types generated from `wirecatch-protocol`. |
| `packages/eslint-config` | `@wirecatch/eslint-config` | The one ESLint configuration. |

### D13. One app process hosts everything
The desktop app process hosts `wirecatch-core` and the Engine. Closing the window hides it and leaves the process in the tray, so it opens again at once from the tray (about 40 ms, measured). Destroying it instead freed its memory but made every reopen take 1.5 to 2 seconds behind a blank white window, which the maintainer rejected. While hidden on Windows, the WebView2 is suspended like a sleeping Edge tab and asked to keep its memory low, and core events are not sent to it (it reloads its list when it wakes); measured, its processes hold about 25 MB in RAM once settled, against about 330 MB open. WebKitGTK has no such switch, so on Linux the window is only hidden. A new main window opens hidden and shows itself once its first view is drawn, so it never flashes blank. `--background` starts the process with no window. There is no separate daemon, so there is one source of truth and nothing to keep in sync. A webview crash cannot take downloads down, because WebView2 and WebKitGTK render in separate processes. The CLI stays standalone and uses the Engine directly.

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
1. `pnpm gate` passes: every check, from `scripts/gate.mjs`, the same checks CI runs.
2. The change follows [ENGINEERING.md](ENGINEERING.md), including the rules only a reviewer can check.
3. New behavior has tests that would fail without it; bug fixes add a regression test.
4. Every public Rust item has a doc comment. No `unwrap()` on input that comes from outside the process.
5. User-facing text follows the house style (no em dashes, plain language, errors say how to fix them).
6. An independent reviewer, not the author, has checked the work against its acceptance checklist.

### D19. Packaging, browser registration and updates
Decided by the lead while the user was away; easy to revisit before the first release.
- Windows: per-user NSIS installer from the Tauri bundler (no administrator rights needed). Replaced by D25: per-machine, in Program Files.
- Linux: `.deb`, `.rpm` and AppImage from the Tauri bundler. Flatpak waits, because sandboxed browsers and native messaging do not mix well yet.
- Browser registration: the app runs the Bridge's per-user `install` on every start (it is idempotent), so installers, AppImages and portable copies all register themselves and repair a broken registration. Uninstall runs `uninstall`.
- Updates: `tauri-plugin-updater` with signed update manifests on GitHub Releases, switched on once the GitHub organization and signing key exist. Until then the updater is compiled in but disabled.

### D20. One-click media add-on (yt-dlp, ffmpeg and a JavaScript runtime)
Decided by the user on 2026-10-11. ffmpeg is not bundled with the app (the static LGPL build alone is 134 MB). It ships inside one optional "Media add-on" together with yt-dlp and the JavaScript runtime yt-dlp needs for some sites (Deno). One click installs all of it; the app offers it at the moment it is first needed (a stream whose video and audio must be joined, or a page such as YouTube that needs extraction), and it also lives in Settings, Add-ons. Every component is pinned to an exact release and verified by SHA-256 before use (A8 lists the sources). Plain files and single-track streams never need it. A later improvement replaces the general ffmpeg build with a small remux-only LGPL build made in CI, which cuts the add-on download from about 60 MB to a few MB.

### D21. Security decisions from the final review
Decided by the lead on 2026-10-11 after the cross-component security review.
1. Media add-on updates verify yt-dlp's OpenPGP signature on `SHA2-256SUMS` against a fingerprint pinned in the app (pure-Rust `pgp` crate, verification only, in `wirecatch-core`). The 48-hour maturity delay applies to every install path (quiet update, Update button, repair, reinstall), not just quiet updates.
2. Program types (exe, msi, scripts, installers and similar) stay in the browser by default, so the browser's Safe Browsing and download warnings apply. Downloads the browser flags as dangerous are never taken over.
3. ffmpeg only ever reads local files of the formats we expect: explicit input format, `-protocol_whitelist file` and a format whitelist on every input. Pinned add-on sources must be long-lived; a remux-only ffmpeg built by our own CI and published with our releases replaces the general build once the GitHub organization exists.
4. CI keeps actions pinned by SHA and gets Dependabot for actions, Cargo and npm, plus `cargo audit`. The Dependabot part is superseded by D28.
5. The app's data folder moves to the local (non-roaming) app data folder, so cookies and add-on programs never follow roaming profiles.
6. Superseded by D22: every bridge build trusts the one Chromium id that follows from our extension key, and the `dev-extension` feature is gone.

Actions only the user can take: reserve the Firefox add-on id `extension@wirecatch.app` on addons.mozilla.org (an unlisted upload is enough) and register the `wirecatch.app` domain, so nobody else can claim them (see D9).

### D22. One Chromium extension id for every build
Decided by the user on 2026-10-11, after the release build refused the extension with "Access to the specified native messaging host is forbidden" because release bridges listed no Chromium id. The lead agreed after weighing the risk.
1. `chdgnmpjoipbgfeldicipccjjghmagki`, the id that follows from the public key in `config/extension-ids.json` (`chromium_id`, `chromium_public_key`), is the one Chromium id for development and store builds. Every bridge build trusts exactly that id and the Firefox add-on id; no cargo feature changes this. `install` writes `allowed_origins: ["chrome-extension://chdgnmpjoipbgfeldicipccjjghmagki/"]` for every Chromium browser, and the bridge still checks the caller's origin by exact match.
2. Every Chromium build of the extension carries the public key in its manifest, so an unpacked load gets this id. The package for the Chrome Web Store (`pnpm build:store`) leaves the key out, because the store does not accept it.
3. The private key is kept by the maintainer in a private place outside the repository, never in the repository or CI. It is used once: on the first Chrome Web Store upload, as `key.pem` at the root of the zip, so the store gives the item this same id. Later uploads never include it (`apps/extension/PUBLISHING.md`).
4. Residual risk, accepted: no store can give this id to anyone else, and a packed extension with it needs the private key. Another extension can claim the id only by being loaded unpacked in Developer mode, in a browser profile that does not already have ours. Such an extension could do comparable harm with its own permissions anyway, so trusting the id in release bridges adds little risk and makes development and store builds work with any installed Wirecatch.
5. Other Chromium stores (for example Microsoft Edge Add-ons) give their own ids, which the bridge does not trust; Edge users install from the Chrome Web Store.

### D23. A window for each download
Decided by the maintainer on 2026-10-11, after a question about a browser download (confirm a file, pick a video quality) was drawn inside the main window, which was never brought forward, so it sat behind the browser and nothing seemed to happen. It works like IDM now, and replaces D4's "start immediately with a toast" default.
1. A download caught from the browser opens its own small window, in front of the browser and focused, whether the main window is open, hidden in the tray or closed. The main window is not brought forward. Windows open always on top and drop it the first time they lose the focus after having had it, so they then behave like normal windows (some Wayland compositors ignore always on top; that is accepted).
2. A file's window shows its name (editable), size or "Size not known yet", Save to with Change, Queue, the site it came from, and Download now (main button), Download later and Cancel. A video's window shows that the video is being read, then its qualities, name, folder and queue in the same window; when the media add-on is missing, its install offer shows there and the download continues after the install.
3. "Start downloads without asking" in the file window turns the window off for files; it is `ask_before_download`, inverted. Its default is now on, and a database migration switches a stored setting on once, because the default changed before the first release while stored settings still held the old default nobody chose. Videos always ask for a quality.
4. After Download now the same window becomes the download's progress window: name, status and why it waits, progress, done and total, speed, time left, connections in use, any error with its fix, media stages, Pause or Resume, Cancel (asks first, then removes the download and its partial file) and Hide (closes the window; the download continues). Once finished it offers Open and Show in folder and closes by itself after about five seconds unless the pointer is over it or it has the keyboard focus. "Close this window when downloads start" sets `show_progress_windows` (default on: the window stays), also in Settings, the settings profile and the command palette.
5. One window per download, each about 28 px down and right of the previous open one. At most five windows ask a question at once; later questions open as earlier windows close or start showing progress. A progress window is the same window continuing, so it keeps its place.
6. The main window never asks these questions itself. Its sidebar shows "Waiting for you" with how many downloads wait for an answer, and clicking it (or the command palette action) reopens their windows. Closing a question window with its X or Escape does not discard the download, since the browser has already let go of it: it stays waiting and counted. Only Cancel discards it. A question answered anywhere never comes back when a window opens or reloads, and one put aside does not open again by itself.
7. Download later adds the download to its queue paused; Resume (or Resume all) starts it.
8. Downloads added inside the app (Add download, paste, drop) open no window. Every row offers "Show progress window".
9. When the download being asked about is already in the list (the same address; for a video the same page, or the same video read from another link to it), the window says so in one line, for example "You downloaded this today at 18:38.", without blocking anything. Two captures of one video are asked about once.

### D24. Output formats for media
Decided by the maintainer on 2026-10-11, so a video can be saved in the format a person's player or editor needs, with its subtitles and chapters, without a separate converter. Applies to pages read with yt-dlp; direct media files and HLS or DASH streams stay as they come.
1. Under the quality list the window offers "Save as": Original (the site's own format, the default, so nobody's downloads change until they choose), MP4, MKV or WebM for a video, and MP3, M4A, Opus, FLAC or WAV when Audio only is picked. MP3, M4A and Opus also offer an audio quality (MP3 128 to 320 kbps, M4A 128 to 256, Opus 96 to 192); M4A and Opus can keep the downloaded audio as it is. Notes say when a bit rate is higher than the download's (it adds no quality) and when FLAC or WAV is made from lossy audio.
2. Every combination is offered. For each quality and format the core takes the streams of that height (and frame rate) that fit the container as they are: MP4 takes H.264 and H.265 with AAC, MP3, AC-3 or E-AC-3 (VP9 and AV1 in MP4 are valid but many players refuse them, and MP4 is chosen for those players); WebM takes VP8, VP9 and AV1 with Opus or Vorbis; MKV takes anything, so it always copies. Only a quality with no fitting stream is re-encoded, and each row says "No conversion" or "Converts" for the chosen format, with a note before the start. Audio re-encoding (AAC for MP4, Opus for WebM) is quick and always allowed.
3. Video is re-encoded to H.264 (MP4, MKV) or VP9 (WebM), at about the source's bit rate, raised when the source codec is more efficient (VP9 and H.265 by 1.4, AV1 by 1.8), or a usual rate for the height when the site gives none. Hardware encoders are tried first, in this order: NVIDIA NVENC, Intel Quick Sync, AMD AMF, then Media Foundation on Windows or VA-API on Linux. Each is tested once per ffmpeg by encoding a short generated clip, and the first that works is remembered while the app runs; one that fails on a real video is skipped from then on and the conversion goes on with the next. The media add-on's ffmpeg (BtbN's LGPL build on Windows and on Linux) has no libx264, so the software encoder is OpenH264 for H.264 and libvpx for VP9; an ffmpeg that has libx264 uses it instead of OpenH264.
4. Subtitles: the window lists the languages the video has, human-written ones first and auto-generated ones after, marked and never ticked unless asked for (machine translations into other languages are left out). MKV gets SubRip tracks, MP4 mov_text and WebM WebVTT, each named and labelled with its language; audio formats, and videos when the person asks, get `.srt` files next to the file. Subtitle files and the cover picture are downloaded by the engine under the same local network rule as the video, and checked to be the format they claim before ffmpeg reads them.
5. Chapters go into MP4, MKV, WebM and M4A through an ffmpeg metadata file; audio formats get title, artist and the video's picture where the format holds them (MP3, M4A, FLAC; tags only for Opus).
6. Every ffmpeg input, including subtitles, the metadata file and the picture, is a local file read only as its named format (`-protocol_whitelist file`, a format whitelist, one absolute `file:` path); text from the site goes into the metadata file, escaped, never into ffmpeg's options.
7. Conversion runs after the download, in the `.wcmedia` work folder, as its own "Converting" step with progress read from ffmpeg's `-progress` against the video's length; the file appears only when complete. Pause and Cancel kill ffmpeg, and a paused or failed conversion starts again from the downloaded parts without reading the page or downloading anything. When converting fails, the download is kept and the error offers Try again (converts again) and Keep the original, which saves the downloaded streams as they came.
8. The window starts from the last choices, kept separately for videos and for audio only (format, audio quality, subtitle languages and whether auto-generated ones are wanted, subtitle files, chapters). They are in Settings, Media, and in the settings profile; older settings and profiles read as Original, so the profile format version is unchanged.

### D25. Installed for every user, never run as administrator
Decided by the maintainer on 2026-10-12, replacing D19's per-user Windows installer.
1. Windows installs per machine, in Program Files, like other desktop download managers. Installing takes one administrator prompt, and so does each update, since it replaces files there; starting, signing in and booting never do. Silent updates through an update service may come later.
2. The installer registers the browser bridge for every user of the computer: `wirecatch-bridge install --all-users` writes the manifests next to the bridge in Program Files (only administrators can change them) and the keys under `HKEY_LOCAL_MACHINE`. The uninstaller removes them. The app's own per-user registration on each start (D19) steps aside when that machine registration already covers its bridge, because an uninstall can only clean up the account that runs it; portable copies and Linux keep registering per user.
3. What belongs to a person stays per user: the download list, settings and cookies in each user's `%LOCALAPPDATA%`, and start at sign-in. One user's cookies are never readable by another.
4. Wirecatch never runs as administrator. A release build started that way says it does not need administrator rights and exits, because an elevated app could be driven by any program of the same user, and the unelevated browser extension could not reach it anyway. Only a start the person elevated ("Run as administrator") is refused: on an account that always has administrator rights, with user account control turned off or as the built-in Administrator, every program runs elevated, the browser included, so the app starts there. Whatever starts it, its endpoint is labelled at its own integrity level, so unelevated programs cannot open an elevated app's pipe. The installer starts the app after installing as the signed-in user, not elevated.
5. Linux is unchanged: `.deb` and `.rpm` already install system-wide, and the AppImage is per user by nature.
6. A Wirecatch the person closed stays closed until something needs it, as with other download managers. The browser extension starts it only for something the person did: a browser download it will take, the media button, or Open Wirecatch in its popup. Its checks on waking and its reconnects send `hello` with `start_app` off, so the bridge only reaches an app that is already running and otherwise answers `app_not_running`, and the extension then waits instead of retrying. This makes D5's "the bridge starts the app when needed" exact.

### D26. The published engine ships its library, and is tested in the workspace
Decided by the lead on 2026-10-12, during W14.
1. The engine's tests and examples share test servers with the other crates through `crates/test-support`, which is not published: one copy of each server instead of one per crate. So the engine's tests cannot be built from the published package.
2. The package therefore holds the library only (`include` in `crates/engine/Cargo.toml`): no `tests/`, no `examples/`. Its tests run in this repository, where CI runs them on Windows and Linux. A packager who wants to run them builds from the repository at the release tag. The in-source test modules (`src/ftp/tests.rs`, `src/http3/tests.rs`) stay in the package, since the library declares them, and also need the repository to build. The gate checks that the package holds no tests or examples, so a change that ships uncompilable files fails.
3. `wirecatch_engine::numeric` is public: `approx_f64` and `f64_to_u64`, the checked conversions every crate uses for byte counts, rates and floats read from outside the process (W14 decision 4), so they are written and reasoned about once. `saturating_cast` joins them for conversions between whole-number types where clamping to the nearest bound is the honest answer (a list's length in a protocol's `u32`, a byte count in SQLite's signed integers); both of its types are sealed to the built-in whole-number types, so the published API promises nothing about other types.
4. `wirecatch_engine::units` is public: `KIB`, `MIB` and `GIB` as byte counts (`u64`), and `KIB_USIZE` and `MIB_USIZE` as lengths in memory. The engine is the lowest crate that the crates measuring bytes already depend on (core, streams, the desktop app), so the units are defined once there rather than once per crate. Only the crates that do not depend on the engine spell sizes out: the protocol (which must not depend on it), the bridge, test-support (which the engine's own tests use) and the end-to-end tests.
5. `wirecatch_engine::url` is public with one function, `without_credentials`, which takes the user name and password out of a URL before it is passed on, logged or stored. The engine's fetcher and HTTP requests use it, and the core needs the same rule for the Mark of the Web; one function keeps the rule identical on both sides. Its neighbour `without_password`, which keeps the user name and serves the engine's error messages about links and its saved resume state, stays inside the engine.

### D27. No browser download is lost or duplicated in a handover
Decided during the extension's W14 review: a download the extension takes from the browser ends in exactly one place, the app or the browser. When that cannot be known, the browser keeps its copy, because two copies beat none, and the person is told what to check.
1. `capture/handover.ts`: each download is paused first, under a handover id the hub chose and saved beforehand. When the app has it, the browser copy is cancelled only if it still reads as paused or cannot be read, and erased only once it reads as interrupted after the cancel. A copy the person resumed or that finished before the cancel is left alone, and one whose state cannot be read after the cancel may have finished first; both get the two-copies warning. A copy read as paused that the browser would not cancel stays paused in its list, and the person is told they can remove it; one whose state could not be read and that would not cancel may be running, so it gets the two-copies warning.
2. On a timeout, a lost connection or an answer the extension cannot read, `withdraw_download` decides: `already_started` means the app has it; `withdrawn` or `not_found` means it does not; a withdraw that fails, or whose answer cannot be read, means the app may have it. Whenever the app does not have it or may have it, the browser copy is resumed. If resuming fails, a copy found running counts as resumed, one found paused is `stuck_paused`, one whose state cannot be read is `maybe_paused`, and one that finished, was interrupted or is no longer listed is gone.
3. `hub/notice-board.ts`: what the person is told about a kept download depends on its browser copy. A gone copy gets no notice. A `stuck_paused` or `maybe_paused` copy gets an urgent notice that it is paused, or may be paused, with how to resume it; when the app may have the download, the notice also says so and asks the person to check Wirecatch's list before resuming. A running copy the app may have gets an urgent warning that it may be downloading twice. A running copy the app does not have gets the reason it stayed in the browser, for the reasons worth telling; for an answer the extension could not read, that it could not be read and that updating the extension may help.
4. `hub/duplicates.ts` and `hub/notice-board.ts`: a copy the app may have is matched with the app's late answer to the same handover, whichever comes first, and what the person is told is decided from the copy's state when the answer arrives, since the person may have resumed or paused it since. A copy the browser lists as paused gets, for a sure confirmation, a notice that the app took it and the paused copy is left over in the browser's list; for an answer that cannot be read, it keeps the notice to resume it, which already asks the person to check Wirecatch's list, or gets the possible two-copies warning when it has no such notice (the person resumed it and paused it again). A copy that is interrupted or no longer listed gets nothing new. Any other copy, running, finished or one the browser will not describe, gets the two-copies warning: that the app took it after all for a sure confirmation, that the app may have it for an answer that cannot be read. A state read while the browser reported the download moving is read again, so what is said matches the copy as it is.
5. Notices form a short list in `storage.session`, newest first, at most one per browser download, each dismissable. A notice is urgent when it asks the person to act (resume a download, or check for two copies); when the list is full, the oldest non-urgent notices go first. A notice asking to resume a download clears when the browser reports the download running, finished, interrupted or erased (`downloads.onChanged` and `downloads.onErased`, in `capture/register.ts`), and on load when the browser says it is no longer paused. Every browser answer is in before stored notices are filtered, so one dismissed, replaced or resolved during the load does not come back.
6. `hub/hub.ts` and `hub/recovery.ts`: each handover is saved in `storage.session` with its id before pausing and stays saved until settled. After a restart, every handover whose browser copy is not interrupted or gone is settled by `withdraw_download` as after a timeout, even when the browser's state cannot be read, and the wait allows a full hello so the app can start. The same notices follow, with two differences: a copy resumed because the app does not have it (`browser_only`) gets no notice, where the live path says why the download stayed in the browser, for the reasons worth telling; and nothing is matched with late answers, since one could only come on the previous run's connections.
7. `native/client.ts`: `start()` connects only when there is no port and no retry pending, so a port with requests on it is never replaced. An answer that cannot be read fails its request at once as `unreadable`; a late one counts only as a possible confirmation, and an error answer that cannot be read drops the port and is never taken as one.
8. Tests: `tests/handover.test.ts` and `tests/native-client.test.ts` cover each case against the real hub, handover and client; `tests/hub-parts.test.ts`, `tests/register.test.ts` and `tests/popup.test.ts` cover the notice list.
9. Known limit: the app keeps handover ids in memory only. If it restarted after taking a download, `withdraw_download` answers `not_found`, so the browser resumes its copy while the app may restore its own: two copies with no warning. Saving handover ids in the app would close this.

### D28. Outdated dependencies are reported, never proposed as pull requests
Decided by the user on 2026-10-12, after closing the five pull requests Dependabot had opened.
1. Nothing opens pull requests or issues for dependency updates. `.github/dependabot.yml` is gone, so Dependabot version updates are off for actions, Cargo and npm.
2. `.github/workflows/outdated.yml` runs `scripts/outdated.mjs` every Monday and on demand. It writes one report to the run's summary: the direct dependencies of every Cargo workspace crate with a newer compatible or incompatible version (from `cargo update --dry-run --verbose` and `cargo metadata`, so no extra tool is installed), `pnpm outdated --recursive`, and the pinned actions whose repository has a newer release. An outdated dependency never fails the run; only a part of the report that could not be produced does.
3. The maintainer reads the report and updates on purpose, one reviewed commit at a time, with its changelog read and its code changes made by cause.
4. Advisories are unchanged: `cargo deny` and `npm audit` in the gate on every push, and weekly in `audit.yml`. GitHub's separate "Dependabot security updates" repository setting also opens pull requests; only the maintainer can turn it off, in the repository's settings.

### D30. The engine's durability, proxy reading, public errors and identity (W7)
Decided during W7 (engine audit), 2026-10-12.
1. Resume survives a power cut, not only a killed process. Each checkpoint reads the ranges that are done, flushes the part file to the disk (`sync_data`), then writes the resume file through a flushed temporary file and a rename; the finished file is flushed before its final rename. The resume file therefore never counts bytes the disk has not received (ENG-7). The cost is one flush per checkpoint interval and one at the end: on the local benchmark the uncapped 256 MiB runs, which end before the first checkpoint, went from about 0.2 s to about 0.6 s, which is the time the disk needs to receive the file; capped runs, which are what real downloads look like, did not change. A flush at the end is the price of a completed file that is really complete; narrowing the promise to "after the process is killed" was rejected.
2. Proxy settings are read in one place (`proxy.rs`) the way the HTTP client reads them, and `NO_PROXY` is matched by the client's own matcher (`hyper_util::client::proxy::matcher`, already built through reqwest), never by a second parser. A host that `NO_PROXY` exempts may use HTTP/3 and may be downloaded over FTP while a proxy is set, because the HTTP client reaches it directly too (ENG-15).
3. `Error::Io` names the action (`IoAction`) and the path, `Error` no longer converts from `io::Error`, `Error::State` is gone (an unreadable resume file is set aside and logged, never reported), and `Error::Stalled` holds a `Duration` (ENG-10, ENG-16). The `fetch` module is private and `naming::from_content_disposition` is crate-private; `header` is re-exported from the `http` crate (ENG-17). These are breaking changes for the next minor release of the pre-1.0 crate; the crate has no changelog, so the commits' `BREAKING CHANGE:` footers are the record.
4. `PathResolver` keeps returning `Result<PathBuf, String>`. The text is the caller's own message for a person, which the engine only carries to `Error::TargetRejected`; the alternative the audit offered, `Box<dyn Error>`, is equally ruled out for library code by ENGINEERING section 4, and a type parameter would make `DownloadRequest` generic for one callback.
5. The published engine names itself, never the app: its default `User-Agent` is `wirecatch-engine/<crate version>` and its messages do not say "Wirecatch" (XC-14). The engine depends on no product crate; the app sets its own `User-Agent` from the product configuration.
6. `naming::unique_path` stays public for now because the streams crate uses it; it goes private, or becomes a reserving API, with the name reservation of ENG-11.

## Assumptions (not yet confirmed)

- A1. No cloud sync of settings in v1; export/import covers moving PCs.
- A2. Target user is the mainstream user who wants it to just work, with power features discoverable rather than hidden.
- A3. License identifier is `GPL-3.0-only` (not "or later"). Easy to change before the first outside contribution.
- A4. SFTP is not part of "FTP" for v1; it can be added later.
- A5. Firefox add-on id is `extension@wirecatch.app`. The Chromium id is `chdgnmpjoipbgfeldicipccjjghmagki` for every build, development and store alike (D22).
- A6. Saved site passwords are not exported in v1 (D6 encrypted export comes later).
- A7. HTTP/3 ships as an experimental option, off by default. For bulk downloads, several TCP connections already fill the link and QUIC costs more CPU; it turns on by default once the benchmark shows it winning. Decided by the lead.
- A8. The ffmpeg part of the media add-on is fetched from the builds the ffmpeg project links to (for example BtbN's GitHub releases for Windows and Linux), verified by checksum; yt-dlp from its official GitHub releases. Decided by the lead; the exact sources are recorded in the add-on manager.

## Open questions

- None blocking. GitHub organization, domains and code-signing certificate are the user's to set up (see D9, D19).

## Rejected names (for reference)

- Ferry: live USPTO registration FERRY (serial 97014250, class 9 downloadable software); crate, domains and GitHub name taken.
- Tidepull, Downpull, Haulstar: viable runners-up (Downpull too descriptive to protect well; Haulstar used by trucking companies).
- Plain English words (Braid, Magpie, Snag, Swoop, Riptide, Flotilla, Quiver and others): taken everywhere.
