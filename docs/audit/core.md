# crates/core audit (excluding src/media/** and src/addons/**)

## CORE-1 A damaged database file bricks the app with no way out
- severity: high
- category: error-handling
- location: crates/core/src/store.rs:205, crates/core/src/store.rs:216, crates/core/src/store.rs:490, crates/core/src/error.rs:84, crates/core/src/manager/mod.rs:241, apps/desktop/src-tauri/src/host.rs:45
- problem: `Store::open` opens the file, runs `configure` and `migrate` and propagates any `rusqlite::Error` straight out of `Manager::open`. A truncated or garbage `wirecatch.db` (power loss with `synchronous=NORMAL`, a bad sync tool, disk error) fails at the first pragma with `SQLITE_NOTADB`/`SQLITE_CORRUPT`, and the desktop host caches that error in its `OnceCell` and shows it on every start. `CoreError::Database` tells the person to "restart Wirecatch", which can never help. There is no `PRAGMA quick_check`, no backup before a migration, no quarantine of the bad file, and no test for a non-SQLite or corrupted file. Rows that fail to decode are skipped at load (store.rs:636) but stay in the table forever, still holding their cookies, and the person is never told downloads went missing.
- fix: In `Store::open`, classify `rusqlite::ErrorCode::NotADatabase | DatabaseCorrupt` (and a failing `PRAGMA quick_check` on open) as a distinct `CoreError::DatabaseDamaged`, rename the file and its `-wal`/`-shm` to `wirecatch.db.damaged-<unix_ms>`, start with a fresh database, and surface a `UiEvent` (or a field on the snapshot) so the UI can say what happened. Copy the file to `wirecatch.db.bak-v<N>` before running any pending migration. Delete or quarantine rows `read_download` cannot decode instead of leaving them. Add store tests for a garbage file, a truncated file and a mid-migration failure.
- cross-package: yes (desktop, to show the recovery notice; protocol if a new UiEvent is added)

## CORE-2 Private-network check is a weaker, divergent copy of the engine's
- severity: high
- category: security
- location: crates/core/src/manager/media_run.rs:1210, crates/core/src/manager/media_run.rs:1161, crates/engine/src/fetch.rs:594
- problem: `media_run::is_private` re-implements `wirecatch_engine::fetch::is_private` but misses CGNAT/Tailscale `100.64.0.0/10`, benchmarking `198.18.0.0/15`, multicast, `240.0.0.0/4` and broadcast, IPv6 multicast, site-local `fec0::/10`, local-use NAT64, and every IPv6 form that embeds an IPv4 address except `::ffff:` (IPv4-compatible, NAT64 `64:ff9b::`, 6to4 `2002::`). The two policies now disagree: a page on a Tailscale media server (`100.x`) is treated as "on the internet" and its formats are refused, while a format at `64:ff9b::c0a8:101` (192.168.1.1) passes the core pre-check and only fails later inside the engine with a confusing transport error instead of the clear "points to a device on your own network" message. Any future fix to the engine's list will silently not apply here.
- fix: Make the engine's classifier public (for example `pub fn is_private(ip: IpAddr) -> bool` or `PrivateNetwork::classify`) and delete the copy in core. Add core tests for a CGNAT page and a NAT64/6to4 format address.
- cross-package: yes (engine)

## CORE-3 "Page is on the local network" is decided from a fresh DNS lookup of an attacker-chosen name
- severity: medium
- category: security
- location: crates/core/src/manager/media_run.rs:1161, crates/core/src/manager/media_run.rs:1185, crates/core/src/manager/media_run.rs:1205
- problem: `network_rule` returns `PrivateNetwork::Allow` for every format when `all_private(page)` is true, and `all_private` resolves the page's host again at that moment. The page host is whatever site the person visited, so its owner controls that DNS answer: answering with `127.0.0.1` or `192.168.x.x` for this one lookup (classic rebinding, while yt-dlp got the real public address minutes earlier) switches the rule to Allow and lets the format URLs reach devices on the person's LAN (router admin pages, NAS) with the format's headers. The protection the comment describes is therefore bypassable by exactly the party it defends against.
- fix: Only treat a page as local when its URL host is an IP literal in a private range or a `.local`/single-label name, or when the user explicitly added the host; otherwise always use `Block`. Alternatively pass `PrivateNetwork::Auto` to the engine and let it decide from the connection it actually makes, instead of a separate lookup. Add a test with a resolver stub that answers private only on the second lookup.
- correction (W4, D34.4): the alternative above does not work. `Auto` decides from a separate lookup of the URL's host and lets the requests through when that lookup was all local, so a name a website chose must never be started with `Auto`. Use the first fix: treat the page as local only from an address written in the URL (classified with `wirecatch_engine::AddressKind`) or a host the user added, and otherwise pass `PrivateNetwork::Block`, under which the engine, since D34.4, holds a starting host written as a name to the same filter as every other name, so it cannot be rebound to the local network.
- cross-package: no

