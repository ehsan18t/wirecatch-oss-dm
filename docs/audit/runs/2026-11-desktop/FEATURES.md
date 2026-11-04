# Features: desktop scope

What the desktop product does, for tracing a feature from its button to its code. Sources: `README.md`, `docs/ROADMAP.md`, `docs/DECISIONS.md` and the code. Each line: what it does, then where it leads. Interface paths are under `crates/desktop/src`; `api::` is `crates/shell/src/api.rs`, which calls `Manager` methods of the same name in `crates/core/src/manager`.

"Others:" lines say how established download managers handle a core feature, only as confirmed in the named source on 2026-11-04. A difference from them is not a finding. Sources are listed at the end.

## Downloading (engine)

- **Segmented download (core):** one file over several connections, re-split as parts finish. `wirecatch-engine`: `download::download`, `segment`, `worker`. Limits: engine `options::MAX_CONNECTIONS` 64; app setting `protocol::MAX_CONNECTIONS_PER_DOWNLOAD` 32, default 8; README says 64.
  - Others: IDM gives a new connection half of the largest remaining segment and reuses finished connections (S1). aria2 sets connections with `-x` (default 1) and `--split`, and does not split below `--min-split-size` (S2). JDownloader opens several connections per file, default 2, up to 20, mostly on premium accounts (S6, page dated 2016).
- **Server connection limits (core):** a connection refused twice, or told to wait long, hands back its segment while another is receiving (D69). `wirecatch-engine`: `worker::wait_or_step_back`, `retry`.
- **Resume (core):** continues after pause, crash or restart; starts over if the file changed on the server. `wirecatch-engine`: `state` (`can_resume`, `same_file`), `target` (`plan_in_folder`, `continues`), `http::range` (`check_partial`, `check_whole`).
  - Others: IDM resumes after dropped connections, shutdowns and power outages (S3). aria2 tracks progress in a `.aria2` control file beside the download, saved every 60 s by default and deleted on completion; `--always-resume` (default true) aborts when resume is impossible (S2). FDM resumes from where a download stopped (S5).
- **Retries (core):** pauses of 500 ms doubling to 10 s with jitter; `Retry-After` replaces the pause; a refusing proxy is not retried (D101, D102). `wirecatch-engine`: `retry`.
- **Protocols:** HTTP, HTTPS, FTP, FTPS, HTTP/3 as an option. `wirecatch-engine`: `http`, `ftp`, `http3`, `transport`.
- **Speed limits:** token buckets, changeable during a run, for the app and each queue. `wirecatch-engine`: `limiter`; `settings/speed.rs`; `wirecatch-shell`: `tray_speed`.
  - Others: aria2 has `--max-download-limit` and `--max-overall-download-limit` (S2). FDM offers traffic usage modes (S5).
- **Disk writes and finishing:** positional writes to a part file, then a rename that never replaces. `wirecatch-engine`: `storage` (`finalize`, `rename_no_replace`, `move_no_replace`), `writer`.
- **Local network guard:** a website's link reaches the local network only under the rules of D34 and D39. `wirecatch-engine`: `net_guard`; `wirecatch-sites`; setting `local_sites`.
- **Proxy in the engine:** system, none or manual; HTTP, HTTPS, SOCKS5. `wirecatch-engine`: `proxy`, `socks`.
- **Command line:** `wirecatch` downloads one link with the engine (folder, connections, headers, proxy). `crates/cli/src/main.rs`: `main`, `build_request`, `proxy_choice`.

## Download list and actions

- **Add a download:** New download or Ctrl+N. `add_dialog.rs` -> `api::add_download`, or `api::add_video_page` for a video page (D51).
- **Paste a link:** Ctrl+V in the window. `main_window` action `PasteLinks`, `links.rs`, then `add_dialog.rs`.
- **Add from a second launch:** a link on the command line reaches the running app (D103). `wirecatch-shell`: `launch::links_from_args`, `launch::hand_over`.
- **Pause, resume:** row, details, selection bar, menu, palette; `main_window/applies.rs` decides what applies. `api::pause`, `api::resume` -> core `downloads.rs`.
- **Pause all, resume all:** `main_window/list.rs`, palette, tray. `api::pause_all`, `api::resume_all`.
- **Cancel, remove:** with or without files, after a question. `main_window/confirm.rs` -> `api::cancel`, `api::remove` -> core `downloads.rs` (`remove_in_background`), `run.rs` (`delete_files`).
- **Try again:** `api::retry`, from `main_window/list.rs`, `row_menu.rs`, `details.rs`, `notices.rs`, `download_windows/view.rs`.
- **Refresh an expired link:** keeps what is downloaded. `main_window/refresh_dialog.rs` -> `api::refresh_link` -> core `downloads.rs`.
  - Others: IDM's "Refresh download address" opens the source page and picks up the new link; if that fails the download restarts (S4).
