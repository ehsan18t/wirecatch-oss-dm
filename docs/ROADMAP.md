# Roadmap

## Phases

1. **Engine and CLI.** Segmented HTTP(S) downloads, dynamic splitting, resume, retries. Benchmarked locally. Done.
2. **Core, Bridge and Extension.** The download manager with SQLite state, the native messaging bridge, the WXT extension for Chromium and Firefox, settings sync (D5). Done.
3. **Desktop UI.** Tauri 2 app hosting the core: download list, queues in the sidebar, schedules, speed limits, command palette, settings export and import (D6), tray, single instance. Done.
4. **Streams.** HLS and DASH detection in the extension and parallel segment download with joining (`wirecatch-streams`). Done.
5. **Media add-on.** One-click yt-dlp, ffmpeg and Deno, pinned by SHA-256, yt-dlp updates verified by signature and a two-day delay (D20, D21). Done; smoke-tested end to end against a real Wikimedia Commons video.
6. **Remaining v1 protocols.** FTP and FTPS done; HTTP/3 available as an experimental option (A7).
7. **Release readiness.** Done: final security review and fixes, Linux suite green in Docker, quiet-machine benchmark, Windows installer built (5.2 MiB), commit history cleaned. Left for the maintainer: GitHub organization, code signing, the updater signing key, store listings for the extension (which also gives Chromium browsers a trusted extension id), and the user actions in D21.
8. **Interface revamp.** One design system for the desktop app and the extension, in two phases. Phase 1 done: the foundation, the main window, the add-download flow, the download window and the extension's pages. Phase 2 done: the settings, the dialogs, the rail, the media button, the installer's pictures and Copy diagnostics. See [REVAMP](REVAMP.md).

## Where things stand and what comes next

As of 2026-10-15 (main at e18e845f) the code quality audit has W0 to W5, W7, W8 and W14 done, and phase 1 of the interface revamp is on main. Since then queues that decide how their downloads run (D72) and proxy profiles chosen per queue (D73) have landed, on the queue rules crate and the run policy and queue choice of D74, followed by fixed labels with help behind an info icon (D84) and the settings regrouped into seven pages (D85). [AUDIT](AUDIT.md) has the status table and every carried item; this section is the order to take them in, which the maintainer may change.

1. The rest of W6: shared test support, CI, and the backlog of proposed gate checks listed under W6.
2. W9 (core manager), W10 (IPC, bridge and CLI) and W11 (desktop shell, with the gaps D43 records), then what is left of W12 (desktop interface) and W13 (extension, with D44.6, the cookie scope the app decides).
3. W15 last: comments only where they earn their place.

## Before the first release

W9 is finished before the release. Features that close a gap in the app come first when building them fixes W9 findings on the way; [AUDIT](AUDIT.md) W9 has each finding's state. In priority order:

1. **What a download does when its file already exists** (feature). A setting with three choices: replace the existing file, mark the download complete without downloading, or download it under a new name (`file (1).zip`, today's only behaviour). The check happens when the server answers, not when the download is added, which also closes core's half of ENG-11: a finished download never replaces a file another program saved under its name meanwhile.
2. **CORE-9:** moving a download to another folder never replaces a file there, and a move that cannot delete the original leaves no duplicate behind; core uses the engine's no-replace move.
3. **CORE-4:** withdrawing a browser capture while it is being confirmed never downloads it twice.
4. **Dropped downloads' file work:** a stopped run's queued file work cannot delete a new run's files (the drain handle in AUDIT's carried items).
5. **One state for a download (CORE-12), one kind of question (rest of CORE-5) and cancel safety (CORE-6):** the flags become one phase, which removes the class of bug behind 3 and 4.
6. **Saves and scans:** progress saves write only the progress (CORE-7), state changes stop scanning every download (CORE-10), and the system proxy is read off the manager's lock.
7. **The rest of W9:** split the long modules (CORE-11), typed add-on errors (CORE-13, CMD-8), no logic on a person's fix (CORE-14), named layout strings (CORE-19), no bool parameters (CORE-25), `UnixMillis` (CORE-26), the latest write per row kept (CORE-8), the small-file cap in the engine (CORE-23), and the small items AUDIT lists for W9.

Release setup, beside the list: the four `TODO-PRODUCT-` values in `config/product.toml` (the maintainer's), a Chrome Web Store package from the release workflow, and the store listings (ROADMAP item 7).

For the maintainer:

- Left for release, as before: the GitHub organization, code signing, the updater signing key and the extension store listings.

## Phase 1 acceptance checklist

Each item is observable from the CLI or the test suite.

1. `wirecatch <url>` downloads a file using several parallel connections, and the result is byte-identical to the source.
2. When a server does not support byte ranges, the download falls back to one connection and still completes correctly.
3. When a connection finishes its part, it takes over half of the largest remaining part (dynamic splitting), so no single slow tail holds up the end.
4. Interrupting a download (Ctrl+C or a killed process) and running the same command again resumes from where it stopped instead of starting over.
5. If the file changed on the server between runs (different size, ETag or Last-Modified), the download restarts cleanly instead of producing a corrupt file.
6. A dropped connection or a server error (5xx, 429, timeout) is retried with backoff, and the download still completes.
7. While downloading, data goes to `<name>.wcpart` next to a `<name>.wcstate` resume file; on success it is renamed to `<name>` and the state file is removed.
8. The file name comes from `Content-Disposition`, then the URL, and is made safe for Windows and Linux. An existing file is never overwritten; a free name like `file (1).zip` is used instead.
9. A local benchmark shows the speedup of several connections over one against a server that limits speed per connection.
10. The disk space for the file is reserved up front without writing it twice (sparse file on NTFS, `fallocate` on Linux).