## CORE-4 Withdrawing a capture while it is being confirmed downloads it twice
- severity: medium
- category: concurrency
- location: crates/core/src/manager/captures.rs:257, crates/core/src/manager/captures.rs:210, crates/core/src/manager/media.rs:266, crates/core/src/manager/media.rs:295
- problem: `confirm_capture` removes the question from `state.pending`, releases the lock, and awaits `insert_confirmed` (which includes a blocking disk scan). If `withdraw_capture` for one of its handovers arrives in that window, it finds `Target::Pending(id)` but no entry in `state.pending`, falls through to `withdraw_media_handover` (the wrong map, which no-ops), removes the handover and answers `Withdrawn`. The confirm then completes, `retarget` silently does nothing because the handover is gone, and the download starts. The extension was told the app dropped it, so the browser keeps its own copy and the file is downloaded twice, which is exactly what the module doc promises never happens. `choose_media` has the same take-out, await, put-back shape. On an insert error the question is put back with handover ids the browser already withdrew.
- fix: Do not remove the question before the await: mark it `deciding` (a state on the pending entry) under the lock, have `withdraw_capture` see that state and either wait for the decision or answer `AlreadyStarted`, and only remove it after `insert` succeeds. Add a test that withdraws between confirm and insert (a fake `unique_path` that blocks on a barrier makes it deterministic).
- cross-package: no

## CORE-5 Two parallel implementations of "a question waiting for the user" have already diverged
- severity: medium
- category: duplication
- location: crates/core/src/manager/captures.rs:328, crates/core/src/manager/media.rs:130, crates/core/src/manager/captures.rs:29, crates/core/src/manager/media.rs:39, crates/core/src/manager/queues.rs:116, crates/core/src/manager/state.rs:38
- problem: Captures awaiting confirmation (`State::pending`) and media awaiting a quality choice (`State::media`) share one id counter but are separate maps with copy-pasted logic: join an existing question with a per-question handover cap, a global cap, insert a `Handover`, confirm/choose by take-out-await-reinsert, reject/dismiss, withdraw. Even the limits are duplicated constants with equal values (`MAX_PENDING`/`MAX_PENDING_MEDIA` = 100, `MAX_HANDOVERS_PER_CONFIRMATION`/`MAX_HANDOVERS` = 32). They have drifted: `delete_queue` re-points `state.pending` questions to the default queue but not `state.media`, so a media choice made after its queue was deleted fails with `QueueNotFound`, and `record_media_failure` for such media logs a warning and loses the failure row entirely. `withdraw_capture` has to guess which map an id lives in (captures.rs:212-220).
- fix: Introduce one `Questions` type (in `state.rs` or its own module) holding `BTreeMap<QuestionId, Question { handovers, queue, kind: Capture(..) | Media(..) }>` with the shared operations (join, cap, withdraw, retarget queue, take for decision) implemented once; captures.rs and media.rs keep only their kind-specific parts. Add a regression test for deleting the queue of a pending media choice.
- cross-package: no

## CORE-6 Public async methods are not cancel-safe and can wedge a download forever
- severity: medium
- category: concurrency
- location: crates/core/src/manager/relocate.rs:46, crates/core/src/manager/relocate.rs:53, crates/core/src/manager/downloads.rs:169, crates/core/src/manager/downloads.rs:194, crates/core/src/manager/captures.rs:262, crates/core/src/manager/media.rs:295
- problem: Several `pub async fn`s on `Manager` set state, await, and only undo it after the await. If the future is dropped mid-way (a caller wrapping it in `timeout` or `select!`, a Tauri command cancelled, a future refactor of `server.rs`), the state is never restored: `set_download_folder` leaves `entry.moving = Some(token)` forever, so the download never starts again (`is_waiting` is false) and any later `remove` awaits that token forever; `remove` leaves entries `removing = true` and hidden until restart; `confirm_capture` and `choose_media` lose the question without a `ConfirmationClosed` event while its handovers stay `Target::Pending` and can never be evicted (`may_forget` is false). Only `server.rs` protects itself by spawning; this is not documented on the API.
- fix: Either run the body of each such method in `self.spawn(...)` and await the `JoinHandle` (so dropping the caller cannot interrupt it), or hold a drop guard that restores the state (cancel `moving`, clear `removing`, put the question back). Document cancel safety on each public async method.
- cross-package: no