- **Move to another folder:** never replaces a file there. `main_window/fixes.rs`, row menu -> `api::set_download_folder` -> core `relocate.rs`.
- **Open, Show in folder:** a program asks first (D43.1). `files.rs` -> `api::file_to_open`, `api::item_to_reveal`.
- **Row menu, details, Copy link:** `main_window/row_menu.rs`, `main_window/details.rs`.
- **List, sort, search, select, keyboard:** one grid, finished by day (D115). `main_window/list.rs`, `sort.rs`, `selection.rs`, `keymap.rs` (the table of shortcuts), `command_bar.rs`.
- **Command palette:** Ctrl+Shift+P finds actions and settings. `main_window/palette.rs`.
- **Reorder within a queue:** `main_window/order.rs` -> `api::reorder`.
- **File already in the folder:** Ask, Replace, Keep existing, Keep both, for every kind of download (D88, D96). Setting `when_file_exists`; core `settle.rs`, `file_exists.rs`; `api::answer_file_exists`; `wirecatch-core::naming`.
  - Others: aria2 `--auto-file-renaming` (default true) appends a number; `--allow-overwrite` (default false) restarts from scratch (S2).
- **Category folders:** files sorted by type. `wirecatch-settings`: `folders`; core `preferences.rs` (`type_folder`); `settings/files.rs`.
- **Saved list and repair:** SQLite download list, repaired when damaged (D17); a failed save shows a notice. `wirecatch-store`; `wirecatch-records`; core `restore.rs`; `main_window/notices_bar.rs`.
- **Sign-ins forgotten after a week:** core `sign_in.rs` (`run_sign_in_expiry`).

## Queues and scheduling

- **Queues (core):** each with its own limit, connections, speed, proxy and folder; following the app by default (D72, D94). `main_window/sidebar.rs`, `queue_dialog.rs` -> `api::create_queue`, `api::update_queue` -> core `queues.rs`. Choosing what runs: `wirecatch-queues::plan`, core `scheduling.rs` (`reconcile`).
  - Others: IDM has several named queues, each with its own schedule and file list, one-time or periodic synchronization (S7). aria2 caps parallel items with `-j` (default 5) (S2).
- **Scheduling (core):** start time, optional stop time and weekdays per queue. `main_window/queue_dialog.rs`; `wirecatch-queues::schedule` (`is_open`, `allows`, `resume_run`); core `scheduling.rs`.
  - Others: IDM schedules a queue's start and stop, with Start Now, and can shut the computer down afterwards (S3, S7). FDM's scheduler starts and pauses downloads and launches applications at set times (S5).
- **Why a download waits:** every waiting download gives a reason. Core `scheduling.rs` (`wait_reason`, `update_wait_reasons`).
- **Start now, pause a queue:** `main_window/queue_header.rs`, `sidebar.rs` -> `api::start_queue_now`, `api::set_queue_paused`.
- **Move between queues:** by dragging or menu. `main_window/drag.rs`, `row_menu.rs` -> `api::move_to_queue`.
- **Order of queues:** `main_window/drag.rs` -> `api::set_queue_order`.
- **Delete a queue:** after typing its name; its downloads move to the default queue or leave the list. `main_window/queue_delete.rs` -> `api::delete_queue`.
- **Auto queues and rules:** a queue claims new downloads by site, file name, type or protocol (D72, D74). `main_window/queue_rules.rs`; `wirecatch-queues::rules`; core `placement.rs`.
- **Test a link:** which queue would take it. `main_window/link_trial.rs` -> `api::try_link`.
- **Queue proxy:** `main_window/queue_proxy.rs`; D73.
- **Told when a queue finishes:** per queue `notify` (D52). Core `batches.rs`; `wirecatch-shell`: `finished`, `notifications::queue_done`.

## Browser capture (app side; the extension is another scope)

- **Bridge (untrusted input):** native messaging host between the extension and the app. `wirecatch-bridge`: `relay`, `app`, `caller`, `raw_frame`, `install`; `wirecatch-shell`: `bridge`; `wirecatch-ipc`.
- **Protocol server:** one client connection answered by the manager (D14, D32). `wirecatch-server`: `serve_connection`, `handshake`, `handle`. Types: `wirecatch-protocol` (`extension`, `wire`, `frame`).
- **Capture of a browser download (core):** one download ends in exactly one place (D27). Core `captures.rs` (`add_capture`, `withdraw_capture`), `handovers.rs`.
  - Others: IDM captures through its "IDM integration module" browser extension (S3).
