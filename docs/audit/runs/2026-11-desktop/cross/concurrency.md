# Cross-module flow: concurrency

## Files read

Whole:

crates/core/src/manager/mod.rs
crates/core/src/manager/state.rs
crates/core/src/manager/run.rs
crates/core/src/manager/downloads.rs
crates/core/src/manager/ticker.rs
crates/core/src/manager/settle.rs
crates/core/src/manager/relocate.rs
crates/core/src/manager/file_exists.rs
crates/core/src/manager/scheduling.rs
crates/core/src/manager/queues.rs
crates/core/src/manager/added_pages.rs
crates/core/src/manager/captures.rs
crates/core/src/manager/handovers.rs
crates/core/src/manager/media_run/mod.rs
crates/core/src/manager/media_run/placement.rs
crates/core/src/naming.rs
crates/desktop/src/library.rs
crates/desktop/src/backend.rs
crates/desktop/src/lifecycle.rs
crates/desktop/src/main.rs

In part, for the question named:

- crates/engine/src/target.rs (first 300 lines: how an exact path is planned)
- crates/engine/src/download.rs (where the resolver is called)
- crates/server/src/lib.rs (first 180 lines: the connection loop and how requests are started)
- crates/settings/src/folders.rs and crates/settings/src/lib.rs (whether resolving a folder under the lock touches the disk: it does not)
- crates/core/src/manager/preferences.rs (settings export and import)
- crates/shell/src/quit.rs (what a quit waits for)
- crates/desktop/src/notices.rs (the event loop)

Left out to stay near the size limit: `crates/core/src/manager/media.rs`, `crates/core/src/manager/addons/`, `crates/core/src/manager/media_run/page.rs` and the desktop's window code. Races inside media questions and add-on installs were not reviewed here.

Checked and found sound: no lock is held across an `.await`, and no path takes the state lock twice. `launch` holds the lock while it spawns, so `finish` cannot see a run before `entry.run` is set. `RunGuard` reports once. Store sends never block (unbounded channel). Events are sent under the lock, so a removal cannot overtake a change of the same row, and `Library::load` subscribes before it reads, so stale events converge. The ticker cannot lose a wake-up, because `Notify` keeps a permit.