## CORE-7 Every progress save rewrites the whole row, cookies included, via INSERT OR REPLACE with positional columns
- severity: medium
- category: performance
- location: crates/core/src/store.rs:598, crates/core/src/store.rs:630, crates/core/src/store.rs:486, crates/core/src/manager/ticker.rs:73, crates/core/src/manager/state.rs:379
- problem: The ticker saves every running download every 2 s by cloning the full `StoredDownload` (origin with cookies, media JSON) and `INSERT OR REPLACE`-ing all 17 columns. In SQLite, REPLACE is a delete plus insert, and with `secure_delete=ON` every delete zeroes the old page, so a progress tick rewrites and zero-fills the whole row and its JSON blobs. REPLACE is also a schema hazard: a column added by a future migration but missed in `save_download` is silently reset to its default on every save, and delete triggers fire. The SELECT, INSERT and `row.get(13)`-style reads repeat the same 17-column list by position in three places, so one reordered column corrupts data without a compile error.
- fix: Add `Op::SaveProgress(Vec<(DownloadId, status, done, total)>)` that runs `UPDATE downloads SET status=?, done=?, total=? WHERE id=?`, and use it from the ticker. Change full saves to `INSERT ... ON CONFLICT(id) DO UPDATE SET ...`. Read columns by name (`row.get("settled")`) or define the column list once as a constant shared by SELECT and INSERT.
- cross-package: no

## CORE-8 The database thread retries deterministic failures with sleeps on every later batch
- severity: medium
- category: error-handling
- location: crates/core/src/store.rs:445, crates/core/src/store.rs:337, crates/core/src/store.rs:358, crates/core/src/store.rs:321
- problem: `apply_with_retries` treats every `rusqlite::Error` the same and sleeps 50 ms, 250 ms and 1 s before giving up. Failures such as a constraint violation, a schema mismatch or `SQLITE_CORRUPT` will never succeed, yet a kept write is retried at the front of every following batch (store.rs:337) and every 5 s, so each subsequent save, flush and the shutdown `Close` pays 1.3 s of sleeping plus a one-by-one replay. With progress saves every 2 s the thread spends most of its time asleep. `MAX_KEPT` then drops the oldest kept writes, which can drop a delete and resurrect a removed download, the very thing the doc comment says never happens.
- fix: Retry with pauses only for `ErrorCode::DatabaseBusy | DatabaseLocked | SystemIoFailure | DiskFull`; for other codes go straight to `apply_one_by_one`, report once, and stop re-including permanently failing writes in the batch transaction (retry them alone on the timer). When trimming to `MAX_KEPT`, coalesce by target (keep the latest write per row) instead of dropping the oldest.
- cross-package: no

## CORE-9 Two file-move primitives, both of which can overwrite a file and leave orphan copies
- severity: medium
- category: duplication
- location: crates/core/src/manager/relocate.rs:205, crates/core/src/manager/relocate.rs:192, crates/core/src/manager/media_run.rs:1105, crates/core/src/manager/media_run.rs:1132
- problem: `relocate::move_file` falls back to `copy` + `remove_file` on any `rename` error, not only a cross-device one. Both `std::fs::rename` (MOVEFILE_REPLACE_EXISTING on Windows, plain rename(2) on Linux) and `std::fs::copy` replace an existing target, so a file that appeared after `unique_path` checked is clobbered. When `rename` fails for another reason (a file locked by an antivirus), the copy succeeds and `remove_file` fails, leaving a full duplicate at the target that the rollback loop (which only undoes earlier pairs) never removes; the rollback's own failures are discarded with `let _`, so a download can end up split across two folders silently. `media_run::place` is a second implementation with different rules (hard link, then rename on any hard-link error, which again replaces).
- fix: One `fs_move_no_replace(from, to)` helper in a shared module: `MoveFileExW` with `MOVEFILE_COPY_ALLOWED` and without `MOVEFILE_REPLACE_EXISTING` on Windows, `renameat2(RENAME_NOREPLACE)` with an `EXDEV` fallback to copy-to-temp-then-link on Linux; delete the partial target when a copy fails; log rollback failures. Use it from both `relocate` and `place`.
- cross-package: no

## CORE-10 Every state change scans the whole download history under the global lock
- severity: medium
- category: performance
- location: crates/core/src/manager/scheduling.rs:45, crates/core/src/manager/scheduling.rs:63, crates/core/src/manager/scheduling.rs:116, crates/core/src/manager/scheduling.rs:132, crates/core/src/manager/downloads.rs:434, crates/core/src/manager/relocate.rs:123, crates/core/src/manager/captures.rs:316
- problem: `reconcile` runs after almost every operation (each add, pause, finish, settings change, scheduler wake) and walks `state.downloads` four times (busy set, planner views, waiting lines, wait-reason update) with fresh `HashMap`/`Vec` allocations, including every completed download in the history. All of it holds the single `Mutex<State>` that every UI command and the 10 Hz ticker also need. `insert` clones the entire `Claimed` set (two `HashSet<String>`) on every attempt of every add, so a bulk add of N links is O(N squared). `earlier_download` re-parses two URLs per history entry per call. A user with tens of thousands of finished downloads pays this on every click.
- fix: Maintain a `waiting: BTreeSet<(QueueId, bool, i64, DownloadId)>` index next to `running` (updated in the same places `running` and `claimed` are, and checked by `assert_consistent`) and have `reconcile` and `update_wait_reasons` visit only running and waiting entries. Pass `unique_path` a closure that checks `claimed` under a short lock instead of cloning it, or clone only the claims in the target folder. Normalise each row's URL once (store it on `Entry`) for `earlier_download`.
- cross-package: no