- **Download window:** a window for each captured download: name, folder, queue, Download now or later (D23, D114). `download_windows/view.rs` -> `api::confirm_capture`, `api::reject_capture`; `wirecatch-shell`: `download_windows`.
- **One window per link:** the same link brings its window to the front (D89). Core `captures.rs` (`known_handover`), `download_windows`.
- **Without a window:** starts with a brief notice (D63). Setting `show_progress_windows`; `wirecatch-shell`: `teller`.
- **Settings for the browser:** kept in the app, changed from the popup (D5, D61). Core `preferences.rs` (`browser_controls`, `update_browser_controls`); `settings/browser.rs`.
- **Cookie scope:** the app picks a capture's cookies with its own Public Suffix List, updated while running (D44, D112). Core `suffix_list.rs`; `wirecatch-sites`.
- **Browser registration:** per browser, on start, and for an AppImage (D19). `wirecatch-bridge::install`; `wirecatch-shell`: `bridge::register_on_start`, `appimage`.

## Video and audio

- **HLS and DASH (core):** parallel segments, encrypted HLS, joined without re-encoding. `wirecatch-streams`: `hls`, `dash`, `select`, `crypto`, `download`, `download::join`.
- **Media capture from pages (core):** a video from the browser or a page added in the app, read by yt-dlp. Core `media.rs` (`add_media_capture`, `choose_media`), `media_run/`; `wirecatch-media::ytdlp`.
  - Others: FDM uses add-ons for files and videos from supported sites and converts formats after download (S5). aria2's manual lists HTTP(S), FTP, SFTP, BitTorrent and Metalink, not streams (S2).
- **Quality choice:** in the download window, or saved choices without asking (D50). `download_windows/media_form.rs` -> `api::choose_media`; `wirecatch-media::choice`; core `media.rs` (`choose_without_asking`).
- **Formats and conversion:** MP4, MKV, WebM; MP3, M4A, Opus, FLAC, WAV; hardware encoders with fallback, warned beforehand (D24). `wirecatch-media`: `convert`, `merge`, `output::encoders`; `download_windows/media_output.rs`.
- **Subtitles, chapters, cover art:** embedded or separate (D93). `wirecatch-media`: `output::subtitles`, `output::metadata`; core `media_run/extras.rs`; `settings/media.rs`.
- **Finished video placement:** subtitles and file placed under a free name or the setting's answer. Core `media_run/placement.rs`.
- **Media add-on:** one-click yt-dlp, ffmpeg and Deno, pinned by SHA-256, yt-dlp updates signature-checked (D20, D21, D38, D80). `settings/addon.rs`, `media_form.rs` -> `api::install_addon`, `update_addon`, `remove_addon`, `check_addon_updates`, `cancel_addon_operation`; core `addons/`; `wirecatch-addons` (`archive`, `manifest`, `signature`).
- **Child processes:** yt-dlp and ffmpeg run as a tree the app stops. `wirecatch-streams`: `process::run`, `process::tree`.

## Failure handling

- **Errors with a fix (core):** a failure says what happened and offers a fix button (D3): Retry, RefreshLink, ChooseFolder, FreeDiskSpace, KeepOriginal, ProxySettings, ChangeQueue. `wirecatch-messages`: `describe`, `describe_run`; `protocol::app::FixAction`; `main_window/fixes.rs`; `wirecatch-media::describe`.
  - Others: aria2 has `--max-tries` (default 5) and `--retry-wait` (default 0; above 0 it also retries after HTTP 503), and `--max-file-not-found` (S2). IDM's error recovery restarts downloads broken by network problems (S3).
- **Keep the original:** when converting a video fails, saves it as downloaded (D24). `main_window/fixes.rs` -> `api::keep_original`.
- **Recovery at start:** restores unfinished downloads and completes those whose file finished. Core `restore.rs`, `startup.rs`.
- **Logs:** capped file, secrets redacted (D40). `wirecatch-logging`; logs and diagnostics in `settings/general.rs` (D87).

## Proxies

