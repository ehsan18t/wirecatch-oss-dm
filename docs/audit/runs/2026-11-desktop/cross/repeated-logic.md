# Cross-module flow: repeated-logic

Started from the `jscpd` results in `tools.md`, then searched for what crates decide for themselves: URL and host checks, retry and backoff, size and speed formatting, name cleaning, process start and kill, file replacement. Suspected copies were read whole; the rest of each crate was searched, not reviewed.

## Files read

docs/audit/AUDIT-PLAN.md
docs/audit/runs/2026-11-desktop/tools.md (Duplicates)
docs/audit/runs/2026-11-desktop/CHUNK-PLAN.md (Bar corrections)
docs/audit/runs/2026-11-desktop/contracts/01.md to 33.md
crates/addons/src/archive.rs (search)
crates/bridge/src/app.rs (`spawn_detached`)
crates/bridge/src/install/host.rs (`files`)
crates/bridge/src/install/plan.rs (`Layout`)
crates/core/src/manager/downloads.rs (`parse_url`, `parse_web_url`)
crates/core/src/manager/media.rs (`title_from_url`, the name in `choose_media`)
crates/core/src/manager/media_run/network.rs
crates/core/src/manager/media_run/placement.rs (from `waiting_to_place`)
crates/core/src/manager/media_run/tests.rs (`parts_get_media_extensions`)
crates/core/src/manager/preferences.rs (settings export)
crates/core/src/manager/proxy.rs (`updater_proxy`)
crates/core/src/manager/relocate.rs (`move_file`)
crates/core/src/naming.rs
crates/core/src/proxy.rs
crates/desktop/src/backend.rs (`config`)
crates/desktop/src/download_windows/media_form.rs (search)
crates/desktop/src/lifecycle.rs (`start_again`)
crates/desktop/src/links.rs
crates/desktop/src/main_window/settings/browser.rs, speed.rs (search)
crates/desktop/src/text.rs (`link_for_display`, `host`)
crates/engine/src/naming.rs
crates/engine/src/proxy.rs
crates/engine/src/state.rs (`save`)
crates/engine/src/url.rs (search)
crates/files/src/whole_file.rs
crates/ipc/src/key.rs (`renew`, `write_private`)
crates/ipc/src/paths.rs
crates/logging/src/lib.rs (folders)
crates/logging/src/redact.rs (`url_for_display`, `hide_link_credentials`)
crates/media/src/lib.rs (`safe_ext`, `file_name`)
crates/media/src/output/subtitles.rs (`clean_name`)
crates/media/src/ytdlp/cookies.rs (`netscape`)
crates/media/src/ytdlp/mod.rs (`cookies_first`)
crates/protocol/src/extension.rs (`read_site`, `host_of`, `normalize_file_type`)
crates/queues/src/rules.rs (`file_type_rule`, `checked`)
crates/queues/src/sites.rs
crates/settings/src/lib.rs (`category_of`, `clean_local_sites`)
crates/settings/src/proxy.rs (`clean_hosts`, from `engine_choice`)
crates/shell/src/api.rs (`file_type`, `is_program`, `site_of`)
crates/shell/src/appimage.rs (the write beside the copy)
crates/shell/src/bridge.rs (`run_bridge`)
crates/shell/src/host.rs (`migrate`, `merge`, `copy_then_delete`)
crates/shell/src/launch.rs (`LINK_SCHEMES`)
crates/shell/src/updates.rs (`run_setup`, `replace_appimage`)
crates/sites/src/cookie_scope.rs (`site_of`)
crates/sites/src/local_network.rs (from `is_local_by_itself`)
crates/sites/src/video_sites.rs (`video_site_for`, `on_site`)
crates/streams/src/download/join.rs (`move_to_free_name`)
crates/streams/src/extension.rs
crates/streams/src/model.rs (`with_extension`)

## Findings

### Xrepeated-logic-1 [P2] The app's own list of link schemes leaves out `ftpes`, so such a link cannot be added from any window