## CORE-11 God modules and functions
- severity: medium
- category: design
- location: crates/core/src/manager/media_run.rs:1, crates/core/src/manager/addons.rs:1, crates/core/src/manager/media.rs:1, crates/core/src/manager/downloads.rs:413, crates/core/src/manager/run.rs:185, crates/core/src/manager/relocate.rs:24, crates/core/src/manager/restore.rs:44, crates/core/src/manager/settle.rs:94, crates/core/src/manager/addons.rs:749, crates/core/src/manager/media_run.rs:68
- problem: `manager/media_run.rs` is 1289 non-test lines mixing run orchestration, the refresh/retry policy, the private-network policy (`network_rule`, DNS), the work-folder manifest (`Downloaded`), small-file fetching, ffmpeg conversion steps, final file placement and a dozen user-facing error texts. `manager/addons.rs` (1037 lines) mixes GitHub release discovery, OpenPGP checking, parallel download, unpack, atomic swap and UI state. `start_media_run` needs `#[allow(clippy::too_many_arguments)]`. Functions over 80 lines: `downloads::insert` (116), `run::finish` (102), `relocate::set_download_folder` (91), `restore::restore` (91), `settle::settle_path_checking` (88), `addons::checked_release` (82). Every subsystem is an `impl Manager` block sharing one `Mutex<State>`, so the add-on installer locks download state just to report progress (addons.rs:648, every 250 ms).
- fix: Split `media_run.rs` into `media_run/{mod.rs (start, run), page.rs (run_page, page_attempt, fetch_parts, fetch_part), extras.rs (fetch_extras, fetch_small), save.rs (save, join_original, inputs_for, subtitle_files), workdir.rs (Downloaded and all work-folder file names), placement.rs (finish, place, final_path, sidecar_path), network.rs (network_rule), errors.rs}`. Split `manager/addons.rs` into `releases.rs` (lookup and signature check), `install.rs` (install_parts, fetch_part, verify_and_unpack, swap_in) and a thin `impl Manager` facade; give `AddonSlot` its own mutex. Replace the 7 parameters of `start_media_run` with a `RunContext` struct. In `insert`, extract `build_row` and `register_handover`; in `finish`, extract `ended_from(RunEnd)` and `apply_end(entry, Ended, Option<Stop>)`.
- cross-package: no

## CORE-12 A download's lifecycle is an implicit state machine spread over flags
- severity: medium
- category: design
- location: crates/core/src/manager/state.rs:119, crates/core/src/manager/scheduling.rs:189, crates/core/src/manager/settle.rs:231, crates/core/src/manager/state.rs:235
- problem: `Entry` carries `row.status`, `run: Option<Run>` (with its own `stop: Option<Stop>`), `removing: bool`, `moving: Option<CancellationToken>`, `cleanup: Option<CancellationToken>`, `resume_first: bool` and `naming.settled`. Most combinations are invalid (removing and moving, `Completed` with a run, `Queued` while moving), and every reader re-derives the real state differently: `is_waiting` (scheduling.rs), `is_stopping` (settle.rs), `entry()` filtering `removing` (state.rs), `server_answered`. A new flag has to be added correctly to each of these by hand, which is how CORE-6 and CORE-4 style bugs arise.
- fix: Introduce `enum Phase { Idle, Running(Run), Moving { done: CancellationToken, run_exited: Option<..> }, Removing, CleaningUp(CancellationToken) }` (or an equivalent) on `Entry` with transitions as methods (`begin_move`, `finish_move`, `begin_remove`), and derive `is_waiting`/`is_stopping` from it in one place.
- cross-package: no

## CORE-13 Add-on install pipeline uses String errors and compares a sentinel string for control flow
- severity: medium
- category: design
- location: crates/core/src/manager/addons.rs:36, crates/core/src/manager/addons.rs:546, crates/core/src/manager/addons.rs:559, crates/core/src/manager/addons.rs:613, crates/core/src/manager/addons.rs:500, crates/core/src/manager/addons.rs:943, crates/core/src/manager/addons.rs:992
- problem: `install_parts`, `fetch_part`, `verify_and_unpack` and `swap_in` all return `Result<_, String>`. Whether a part was cancelled is detected by `reason == STOPPED` (comparing against the text "It was stopped."), so rewording that message changes behaviour. Errors lose their kind and source (`map_err(|e| e.to_string())` on `CoreError`, `JoinError`), and `Lookup::Refused(e.to_string())` turns a blocking-task panic into "the release does not meet the rules".
- fix: Define `enum InstallError { Stopped, Download { component, source: wirecatch_engine::Error }, Checksum { component }, TooLarge { component }, Unpack(..), Swap { component, source: io::Error }, Internal(String) }` with `thiserror`, match on `InstallError::Stopped`, and render the person-facing sentence only in `end_addon_op`.
- cross-package: no