- **App proxy:** system, none or a saved profile, one for everything or per protocol, with login and hosts to reach directly (D71, D73). `settings/network.rs`, `main_window/proxy_dialog.rs` -> `api::create_proxy_server`, `update_proxy_server`, `delete_proxy_server`; core `manager/proxy.rs`; `wirecatch-settings::proxy`.
- **Copy from system, connection test:** `api::copy_system_proxy`, `api::test_proxy_server`.
- **Passwords:** kept apart from settings. `wirecatch-records`; `wirecatch-store` (`save_proxy_logins`).
- **Tray switch:** `wirecatch-shell`: `tray_proxy`.

## Settings

- **Pages:** General, Files, Speed, Network, Media, Browser, Notifications (D85), plus add-on and About (D109). `main_window/settings/`; `api::get_settings`, `api::change_settings`; core `preferences.rs`; checks in `wirecatch-settings`; fields in `protocol::app::AppSettings`.
- **Search in settings:** `settings/mod.rs`.
- **Export and import:** settings and queues, across systems, with a preview (D6). `main_window/backup_dialog.rs` -> `api::export_settings`, `preview_import`, `import_settings`; `wirecatch-settings::settings_file`.
- **Appearance:** theme, accent, clock, list lines. `theme.rs`; `wirecatch-shell`: `appearance`; `settings/general.rs`.

## App shell

- **One process, single instance:** the app hosts the manager and the bridge endpoint (D13). `wirecatch-shell`: `host`, `launch`; `backend.rs`.
- **Tray:** Open, New download, downloads folder, Pause all, Resume all, Queues, Speed limit, Proxy, Quit (D104). `tray.rs`; `wirecatch-shell`: `tray`, `tray_queues`, `tray_speed`, `tray_proxy`, `activity`.
- **Close to tray, quit:** `lifecycle.rs`, `closing.rs`; `wirecatch-shell`: `quit` (`quit_sequence`).
- **Start at sign-in:** `sign_in.rs`; `wirecatch-shell`: `autostart`.
- **Not run as administrator:** refused at start (D25). `wirecatch-shell`: `elevation`.
- **Notifications:** started, finished, failed, by system or browser notice, with Do Not Disturb rule (D52, D63, D64, D65, D67). `notices.rs`; `wirecatch-shell`: `teller`, `notifications`, `system_notifications`, `browser_notices`.
- **A notice opens its download:** main window at the queue, selected and highlighted (D98). `notices.rs`, `main_window`.
- **Download complete window:** `download_windows/finished.rs`; `wirecatch-shell`: `finished`.
- **Mark of the Web, private files:** `wirecatch-files`.
- **Windows and themes:** drawn with GPUI through the `ui/` library (D95, D99). `main_window/mod.rs`, `title_bar.rs`, `rail.rs`, `ui/`, `text.rs`.

## Updates and release tooling

- **App updates:** signed manifest from GitHub Releases, a minute after start and daily; `prerelease_updates` setting (D111). `app_update.rs` -> `api::check_app_update`, `fetch_app_update`; core `app_update.rs`; `wirecatch-updates` (`manifest`, `release`); `wirecatch-shell`: `updates::install`.
- **Public Suffix List updates:** setting `update_suffix_list`. `api::check_suffix_list`; core `suffix_list.rs`.
- **Installers:** `.exe`, `.deb`, `.rpm`, AppImage (D106, D110, D117). `scripts/package.mjs`, `crates/desktop/packaging/`, `scripts/notices/`.
- **Product identity:** generated from `config/product.toml` (D29). `crates/product`, `scripts/product/`.
- **Gates and CI:** `scripts/gate.mjs`, `scripts/gate/`, `scripts/ci/`, `.github/workflows/` (`ci`, `audit`, `outdated`, `release`); `scripts/release/manifest.mjs`.

## Sources for "Others"

Read on 2026-11-04. S1: IDM, Dynamic Segmentation and Performance, internetdownloadmanager.com/support/segmentation.html. S2: aria2c manual 1.37.0, aria2.github.io/manual/en/html/aria2c.html. S3: IDM features, internetdownloadmanager.com/features2.html. S4: IDM FAQ, internetdownloadmanager.com/register/new_faq/sites2_3.html and sites2_6.html (search excerpt). S5: FDM features, freedownloadmanager.org/features.htm. S6: JDownloader wiki, jdownloader.org/knowledge/wiki/glossary/chunkload. S7: IDM Scheduler, internetdownloadmanager.com/support/idm-scheduler/idm_scheduler.html.

Not confirmed from a project's own pages, so left out: JDownloader's resume, queues, scheduler, video and failure handling; IDM's HLS and DASH handling; FDM's retries and queues; aria2's scheduling.
