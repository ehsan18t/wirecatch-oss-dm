# Roadmap

## Phases

1. **Engine and CLI.** Segmented HTTP(S) downloads, dynamic splitting, resume, retries. Benchmarked locally. Done.
2. **Core, Bridge and Extension.** The download manager with SQLite state, the native messaging bridge, the WXT extension for Chromium and Firefox, settings sync (D5). Done.
3. **Desktop UI.** Tauri 2 app hosting the core: download list, queues in the sidebar, schedules, speed limits, command palette, settings export and import (D6), tray, single instance. Done.
4. **Streams.** HLS and DASH detection in the extension and parallel segment download with joining (`wirecatch-streams`). Done.
5. **Media add-on.** One-click yt-dlp, ffmpeg and Deno, pinned by SHA-256, yt-dlp updates verified by signature and a two-day delay (D20, D21). Done; smoke-tested end to end against a real Wikimedia Commons video.
6. **Remaining v1 protocols.** FTP and FTPS done; HTTP/3 available as an experimental option (A7).
7. **Release readiness.** Done: final security review and fixes, Linux suite green in Docker, quiet-machine benchmark, Windows installer built (5.2 MiB), commit history cleaned. Left for the maintainer: GitHub organization, code signing, the updater signing key, store listings for the extension (which also gives Chromium browsers a trusted extension id), and the user actions in D21.
8. **Interface revamp.** One design system for the desktop app and the extension, in two phases. Phase 1 done: the foundation, the main window, the add-download flow, the download window and the extension's pages. Phase 2 rebuilds the rest after the maintainer's review and has not started. See [REVAMP](REVAMP.md).

## Where things stand and what comes next

As of 2026-10-15 (main at e18e845f) the code quality audit has W0 to W5, W7, W8 and W14 done, and phase 1 of the interface revamp is on main. [AUDIT](AUDIT.md) has the status table and every carried item; this section is the order to take them in, which the maintainer may change.

1. The flaky tests still open under W6 in AUDIT. The tests that bounded the clock from above, behind most CI failures that local runs did not show, are fixed and refused by the `test-timing` check; the stray part file behind the Windows-only `.wcpart` failures is fixed. Open: `a_resume_file_that_cannot_be_read_is_logged_with_the_reason` (engine, failed once) and `progress_events_are_batched` (core, a real-time window).
2. Phase 2 of the interface revamp ([REVAMP](REVAMP.md)): settings pages, the add-ons page, the remaining dialogs, the command palette, the in-page media button, the tray menu and installer pages, "Copy diagnostics" in the extension popup, the desktop's "already running" notice, then what phase 1 left open.
3. The rest of W6: shared test support, CI, and the backlog of proposed gate checks listed under W6.
4. W9 (core manager), W10 (IPC, bridge and CLI) and W11 (desktop shell, with the gaps D43 records), then what is left of W12 (desktop interface) and W13 (extension, with D44.6, the cookie scope the app decides).
5. W15 last: comments only where they earn their place.

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
