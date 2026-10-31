# Roadmap

## Phases

1. **Engine and CLI.** Segmented HTTP(S) downloads, dynamic splitting, resume, retries. Benchmarked locally. Done.
2. **Core, Bridge and Extension.** The download manager with SQLite state, the native messaging bridge, the WXT extension for Chromium and Firefox, settings sync (D5). Done.
3. **Desktop UI.** The desktop app hosting the core, first on Tauri 2 and now drawn in Rust on GPUI (D95, D107): download list, queues in the sidebar, schedules, speed limits, command palette, settings export and import (D6), tray, single instance. Done.
4. **Streams.** HLS and DASH detection in the extension and parallel segment download with joining (`wirecatch-streams`). Done.
5. **Media add-on.** One-click yt-dlp, ffmpeg and Deno, pinned by SHA-256, yt-dlp updates verified by signature and a two-day delay (D20, D21). Done; smoke-tested end to end against a real Wikimedia Commons video.
6. **Remaining v1 protocols.** FTP and FTPS done; HTTP/3 available as an experimental option (A7).
7. **Release readiness.** Done: final security review and fixes, Linux suite green in Docker, quiet-machine benchmark, Windows installer built (5.2 MiB), commit history cleaned. Left for the maintainer: GitHub organization, code signing, the updater signing key, store listings for the extension (which also gives Chromium browsers a trusted extension id), and the user actions in D21.
8. **Interface revamp.** One design system for the desktop app and the extension, in two phases. Phase 1 done: the foundation, the main window, the add-download flow, the download window and the extension's pages. Phase 2 done: the settings, the dialogs, the rail, the media button, the installer's pictures and Copy diagnostics. See [REVAMP](REVAMP.md).

## Where things stand and what comes next

As of 2026-10-31 the code quality audit has W0 to W5, W7, W8 and W14 done, most of W6, and W9's findings that could break a download (CORE-4 to CORE-7, CORE-9, CORE-10, CORE-12). The GPUI desktop app is on main (D95, D107), and so are queues that decide how their downloads run (D72), proxy profiles (D73) and queue folders (D94). [AUDIT](AUDIT.md) has the status table and every carried item. Of the seven items under "Before the first release", the About page and the updates are left; this list is the order after it, which the maintainer may change.

## After the first release

1. The rest of W6: shared test support, CI, and the backlog of proposed gate checks listed under W6.
2. W10 (IPC, bridge and CLI), W11 (desktop shell, with the gaps D43 records) and W13 (extension, with D44.6, the cookie scope the app decides).
3. The rest of W9 (CORE-8, CORE-11, CORE-13, CORE-14, CORE-19, CORE-23, CORE-25, CORE-26 and the small items). These improve the code without changing what a download does. W12's findings in the Svelte interface go with it (D95); any that still apply to the GPUI interface are carried over.
4. W15 last: comments only where they earn their place.

## Before the first release

Set with the maintainer on 2026-10-30. The release waits for the seven items below, in this order (reordered the same day so nothing is built twice for D95), and is delayed for them if need be; the rest of W9 does not (see "After the first release"). A bug found on the way is fixed whatever part of the app it is in.