- Where: `crates/desktop/src/links.rs`, `LINK`; against `crates/engine/src/url.rs`, `WEB_SCHEMES` and `FTP_SCHEMES`, and `crates/shell/src/launch.rs`, `LINK_SCHEMES`
- Category: duplicate
- Failure: `ftpes://files.example.com/a.iso` is typed or pasted in the Add dialog, the command bar or the refresh dialog, or passed on the command line. `extract` finds no link, and the Add dialog counts the line as one without a link. The download is never added, though the manager and the engine support the link.
- Evidence: the schemes are written by hand in three places with two contents. The regex is `(?:https?|ftps?)://`, which does not match `ftpes://`. `LINK_SCHEMES` lists all five, and `core::parse_url` takes all five through `is_supported`. `Launch::from_args` therefore accepts a link that `add_dialog` then drops at `links::extract`. `crates/desktop` already depends on `wirecatch-engine`, which owns the list. `links::extract` is the only way in for `add_dialog`, `command_bar`, `main_window/mod.rs`, `refresh_dialog` and `tray`.
- Confidence: certain

### Xrepeated-logic-2 [P2] Replacing a finished file's extension is written twice, and only the streams copy spares a name's own words

- Where: `crates/core/src/manager/media_run/placement.rs`, `final_path`; against `crates/streams/src/model.rs`, `with_extension`
- Category: duplicate
- Failure: a person types `Episode 2. The end` as the file name in the quality window. The file written is `media.mp4`, so `final_path` gives `Episode 2.mp4` and the words after the dot are lost.
- Evidence: `with_extension` replaces only what `extension::is_replaceable` knows and otherwise appends the new extension, with the test `output_extension_is_replaced_only_when_it_is_a_media_one`. `final_path` calls `Path::with_extension` whenever the planned extension differs, whatever it is. `choose_media` passes the typed name as it is, under the comment "A typed name keeps its words". `parts_get_media_extensions` tests only `Invoice`-style names. Contract 17 lists the behaviour as 17-3; this finding adds the copy that already holds the right rule.
- Confidence: certain

### Xrepeated-logic-3 [P3] Whether a host is covered by a site is written separately in four places, which differ on trailing dots, case and addresses

- Where: `crates/queues/src/sites.rs`, `SitePattern::covers` and `is_under`; `crates/sites/src/video_sites.rs`, `on_site`; `crates/sites/src/local_network.rs`, `is_local_by_itself`, `is_listed`, `is_under` and `address_in`; `crates/media/src/ytdlp/mod.rs`, `cookies_first`
- Category: duplicate
- Failure: three cases go the wrong way.
  - A queue rule `example.com` does not cover `https://example.com./a.zip`, so the download goes to another queue.
  - A queue rule `10.0.0.0/8` covers `ftp://10.0.0.5/a` but not `ftps://10.0.0.5/a`, and `example.com` does not cover `ftps://FILES.Example.com/a`.
  - `video_site_for` does not recognise `https://youtube.com./watch?v=1`, so the Add dialog does not treat it as a video page.
- Evidence: `local_network`, `cookie_scope` and `cookies_first` trim the trailing dot from `host_str`, while `covers` and `video_site_for` do not, although `read_site` strips it from the stored patterns. `covers` trusts `Url::host` to return `Host::Ipv4` and a lower-case name, which contract 09 says is not so for `ftps` and `ftpes`, whose hosts are opaque; `address_in` parses the host's text instead and so sees the address. Each file's comments state its own assumption ("both are lower case"). Nothing shared exists: a host fix must be made in each of the four.
- Confidence: likely (the `ftps` behaviour comes from contract 09 and the code; no test was run)

### Xrepeated-logic-4 [P3] Placing a finished video has its own move that never replaces, weaker than the engine's

- Where: `crates/core/src/manager/media_run/placement.rs`, `place`; against `crates/engine/src/naming.rs`, `rename_no_replace`, and `crates/core/src/manager/relocate.rs`, `move_file`
- Category: duplicate
- Failure: a video finishes on a FAT or exFAT drive, or `hard_link` fails for any other reason, while another program saves a file under the same name. The check passes and then `std::fs::rename` replaces that file, which is lost.
- Evidence: `place` tries `hard_link`, and on any error other than `AlreadyExists` it renames after `is_free_on_disk`. The engine's `rename_no_replace` is one step on these drives: `MoveFileExW` without the replace flag, and `RENAME_NOREPLACE` on Linux, whose documentation names FAT32 and exFAT. It is public, and `relocate::move_file` already uses it. Its check-then-rename fallback remains only for file systems with neither that flag nor hard links.
- Confidence: certain (the window is short)

### Xrepeated-logic-5 [P3] A file type with a dot is accepted in a category rule and never matches, though queue rules honour it