Proof tests were run against a copy of the tree at `86c410c62`, on Windows. Patches are in `%TEMP%\wirecatch-audit\2026-11-desktop\` until stage 2 takes them into `proofs/`.

## Findings

### Xconcurrency-1 [P0] Two questions about one file, both answered Replace, run two downloads on one path

- Where: `crates/core/src/manager/file_exists.rs`, `replace_at`, `answer_file_exists`, `on_found`; `found_existing` for the check that misses it
- Category: bug
- Failure: `report.pdf` is in the folder and two downloads from different links both want that name (a chosen name or `Content-Disposition`), each with a different provisional path. Both stop for the duplicate and both ask, which is the default setting. The person answers Replace on both windows. Both rows now hold `report.pdf`, both run at once with `ExistingFile::Replace`, and both use `report.pdf.wcpart` and `.wcstate`. In the proof the first link's row ended `Completed` while the file held only the second link's bytes, and the second row ended `Failed` with "The folder ... cannot be used". So a row reports a finished file that is not its own, and the other fails with a misleading fix. With the setting at Replace no person is needed: two runs only have to overlap between `found_existing` and the other's `finish`, which is likely when a batch of links has one server name.
- Evidence: an open question holds no claim on the path it asks about, so `taken_by_another` in `found_existing` is false for the second download. `replace_at` calls `state.claimed.insert(path)` without checking `contains`. The engine's contract (01) says "Two runs at once on one path are the caller's to prevent", and `plan::plan` does not look at paths. Proof output: `PATHS ...saved\report.pdf | ...saved\report.pdf`; `END Completed None / Failed Some(UserError { message: "The folder ... cannot be used. Choose another folder.", fix: Some(ChooseFolder) })`; `FILE len 4194304 aa 0 bb 4194304`. `assert_consistent` also panics on the ticker in that debug build: `claimed paths out of step, left: 2, right: 1`. In a release build there is no panic, and the first row's completion gives up the claim while the second still runs. Which bytes survive depends on timing; a mix of both is possible when the second run resumes the first's resume file (same size, no validators).
- Reproduction: `git apply Xconcurrency-1.patch`, then `cargo test -p wirecatch-core --test manager two_answers_of_replace -- --nocapture`. Fails with `two downloads share one path`.
- Confidence: certain

### Xconcurrency-2 [P2] A removal still stopping its run when the app quits is undone, and the download starts again at the next start

- Where: `crates/core/src/manager/downloads.rs`, `finish_removing`; `crates/core/src/manager/mod.rs`, `shutdown`
- Category: bug
- Failure: the person removes a running download and the app quits before the run has stopped (up to `stop_timeout`, 5 s, for a server that does not answer). `finish_removing` meets `lock_open` returning `Closed` and stops, so the database never hears of the removal and the partial files stay. If `shutdown` reached the run first, the stop reason is `Shutdown` and the status is kept, so after the restart the removed download is running again. A capture the app gave back to the browser (`remove_in_background`) comes back the same way, so the browser and the app can both download one file, which `captures.rs` promises never happens.
- Evidence: `Hold::Removing` is not saved, and `shutdown` does not wait for removals. `Manager::remove` returned `Err(Closed)` in the proof, and after reopening the row was back as `Connecting`.
- Reproduction: `git apply Xconcurrency-2.patch`, then `cargo test -p wirecatch-core --test manager a_removal_in_progress_at_quit -- --nocapture`. Output: `REMOVE Err(Closed)`, `AFTER Ok(Connecting)`, `the removed download came back`.
- Confidence: certain

### Xconcurrency-3 [P2] A browser capture of a file already in the folder opens its question and closes it at once

- Where: `crates/core/src/manager/captures.rs`, `await_answer` and `capture_arrived`; `crates/core/src/manager/run.rs`, `finish`
- Category: wrong implementation
- Failure: a link whose server names the file is captured again after the file has finished, with the default Ask. The run stops for the duplicate, and `finish` asks. `exited` then wakes `await_answer`, which finds the path not settled and answers "It stopped before the server answered." The row is removed and the question closed. D88.3 says asking pauses the download and opens its window; instead the window is asked for and closed within one exchange, the person never gets to choose, and the browser saves its own copy.
- Evidence: `server_answered` is `naming.settled || Completed`, and `ask` leaves `settled` false. The server has answered, so the reply's reason is also false. No test in `crates/core/tests/manager` captures a file that exists. Proof output, in order: `AGAIN Err(NotStarted("It stopped before the server answered."))`, `EVENT asked QuestionId(1)`, `EVENT closed QuestionId(1)`, `EVENT removed [DownloadId(2)]`, `OPEN 0 ROWS 1`.
- Reproduction: `git apply Xconcurrency-3-and-6.patch`, then `cargo test -p wirecatch-core --test manager a_finished_file_captured_again -- --nocapture`.
- Confidence: certain

### Xconcurrency-4 [P2] A quit during a move to another folder neither waits for it nor records it

- Where: `crates/core/src/manager/mod.rs`, `shutdown`; `crates/core/src/manager/relocate.rs`, `finish_moving`, `move_file`
- Category: bug
- Failure: the person moves a download to another drive, then quits during the copy. `shutdown` waits only for runs, so it closes the database while the copy goes on. If the process ends mid-copy, a cut-off file stays at the final name in the new folder (`move_file` copies straight to `to`), with nothing in the list about it. If the copy ends first, `finish_move`'s save arrives after the database closed and is dropped: the list still names the old folder while the files are in the new one. A finished row then points at a missing file, and an unfinished one starts over from zero with its part file stranded.
- Evidence: `exits` in `shutdown` holds only `run.exited`; `Hold::Moving` is never looked at. The store's contract (03) says saves after close "log and do nothing". `set_download_folder` documents "Runs to the end even when the caller stops waiting", which the quit does not honour. `QUIT_PATIENCE` applies to `shutdown` only, so it gives the move no time. Not run: which of the two ends happens depends on the runtime's teardown when `cx.quit()` returns.
- Confidence: likely

### Xconcurrency-5 [P2] Choosing a path from the server's answer blocks an async thread on the disk

- Where: `crates/core/src/manager/settle.rs`, `settle_path`, `settle_path_checking`; `crates/core/src/manager/file_exists.rs`, `found_existing`, `saved_at`
- Category: wrong implementation
- Failure: a download into a folder on an unreachable network share or mapped drive gets its server's answer. `exists`, `metadata` and reading the resume file (`left_by`) block the tokio worker until the system gives up, often many seconds. As many first answers as there are workers stall the runtime, and every interface command goes through it (`Backend::run` spawns there), as do the ticker and the bridge. The windows stop updating and commands do nothing until the share times out.
- Evidence: `Destination::choose` documents "the resolver is the caller's, which must not block", and is called directly in the async `download` body (`crates/engine/src/download.rs`). `settle_path_checking`'s comment takes the disk check off the lock for "a folder that is slow to answer (an unreachable network share)" but keeps it on the async thread. `insert` does the same check through `blocking`, and says why. Not measured against a real share.
- Confidence: likely

### Xconcurrency-6 [P3] Two captures of one link at the same moment make two downloads

- Where: `crates/core/src/manager/captures.rs`, `capture_arrived`, `join_unfinished`; `crates/core/src/manager/downloads.rs`, `insert`
- Category: bug
- Failure: two captures of one link with different handover ids arrive together, and the server runs requests side by side. Each `join_unfinished` finds nothing because neither has been added, and each then goes through `insert`, whose `blocking` step lets the other past. Two rows and two files result (`report.pdf`, `report (1).pdf`), against D89.
- Evidence: the check and the insert are separate lock sections with an await between them, and `insert` re-checks only the handover id. Proof over 20 rounds: `MOST ROWS 2`.
- Reproduction: `git apply Xconcurrency-3-and-6.patch`, then `cargo test -p wirecatch-core --test manager two_clicks_at_once -- --nocapture`.
- Confidence: certain

### Xconcurrency-7 [P3] Pause all while a cancel is stopping keeps the download and its partial file

- Where: `crates/core/src/manager/downloads.rs`, `pause_all`; `crates/core/src/manager/state.rs`, `Entry::stop`
- Category: bug
- Failure: the person cancels a running download and, while it stops, Pause all arrives (tray, browser or main window). `Stop::Pause` is a person's choice and replaces `Stop::Cancel`, so the row ends `Paused` and the partial file is not deleted. The same goes for a pending `Restart`. Pause all was not a choice about that download. `resume_all` already guards this through `requeue_when_stopped(AfterCancel::Stay)`; `pause_all` has no such guard. The window is short for a run that stops promptly and up to `stop_timeout` for one that does not.
- Evidence: `Entry::stop`'s condition, and `pause_all` calling `stop_run(entry, Stop::Pause)` on every entry. `the_users_choice_is_never_overridden_by_the_app` does not cover this order.
- Confidence: certain

### Xconcurrency-8 [P3] Resume all and a new link leave a file-exists question open while the download runs

- Where: `crates/core/src/manager/downloads.rs`, `resume_all`, `refresh_link`; `crates/core/src/manager/file_exists.rs`, `answer_file_exists`
- Category: bug
- Failure: a download is paused on a question, and Resume all (or Refresh link) sends it back in line without closing the question, which D88.3 requires of resuming and `requeue` does. Once its run has started, an answer from the still-open window takes the question off, sends `ConfirmationClosed`, then refuses with `DownloadNotFound` for a download that is plainly in the list. The person's Replace or Keep both is lost, and the run asks again when its server answers.
- Evidence: only `requeue`, `cancel` and `finish_removing` call `close_file_exists` among these actions. `answer_file_exists` removes the question before it checks `e.run.is_none()`.
- Confidence: certain

### Xconcurrency-9 [P3] Moving a finished file can give up another download's claim on its old path

- Where: `crates/core/src/manager/relocate.rs`, `finish_move`
- Category: bug
- Failure: a finished file holds no claim. Its move renames the file, off the lock. In the moment before `finish_move` takes the lock, a new download with that name finds the old path free on disk and claims it. `finish_move` then runs `state.claimed.remove(old_path)`, which removes the new download's claim, so a third download can be given the same path, with the result of Xconcurrency-1. Debug builds panic in `assert_consistent`.
- Evidence: `move_files` reserves only when `!completed`, but `finish_move` removes `old_path` whether or not this download held it. The window is very short, and three downloads are needed for the path to be shared.
- Confidence: likely

### Xconcurrency-10 [P3] A removal gives up the path before it deletes the files, and a new video can start in the folder being deleted

- Where: `crates/core/src/manager/downloads.rs`, `finish_removing`; `crates/core/src/naming.rs`, `unclaimed_path`
- Category: bug
- Failure: an unfinished video is removed. `remove_entry` frees its path under the lock, and the work folder is deleted afterwards, off it. A new video of the same name takes that path, since `unclaimed_path` does not look at the disk, and its run uses the same `<path>.wcmedia`. The deletion then removes the new run's files as it writes them, and the run fails or loses parts. A plain download is safe here because `unique_path` sees the partial files. A cancel avoids this through `entry.cleanup`, which a removal does not have.
- Evidence: the order of `remove_entry` and `delete_files` in `finish_removing`. The window is as long as the deletion, so it needs a slow disk or a large work folder, and the new video must already be waiting for its answer. A retry recovers.
- Confidence: likely

## Suggestions

### Xconcurrency-S1 Hold the part file exclusively during a run

- Today: the engine takes no lock on the part and resume files, and relies wholly on the manager's claims to keep two runs apart, so any slip in the claims becomes wrong file content (Xconcurrency-1, Xconcurrency-9).
- Better: open the part file with an exclusive advisory lock (`File::try_lock`) for the length of a run, and fail the second run with a clear error. Fixing the claims remains necessary; this is the last line of defence.
- Value: a future bookkeeping bug costs a person one failed download and a message, not a finished file with the wrong content.