## CORE-14 UserError (UI text) is the internal error type of the media pipeline, and logic branches on its fix field
- severity: medium
- category: design
- location: crates/core/src/manager/run.rs:31, crates/core/src/manager/media_run.rs:951, crates/core/src/manager/media_run.rs:1161, crates/core/src/manager/media_run.rs:838, crates/core/src/manager/media_run.rs:1233, crates/core/src/manager/media.rs:416, crates/core/src/manager/media.rs:872
- problem: Media runs fail with `wirecatch_protocol::app::UserError`, the final presentation type, built deep inside the pipeline. `fetch_part` decides whether to refresh the page's addresses by checking `user_error::describe(..).fix == Some(FixAction::RefreshLink)`, so changing which button the UI offers for an HTTP status changes retry behaviour. User-facing sentences are scattered across media_run.rs, media.rs and sign_in.rs instead of `user_error.rs`, which exists for exactly that, so wording rules (D18.5) cannot be reviewed in one place and there are about 20 `UserError { .. }` literals outside it.
- fix: Give the media pipeline a typed `MediaError` (Refused { status }, AddonMissing, Network(..), Disk(io::Error), Ffmpeg(..), PrivateNetwork, ...), classify refusals from `EngineError::HttpStatus` directly, and convert to `UserError` once in `run::finish` through functions in `user_error.rs`. Move every person-facing sentence in scope into `user_error.rs`.
- cross-package: no

## CORE-15 CoreError::Invalid is a catch-all that sends wrong protocol error codes
- severity: medium
- category: error-handling
- location: crates/core/src/error.rs:39, crates/core/src/error.rs:139, crates/core/src/manager/addons.rs:176, crates/core/src/manager/addons.rs:216, crates/core/src/manager/addons.rs:333, crates/core/src/manager/media.rs:152, crates/core/src/manager/captures.rs:364
- problem: About 30 call sites in scope use `CoreError::Invalid(String)` for unrelated conditions: bad user input, capacity limits ("100 downloads are already waiting"), network failures while checking for add-on updates (addons.rs:298 then 333) and I/O failures deleting add-on files (addons.rs:176, 216). `to_protocol` maps all of them to `ErrorCode::InvalidRequest`, so a socket client cannot tell "you sent something wrong" from "the network is down" or "the disk refused", and callers cannot match on the cause. `WrongState(&'static str)` has the same problem for state errors.
- fix: Split into specific variants (`LimitReached { what, max }`, `UpdateCheckFailed(String)`, `File { path, source }` for the add-on deletions, `Invalid` only for input validation), map them to distinct `ErrorCode`s, and keep the message text in the `#[error]` attributes.
- cross-package: yes (protocol, if new ErrorCode values are added)

## CORE-16 Lossy rebuild of io::Error and duplicated adapters around user_error::describe
- severity: medium
- category: error-handling
- location: crates/core/src/manager/media_run.rs:1252, crates/core/src/manager/media_run.rs:1270, crates/core/src/manager/run.rs:411, crates/core/src/manager/media_run.rs:85, crates/core/src/user_error.rs:211
- problem: `media_run::disk` and `stream_error` turn an `io::Error` into `EngineError::Io(io::Error::new(e.kind(), e.to_string()))` only because `user_error::describe` takes an `EngineError`. The rebuilt error has no `raw_os_error()`, which defeats the `DISK_FULL_CODES` fallback in `user_error::disk` (any full-disk code std does not classify becomes "Could not write the file ... Check the drive" instead of the Free disk space fix). The same "wrap a URL parse error in `EngineError::InvalidUrl` and describe it" adapter is copy-pasted in run.rs and media_run.rs.
- fix: Make `user_error::disk(&io::Error, folder)` and an `invalid_link()` constructor `pub(crate)` and call them directly; delete the rebuild and the copies.
- cross-package: no

## CORE-17 Core parses the engine's private resume-file format by key name
- severity: medium
- category: design
- location: crates/core/src/naming.rs:177, crates/core/src/naming.rs:186
- problem: `naming::left_by` reads the engine's `.wcstate` resume file as untyped JSON and looks up `"url"` to decide whether a leftover part file belongs to this download. That file format is an engine implementation detail with no stability guarantee; renaming the field, nesting it or switching to a binary format makes `left_by` silently return `false`, and a restarted capture then picks `name (1)` and downloads from scratch instead of continuing.
- fix: Add a documented engine API such as `wirecatch_engine::naming::resume_url(path) -> Option<Url>` (or `belongs_to(path, url) -> bool`) next to `part_paths`, covered by an engine test, and call it here.
- cross-package: yes (engine)