- Where: `crates/settings/src/lib.rs`, `category_of` and `clean_extensions`; against `crates/queues/src/rules.rs`, `file_type_rule`
- Category: duplicate
- Failure: a person adds `tar.gz` to a category's file types. It is saved, but `backup.tar.gz` is compared as `gz` only, so it goes to whichever category lists `gz`, or to Other.
- Evidence: both lists are kept by `normalize_file_type`, which keeps `tar.gz`. `file_type_rule` matches by the end of the name, with the longest rule winning, and has a test for `tar.gz`. `category_of` takes the text after the last dot and looks it up exactly.
- Confidence: certain

### Xrepeated-logic-6 [P3] Two writers go around `whole_file::replace` and lose what it guarantees

- Where: `crates/bridge/src/install/host.rs`, `files::write`; `crates/core/src/manager/preferences.rs`, the settings export; against `crates/files/src/whole_file.rs`, `replace`
- Category: duplicate
- Failure: two cases.
  - The export writes over the chosen file in place with `std::fs::write`, so a full disk or a crash during an export over an earlier one loses the earlier one.
  - The bridge's `write` renames without flushing the temporary file, so a power cut after a rewrite can leave an empty or partial native messaging manifest. The browser then cannot start the bridge until the app next starts and registers it again.
- Evidence: `files::write` is documented in `Host::write_file` as replacing "atomically", yet has no `sync`. `whole_file::replace`, `engine::state::save`, `ipc::key` and `appimage` all flush before the rename. `core` already uses `whole_file::replace` for the suffix list. The bridge needs a per-process temporary name, which `replace` does not offer, and has no dependency on `wirecatch-files` today.
- Confidence: likely (the power-cut result depends on the file system)

### Xrepeated-logic-7 [P3] `updater_proxy` is a third copy of "which proxy for https", and nothing uses it

- Where: `crates/settings/src/proxy.rs`, `updater_proxy` and `UpdaterProxy`; `crates/core/src/manager/proxy.rs`, `Manager::updater_proxy`; the re-export in `crates/core/src/lib.rs`
- Category: dead code
- Failure: no product code calls it. App updates use the app's proxy through `Fetcher` (contract 12), and its doc comment says "the proxy the app updater uses". It has already drifted from what it describes: it reads `https`, then `all`, then direct, and it drops a proxy that does not parse. It also has its own tests to keep up.
- Evidence: a search of `crates` and `apps` finds the names only in these files and `crates/settings/src/proxy/tests.rs`. Requests choose through `Proxies::proxy_for`, and `ytdlp_proxy` is the one other reader with a reason to exist.
- Confidence: certain

### Xrepeated-logic-8 [P3] `text::link_for_display` is a hand-written second password mask over links already masked

- Where: `crates/desktop/src/text.rs`, `link_for_display`; used by `main_window/list.rs` and `main_window/details.rs`; against `crates/logging/src/redact.rs`, `url_for_display`
- Category: no-op code
- Failure: the function changes nothing for a row. A reader takes it for the guard, and a change to what is hidden in a link would have to be made in both.
- Evidence: `Entry::summary` in `crates/core/src/manager/state.rs` already sets `row.url` through `url_for_display`, which also hides a bare user name. The window's `row.url` comes from that summary (contract 04). `link_for_display` gives the same text for such input, using a simpler parser that lacks `hide_link_credentials`'s handling of a password holding `/`, `?` or `#`. `crates/desktop` depends on `wirecatch-logging`.
- Confidence: likely (`summary` as the only source of rows comes from contract 04 and was not traced through `Library`)

### No finding

- `jscpd`: both cross-file clones (`choose_folder` in the Add and queue dialogs, the dialog frame in the proxy and queue dialogs) are identical and short, with no drift. The other nine are tests or the sync and async frame readers.
- Retry and backoff: the engine's `retry::Attempts` is the one download policy, used by `Fetcher` and so by streams and add-ons. The pauses in `store`, `bridge::app`, `shell::bridge` and `suffix_list` serve other jobs.
- Size and speed: `shell::activity::format_bytes` is the one formatter, and units come from `engine::units`.
- Name cleaning: `sanitize` is the one cleaner. `core::candidate` and the engine's `Candidates` both number names; the engine's shortens long names and core's does not, which has no effect since `sanitize` leaves room.
- Process start and kill: `streams::process` is the only runner of the add-on's programs and the only killer. The other starts (`spawn_detached`, `start_again`, `run_bridge`, `replace_appimage`) have different needs.
- Local address rule: `sites::is_local` reads the engine's `AddressKind`.
- Folder moves: `shell::host` and `core::relocate` copy for different jobs, and neither breaks the other's rule.

## Suggestions

None.
