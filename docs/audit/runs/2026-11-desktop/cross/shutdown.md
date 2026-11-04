# Cross-module flow: shutdown

Quitting from the window, the tray and the system with downloads, media programs, file work, the bridge server and database writes in flight. Windows was traced through the GPUI source; Linux was read only, not run. No test was run for this review.

## Files read

Whole:

crates/shell/src/quit.rs
crates/shell/src/host.rs
crates/shell/src/bridge.rs
crates/desktop/src/lifecycle.rs
crates/desktop/src/main.rs
crates/desktop/src/backend.rs
crates/desktop/src/tray.rs
crates/desktop/src/closing.rs
crates/desktop/src/app_update.rs
crates/core/src/manager/mod.rs
crates/core/src/manager/ticker.rs
crates/core/src/manager/run.rs
crates/core/src/manager/relocate.rs
crates/core/src/manager/downloads.rs
crates/core/src/manager/added_pages.rs
crates/core/src/manager/media_run/mod.rs

In part, the sections this flow passes through:

crates/store/src/lib.rs (`Store` writes, `close`, the database thread)
crates/core/src/manager/captures.rs (`add_capture` end, `await_answer`, `withdraw_capture`, confirming)
crates/core/src/manager/state.rs (`Stop`, `Hold`, `Entry::stop`)
crates/core/src/manager/file_exists.rs (`take_next_free_name`)
crates/desktop/src/main_window/mod.rs (`bring_up`, the close step)
crates/streams/src/process/tree.rs (`Tree::kill`, its `Drop`)
crates/desktop/packaging/windows/installer.iss (Restart Manager lines)
GPUI, outside the repo: `gpui-pre-0.3.10/src/app.rs` (`Application::run`, `on_quit`, `shutdown`), `gpui-pre-windows-0.3.10/src/platform.rs` (`run`, `quit`, `handle_end_session`)
tokio 1.54.1: `runtime/runtime.rs` and `runtime/blocking/pool.rs` (`Drop`)

Left out: the engine's `FileWork` and writer, `crates/core/src/manager/addons/`, `media.rs` and `crates/server` beyond their contracts; `crates/ipc` (endpoint release); GPUI's Linux platform.

## Findings

### Xshutdown-1 [P1] Quitting during a move to another folder leaves the list pointing at the old path

- Where: `crates/core/src/manager/mod.rs`, `Manager::shutdown`; `crates/core/src/manager/relocate.rs`, `finish_moving`, `move_files`, `finish_move`
- Category: bug
- Failure: a person moves a download to a folder on another drive (`set_download_folder`), which copies the file, and quits from the tray while it copies. The list closes and the app exits, but the copy runs to its end and deletes the original. On the next start a finished download's row still names the old path, where no file exists, and restore leaves `Completed` rows as they are. An unfinished download finds no part file at its old path and starts again from zero, with its part and resume files stranded in the new folder for good. If the process is ended outright instead (system shutdown, `Xshutdown-S1`), the original stays, and a truncated copy stays in the new folder under the final name with nothing to clean it up.
- Evidence: `shutdown` waits only for `entry.run`'s `exited`; `Hold::Moving` is never looked at, though `finish_removing` shows the token to wait on (`move_under_way`). `move_file`'s copy and `remove_file` run on the blocking pool, which tokio finishes on exit (see `Xshutdown-3`), while the task awaiting it is dropped, so `finish_move` never runs. If `finish_move` does run after the close, `self.save` reaches `Store::send`, which logs "the database is closed; change not saved". `finish_moving` also uses `self.lock()`, not `lock_open`, so a move that was waiting for its run to stop goes on to move the files after `state.closed` is set. `take_next_free_name` reaches `move_files` the same way. While the old process finishes the copy, it has no window or tray icon, and a new launch can open the same list and resume at the path being moved. No test in `crates/core/tests/manager` combines a move with `shutdown`.
- Confidence: certain for the unsaved path; the copy finishing rests on the same reading as `Xshutdown-3`

### Xshutdown-2 [P2] A removal in flight at quit is dropped, so the download returns and a refused capture is downloaded twice

- Where: `crates/core/src/manager/downloads.rs`, `finish_removing`, `remove_in_background`; `crates/core/src/manager/captures.rs`, `add_capture`'s `Err` arm of `await_answer`
- Category: bug
- Failure, removal: a person removes a running download and quits before its run has stopped (up to `stop_timeout`, longer when the removal waits for a move). The download is back in the list on the next start, `Paused`, with its partial files kept.
- Failure, capture: the browser hands over a download whose server has not answered yet (the wait is up to `capture_timeout`, 8 s) and the person quits. The app answers `NotStarted`, so the browser keeps its copy, but the row stays saved as running, and the next start restores it and downloads it too. Two copies, with the app having told the browser it took nothing.
- Evidence: `begin_remove` is memory only (`Hold` is not saved). `finish_removing` calls `lock_open` again after waiting on `exited`; `shutdown` set `state.closed` before that wait can end, so it returns `CoreError::Closed` before `delete_downloads`. `Stop::Remove` is a person's choice that `Stop::Shutdown` does not replace, so `finish` saves `Paused`. In a capture, the run's exit wakes `await_answer` with `server_answered()` false, then `remove_in_background` calls `remove`, whose first `lock_open` fails, leaving only a warning in the log. `Stop::Shutdown` keeps the status, and restore turns it into `Queued` with `resume_first`. The bridge server keeps serving through the stop, so the `NotStarted` answer does reach the browser. The store's own doc calls this outcome out: "a lost delete would bring back a removed download". D27.9 records a neighbouring limit (handover ids lost over a restart), but not this one, where the app refused the download itself.
- Confidence: certain from the code; not run