## CORE-18 Errors that matter are discarded with let _ or silent early returns
- severity: medium
- category: error-handling
- location: crates/core/src/manager/mod.rs:251, crates/core/src/manager/relocate.rs:193, crates/core/src/manager/addons.rs:1007, crates/core/src/manager/addons.rs:996, crates/core/src/manager/media_run.rs:1006, crates/core/src/manager/media_run.rs:1127, crates/core/src/manager/run.rs:395, crates/core/src/manager/addons.rs:237
- problem: Several discarded results hide real damage. mod.rs:251 deletes the `tmp` folder of cookie files left by a crash and ignores a failure, so cookies can stay on disk with no log (the comment says "nothing may keep them"). relocate.rs:193 ignores a failed rollback, leaving a download's part and resume files in two folders. addons.rs:1007 ignores failing to put the old add-on part back after a failed swap, leaving the add-on broken while the record still lists it. run.rs:395 ignores a panic in the Mark-of-the-Web task. `rescan_addon` returns silently when its blocking task fails. media_run.rs:1006 and 1127 leave work folders and duplicate files behind silently.
- fix: Log every one of these at `warn!` with the path and error; for the cookie folder, retry on the next start and log at `error!`; for the add-on rollback and the relocate rollback, return an error that says what state was left so the UI can tell the person.
- cross-package: no

## CORE-19 Magic file names, strings and numbers scattered through the manager
- severity: medium
- category: magic-value
- location: crates/core/src/manager/media_run.rs:516, crates/core/src/manager/media_run.rs:536, crates/core/src/manager/media_run.rs:548, crates/core/src/manager/media_run.rs:551, crates/core/src/manager/media_run.rs:573, crates/core/src/manager/media_run.rs:574, crates/core/src/manager/addons.rs:171, crates/core/src/manager/addons.rs:506, crates/core/src/manager/addons.rs:577, crates/core/src/manager/addons.rs:1028, crates/core/src/manager/addons.rs:314, crates/core/src/manager/addons.rs:891, crates/core/src/manager/media.rs:666, crates/core/src/manager/media_run.rs:1029, crates/core/src/settings.rs:167, crates/core/src/manager/media_run.rs:1112, crates/core/src/manager/media_run.rs:1195, crates/core/src/manager/media_run.rs:641, crates/core/src/manager/sign_in.rs:27, crates/core/src/manager/mod.rs:237
- problem: About 30 unnamed literals, several duplicated so a change in one place silently breaks the other. Work-folder layout: `"sub-{n}.{ext}"` (twice), `"cover.image"` (twice), `"metadata.txt"` (twice), `"subtitle-{n}.srt"`, `"output.{ext}"`, `"joined.{ext}"`, `"{role}.done"`; add-on layout: `".work"` (twice), `"{name}-{version}.download"` (two separate `format!`s that must agree for cleanup to work), `"stage-"`, `"old-"`; data folder `"tmp"`. Component names `"yt-dlp"`, `"ffmpeg"`, `"deno"` as bare strings in lookups and sort order. yt-dlp selector `"bv*+ba/b"` and labels `"Best available"`, `"Original"`, `"Unknown"`. Numbers: language length `35` in media_run duplicates `MAX_LANGUAGE_CHARS`; `10_000` name attempts (while `naming::unique_path` loops unbounded); DNS timeout `10 s`; size poll `200 ms`; head `512` bytes; `32..=512` kbps; `"after 7 days"` in `FORGOT_SIGN_IN` duplicates `FORGET_AFTER_MILLIS`; `1024 * 1024` written out four times.
- fix: A `workdir` module with named constants and one function per derived name (`release_file(component)`, `marker(role)`); a `ComponentName` enum (or constants) in the add-on manifest; constants for the yt-dlp selector and quality labels; reuse `settings::MAX_LANGUAGE_CHARS`; one `MIB` constant; build `FORGOT_SIGN_IN` from `FORGET_AFTER` with `format!`; share one `MAX_NAME_ATTEMPTS` between `unique_path`, `settle_path_checking` and `place`.
- cross-package: no

## CORE-20 Risky logic without tests
- severity: medium
- category: testing
- location: crates/core/src/store.rs:748, crates/core/src/manager/captures.rs:200, crates/core/src/manager/queues.rs:94, crates/core/src/manager/media_run.rs:1290, crates/core/src/owner_only.rs:218
- problem: Missing coverage on the riskiest paths: no test opens a garbage, truncated or non-SQLite database file (CORE-1); no test races `withdraw_capture` against `confirm_capture`/`choose_media` (CORE-4); no test deletes the queue of a pending media choice (CORE-5); no test for CGNAT, NAT64 or 6to4 addresses in `is_private`/`network_rule` (CORE-2, CORE-3); no test that a deterministic store failure does not slow later saves (CORE-8); no test that a cancelled `set_download_folder` future leaves the download usable (CORE-6).
- fix: Add one regression test per item above, each failing on the current code, using the local test server and the fake engine already in `src/manager/fake.rs`.
- cross-package: no