1. **Notices that come back** (bug; done, D97). Notices the person closed show again a little later, after switching tabs or after a minute or so, and a notice from minutes ago can show a second time, such as a failure for a download that is no longer in the list. The cause is found and named first, with a test that fails; a closed notice never comes back, a notice about a download that is gone is not shown, and Clear all is always there while several notices show.
2. **Every download follows "If the file is already in the folder"** (feature; done, D96). The setting (Ask me, Replace the file, Keep the existing file, Keep both, with the question's "Remember my choice") covers every kind of download, however it arrives: a file added in the app, from the command line or captured from the browser (done, D88), and now also a stream (HLS or DASH) and a video or audio download read by the media add-on, which today always take the next free name. Those are checked when their job ends and their file is placed, since that is when the name is final (D88.5). The setting decides whenever a file of that name is in the folder, whatever the file is, not only for a likely duplicate (D96). The question says which it is: the same file again, or a different file with the same name.
3. **A notice opens the app at its download** (feature; done, D98; the notice already offers Open and Show in folder, D90). The main window's part is carried into the GPUI interface. Clicking a notice's text, in the browser or the system's notification, brings the main window up showing the queue the download is in, with the download selected, scrolled into view and highlighted for a moment, so the person sees at once where it is. The buttons keep doing what they say: Open opens the file, Show in folder shows it in its folder.
4. **Save to says who chose the folder** (change to D94.4; done, D94.6). The window's part is carried into the GPUI interface. A download window shows the folder the app chose, from the queue's own folder or else the folder for the file type, marked as the app's choice ("Chosen by the Overnight queue", "By file type"). Picking another folder makes it the person's choice, marked as such, with a way back to the app's; picking the folder the app already chose leaves it the app's choice. The person's choice wins over the queue's, as D94.2 says.
5. **The desktop interface in GPUI** (rebuild; done, D95, D106, D107: every screen, the installers, and the Tauri app removed). The desktop app's interface is rebuilt in Rust on GPUI with gpui-component, calling the core directly, and Tauri and the webview go. The look follows Zed's rather than copying today's screen for screen, keeping every screen, state and behaviour the decisions describe. It comes after the items whose work is mostly in the core, and before the About page and the updates, which are built for it rather than twice; the browser extension keeps its web interface and `packages/ui`.
6. **An About page** (feature; built in the GPUI interface). Settings gets an About page with the app's name and version, its licence, the maintainer, the repository, the website and where to report a problem, all read from `config/product.toml`, plus the versions of the media add-on's programs when installed.
7. **Updates from GitHub Releases** (feature; built for the GPUI app, since Tauri's updater and installer builder go with Tauri, D95). The app checks GitHub Releases for a newer version and offers to install it. A setting, "Get pre-release versions", off by default, also offers pre-releases; off, only full releases are offered. The release workflow builds every installer for Windows and Linux, on x64 and ARM64, names each file by one scheme (product, version, system, architecture), writes the update manifests for both channels, and drafts the GitHub release with all of it for the maintainer to publish. Needs the updater signing key, which is free to make and the maintainer's to keep (D19); code signing stays out of the first release.

Done before the release:

- **Queues save to their own folder** (feature; done, D94).
- **What a download does when its file already exists** (feature; done for files, D88; videos are item 4 above).
- **One download window per link** (bug; done, D89). Clicking the same link again while its download window is open brings that window to the front instead of opening a second one.
- **The in-page media button cannot start a video twice** (bug; done, D89). Clicking the button again for a video that is being read or whose window is open brings that window to the front; the button stays as it is. Today repeated clicks start several reads, and none gets past reading.
- **Toasts show above an open dialog** (bug; done). Dialogs open in the browser's top layer, so a toast raised while one is open (a proxy test that failed, say) sits under its blurred backdrop, where nobody can read it. The toasts move into the topmost open dialog while there is one, so they show above it, can be clicked and are announced.
- **Finished downloads are told in the browser** (bug; settled by D90: a system notification is the default, since the browser is rarely in front when a download finishes). With "Show a notice in the browser" chosen, a Windows notification shows instead even with the browser in front: the app waits one second (`NOTICE_ANSWER_WAIT_MS`) for the extension to confirm, and a first notice in a tab must wake the background, place the frame, load the notices page and answer through the bridge. Measured first, then fixed where the time goes. A browser not in front, or a tab no extension can draw in, still falls back by design (D65.5).
- **The browser notice offers Open** (amends D65.4; done, D90). Pressing it opens a document, picture, video or audio file directly; a program, a script or an unknown type opens only after the app's own confirmation window, which no website can cover, so a click a website tricked never runs a program. The setting's description stops promising "the same buttons" until it is true.
- **Settings and downloads switch at the sidebar's foot** (change to D47; done, D91). On the settings screen the foot of the sidebar shows "Downloads" where the downloads screen shows "Settings", and the "Back to downloads" item at the top goes; Escape leaves settings when its search is empty.
- **Where a video's subtitles go** (settings change; done, D93). "Save subtitles as separate files" becomes a choice: embed them in the video, or save them as separate files. With embedding chosen, a second choice says what happens when the format cannot hold subtitles (the "Original" format, or audio only): save them as separate files (today's behaviour) or leave them out. The quality question follows the same rule. The Media page is split into a Video group and an Audio group, so the labels drop "for videos" and "for audio" ("Subtitle languages"); "Save audio as" heads the Audio group and says that it converts.
- **The proxy list's buttons sit in its header** (layout; done). "Copy from system" and "New proxy" move from below the list to the "Proxy servers" header row, so they stay in one place however many servers there are, and an empty list reads as one line saying there are none yet.
- **The queue list's layout** (layout; done). "New queue" moves to the "Your queues" header row, as the proxy list's buttons do; "Test a link" stops being a panel inside the card and becomes its own row with a full-width field, a Try button as tall as the field, and its answer below; the queue and proxy lists share one list component, so their rows, buttons and empty state look and behave alike.
- **A modern title bar** (design; done, D92). New window buttons in place of the "wire" glyphs: small rounded buttons with plain Windows 11 shapes and no background at rest, the one under the pointer filled (grey, red for Close), as variant D of the title bar mockup; the search box keeps its shape; every part of the bar that is not a control drags the window and a double-click on it maximizes, since today the wrappers of the search box and of the button groups refuse a drag across a wide band; the search box shows no keyboard shortcuts; and the speed readout is hidden while nothing downloads instead of saying "Idle", without moving the rest of the bar when it comes and goes.
- **CORE-9 (done):** moving a download to another folder never replaces a file there, and a move that cannot delete the original leaves no duplicate behind; core uses the engine's no-replace move.
- **CORE-4 (done):** withdrawing a browser capture while it is being confirmed never downloads it twice.
- **Dropped downloads' file work (done):** a stopped run's queued file work cannot delete a new run's files (the drain handle in AUDIT's carried items).
- **One state for a download (CORE-12), one kind of question (rest of CORE-5) and cancel safety (CORE-6) (done):** the flags become one phase, which removes the class of bug behind 13 and 14.
- **Saves and scans (done):** progress saves write only the progress (CORE-7), state changes stop scanning every download (CORE-10), and the system proxy is read off the manager's lock.

Release setup, beside the list, for later: the four `TODO-PRODUCT-` values in `config/product.toml` (the maintainer's), a Chrome Web Store package from the release workflow, and the store listings (ROADMAP item 7).

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