### Xshutdown-3 [P1] The quit time limit does not end the process: a stuck blocking task keeps it alive with no window or tray icon

- Where: `crates/shell/src/quit.rs`, `quit_sequence`; `crates/desktop/src/backend.rs`, `Backend` (owns `tokio::runtime::Runtime` as a GPUI global); `crates/desktop/src/lifecycle.rs`, `quit`
- Category: bug
- Failure: a download's file work hangs on a dead network share or a removed disk. `Manager::shutdown` waits on `exited`, which waits for `FileWork::finished()`, so `QUIT_PATIENCE` runs out and the log says "exiting anyway". The app then calls `cx.quit()`, and `main` never returns while that file work is stuck, since dropping the runtime waits for it. The process stays in the task list holding the part file, with nothing on screen to say so. A long copy (`Xshutdown-1`) or a slow disk does the same for as long as it lasts.
- Evidence: the path to the drop: `WindowsPlatform::quit` posts `WM_QUIT`, `run` returns, `Application::run(self, ..)` ends and drops its `Rc<AppCell>`. Only `wasm` calls `mem::forget`, and every other holder found (`on_quit` and the platform callbacks, `AsyncApp`) is weak, so `App` and its globals, `Backend` with its `Runtime` among them, drop on the main thread. `impl Drop for BlockingPool` calls `self.shutdown(None)`, with no time limit, which waits for blocking threads running a task. `Runtime::shutdown_timeout` or `shutdown_background` is never used, and `QUIT_PATIENCE`'s doc says "the backstop". The comment in `RunGuard::drop` also expects a runtime shutting down.
- Also: when the limit runs out, the dropped `shutdown` future skips `save_batches_at_shutdown` and `store.close`, for every download, because of the one stuck run. Rows saved by finished runs are safe, as the database thread applies what it has received.
- Confidence: likely. Every step is read from source, but `App` being dropped was not observed. A small GPUI program with a `Drop` global and a stuck `spawn_blocking` task would settle it.

### Xshutdown-4 [P2] On Linux, media programs outlive an app ended by a signal or a crash

- Where: `crates/streams/src/process/tree.rs`, `Tree`
- Category: wrong implementation
- Failure: the app gets `SIGTERM` (`kill`, a logout that does not kill the whole session) or crashes while yt-dlp or ffmpeg runs. The program goes on downloading or converting, unseen, using bandwidth and writing into `<file>.wcmedia`. The next start runs the same job on the same work folder, so two programs write there.
- Evidence: on Unix the group is killed only from `Tree::kill`, reached by cancel, time limit or `Drop`, none of which run when the process is ended. Nothing in `crates/` handles `SIGTERM`, and nothing sets `PR_SET_PDEATHSIG`. On Windows, `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE` covers this: closing the handle at process death kills the job. A normal quit is fine on both, since the tasks drop as the runtime goes down.
- Confidence: likely; read only, Linux not run

Checked and found sound:

- Window, tray and update quits all go through `lifecycle::quit`, safe to repeat, with `QuitMode::Explicit` keeping the app alive until the stop ends.
- `state.closed` stops new starts: `reconcile` does nothing once closed, and a person's stop reason survives `Stop::Shutdown`.
- A run ended at quit saves its reading in `finish`, and `Processing` returns to `Downloading`.
- The database thread applies everything sent before `Close`, and its back-off is cut short by a close.
- `free_endpoint` ends the connections and the listener before `start_again`. Background tasks all listen to `stopped`. Page reads are not cancelled by `shutdown`, but they end when the runtime drops their tasks, killing their programs through `Tree`.
- `Closed` reaches the browser as a plain message ("Wirecatch is shutting down").

## Suggestions

### Xshutdown-S1 Stop cleanly when the system ends the session

- Today: Wirecatch registers no `on_app_quit` and handles no signal. On Windows, `WM_ENDSESSION` (log off, restart, and the installer's Restart Manager, `CloseApplications=force`, even during a quit already under way) reaches GPUI's `handle_end_session`, which calls `std::process::exit(0)` at once. On Linux, `SIGTERM` ends the process. The result equals a kill, which `FT-manager-kill` shows recovers: up to 1 s of data is fetched again and up to 2 s of a row's counter is behind until the engine reads its resume file. Writes still in the channel are lost, and `save_batches_at_shutdown` does not run.
- Better: stop runs and close the store from `on_app_quit` and a signal handler. GPUI waits only `SHUTDOWN_TIMEOUT` (200 ms) for those handlers, so a longer stop needs `ShutdownBlockReasonCreate` or a wait inside the handler. IDM and similar managers resume after shutdowns, as Wirecatch already does, so the gain is only a tidier stop.
- Value: nothing is fetched twice after a restart, and the guarantee in D13 holds for every way of quitting.

### Xshutdown-S2 Say that work is still finishing when a quit has to wait

- Today: windows and the tray icon go at once, and the process then lingers for as long as the stop takes, up to 25 s, or longer in `Xshutdown-3`. A move in flight gets no warning.
- Better: when a move is under way, ask before quitting, or keep the tray icon with a "Finishing up" tooltip until the stop ends.
- Value: a person does not eject a disk or shut down thinking the app has finished.