## CORE-21 Win32 ACL code: hand-rolled wrappers and an unchecked DACL-present flag
- severity: low
- category: unsafe
- location: crates/core/src/owner_only.rs:59, crates/core/src/owner_only.rs:89, crates/core/src/owner_only.rs:93, crates/core/src/owner_only.rs:120, crates/core/src/owner_only.rs:159
- problem: The unsafe is sound as used, but: `OwnedHandle` re-implements `std::os::windows::io::OwnedHandle` (which also handles invalid handles); `GetSecurityDescriptorDacl`'s `present` is ignored, so if the descriptor ever lacked a DACL `dacl` stays null and `SetNamedSecurityInfoW` with a null DACL grants Everyone full access (fail-open on a security control; unreachable today only because the SDDL literal starts with `D:`); `from_wide` hand-rolls a strlen loop; several SAFETY comments state the conclusion ("plain out-parameter call") rather than the invariant (handle validity, buffer alignment and lifetime).
- fix: Use `std::os::windows::io::OwnedHandle::from_raw_handle`; return an error unless `present != 0 && !dacl.is_null()`; compute the length with `lstrlenW` or `widestring::U16CStr::from_ptr_str`; rewrite SAFETY comments to name the invariant each call relies on.
- cross-package: no

## CORE-22 Failing to restrict the data folder to the owner is only a warning
- severity: low
- category: security
- location: crates/core/src/store.rs:742, crates/core/src/store.rs:211, crates/core/src/store.rs:214
- problem: D17 requires the database (which holds browser cookies) to be readable only by the current user, but `restrict_to_owner` logs a `warn!` and continues when the ACL or chmod fails. Nothing reaches the UI, so the person never learns their cookies are readable with the folder's inherited access, and a regression in `owner_only` would ship unnoticed.
- fix: Return the outcome from `Store::open` (for example a `restricted: bool` on the snapshot) and emit a one-time `UiEvent` or settings warning when it fails; keep starting the app.
- cross-package: yes (desktop, to show the warning)

## CORE-23 Small-file size cap is enforced by polling
- severity: low
- category: performance
- location: crates/core/src/manager/media_run.rs:641, crates/core/src/manager/media_run.rs:609
- problem: `fetch_small` caps subtitle and cover downloads by polling `Progress` every 200 ms and cancelling once `done` passes the limit, against the repo rule of no polling where an event exists. Up to 200 ms of data past the cap is written, and every fetch keeps a timer task alive.
- fix: Add `max_total_bytes` to `DownloadOptions` in the engine (it already enforces `max_request_bytes` and refuses oversized announced totals through `resolve_path`, as manager/addons.rs:606 does) and drop the poll loop.
- cross-package: yes (engine)

## CORE-24 futures_join silently drops every future after the second
- severity: low
- category: design
- location: crates/core/src/manager/media_run.rs:878, crates/core/src/manager/media_run.rs:382
- problem: `futures_join` takes a `Vec` of futures but only polls the first two; a third is dropped without running and without an error. It is correct today only because `ROLES` has two entries and the `zip` truncates to it, so adding a third part kind (a separate subtitle track, say) would silently never download it.
- fix: Use `tokio::task::JoinSet` or `futures_util::future::join_all` (move `futures-util` from dev-dependencies), or make the signature `join2(a, b)` so the limit is in the type.
- cross-package: no

## CORE-25 Boolean parameters with unreadable call sites
- severity: low
- category: design
- location: crates/core/src/manager/addons.rs:235, crates/core/src/manager/addons.rs:261, crates/core/src/manager/addons.rs:429, crates/core/src/manager/downloads.rs:162, crates/core/src/manager/state.rs:425, crates/core/src/user_error.rs:21, crates/core/src/manager/relocate.rs:177, crates/core/src/settings.rs:172
- problem: Call sites such as `spawn_addon_rescan(true)`, `rescan_addon(false)`, `start_addon_op(.., true)`, `describe(&e, folder, true)`, `requeue_when_stopped(false)` and `check_media_defaults(.., true)` cannot be read without opening the callee. `remove(ids, delete_file: bool)` is public API.
- fix: Small enums: `Scan::Deep | Scan::Quick`, `OpKind::Quiet | OpKind::Visible`, `Worked::Before | Worked::Never`, `AfterStop::Resume | AfterStop::Restart`, `MediaKind::Audio | Video`, and `RemoveFiles::Keep | Delete` on the public `remove`.
- cross-package: yes (desktop, for the public `remove` signature)

## CORE-26 Confirmation ids and timestamps are bare integers
- severity: low
- category: design
- location: crates/core/src/manager/state.rs:38, crates/core/src/manager/handovers.rs:22, crates/core/src/error.rs:24, crates/core/src/manager/addons.rs:32, crates/core/src/manager/addons.rs:47, crates/core/src/manager/media.rs:38, crates/core/src/manager/sign_in.rs:21
- problem: Downloads and queues have newtypes, but confirmation/media-choice ids are raw `u64` (shared counter, two maps, see CORE-5), run ids are `u64`, and wall-clock times are raw `i64` Unix milliseconds with hand-computed constants (`CHECK_EVERY_MILLIS` and `DAY_MILLIS` are the same value defined twice) while other intervals use `Duration`. Mixing a `QuestionId` with a run id, or milliseconds with seconds, compiles.
- fix: Add `QuestionId(u64)` in `wirecatch-protocol` and use it for `PendingCapture.id`, `MediaChoice.id` and the maps; add a `UnixMillis(i64)` newtype with `checked_add(Duration)`/`since()` and express the constants as `Duration`.
- cross-package: yes (protocol)

## CORE-27 Supported URL schemes and small helpers duplicated
- severity: low
- category: duplication
- location: crates/core/src/manager/downloads.rs:538, crates/engine/src/transport.rs:221, crates/cli/src/main.rs:65, crates/core/src/manager/settle.rs:237, crates/core/src/manager/relocate.rs:215, crates/core/src/store.rs:669, crates/core/src/manager/media_run.rs:622, crates/core/src/manager/media_run.rs:1045, crates/core/src/manager/media.rs:864
- problem: The list `http | https | ftp | ftps | ftpes` lives in core, the engine and the CLI separately (and `CoreError::InvalidUrl`'s message omits ftpes). "File name of a path as a lossy String" is written out at about eight sites (`naming_of`, `file_name_of`, inline in store, run, downloads, media_run). The `HashMap<String, String>` to `Vec<Header>` to `HeaderMap` conversion is duplicated in `fetch_small` and `part_options`, and header lookup by name exists as `headers::referrer`, `media::cookie_header` and an inline user-agent search.
- fix: Export `wirecatch_engine::is_supported_scheme`; add `naming::file_name_of(&Path) -> String`, `headers::find(&[Header], name)` and `headers::from_pairs(&HashMap<..>) -> HeaderMap`, and use them everywhere.
- cross-package: yes (engine, cli)

## CORE-28 Mutex poisoning is swallowed even though State has cross-field invariants
- severity: low
- category: concurrency
- location: crates/core/src/manager/mod.rs:401, crates/core/src/manager/state.rs:311
- problem: `lock()` recovers from a poisoned mutex with `into_inner()` on the grounds that it is "plain data", but `State` keeps `running`, `claimed` and `dirty` in step with `downloads` (checked only by `assert_consistent` in debug builds). A panic mid-update in release leaves those sets out of step forever: a stale `claimed` entry blocks a file name, a stale `running` entry leaks a concurrency slot.
- fix: On recovering a poisoned lock, log at `error!` and rebuild the derived sets (`running`, `claimed`) from `downloads` before returning the guard; a `State::repair_indexes()` next to `assert_consistent` does it.
- cross-package: no

## CORE-29 Tests rely on fixed sleeps for negative assertions
- severity: low
- category: testing
- location: crates/core/tests/manager/media.rs:547, crates/core/tests/manager/media.rs:564, crates/core/tests/manager/media.rs:992, crates/core/tests/manager/media.rs:1083, crates/core/tests/manager/addons.rs:87, crates/core/tests/manager/addons.rs:736, crates/core/tests/manager/protocol.rs:738
- problem: Several tests sleep 100 to 800 ms and then assert that something did not happen (no extra request, no second yt-dlp call, a child process stopped). On a loaded CI machine the event can land after the sleep and the test passes while the bug is present; on a fast one the suite is slower than needed (about 4 s of fixed sleeps in these files alone).
- fix: Drive these with the `ManualClock` and tokio's paused time (`#[tokio::test(start_paused = true)]` with `tokio::time::advance`), or wait for an explicit "settled" signal from the fake server or fake tool before asserting the negative.
- cross-package: no

Summary: 29 findings in crates/core (2 high, 18 medium, 9 low, 0 critical).
The highest risks are a damaged database permanently stopping the app with no recovery, and a duplicated, weaker private-network check (plus a rebinding-prone "local page" decision) in the media pipeline.
The main structural debt is two parallel pending-question implementations that have already diverged into bugs, a cancel-unsafe async API, stringly-typed errors, and media_run.rs/addons.rs needing to be split.
