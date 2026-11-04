# Cross-module flow: crash-and-resume

What happens to a normal, stream or media download and its saved row when the app dies at any moment, and how the next start restores it. Nothing was built or run: every finding comes from reading the code, and the fault tests in `fault-tests.md` (`FT-manager-kill`, `FT-kill`) did not reach these states, because their kills fall while data flows.

## Files read

Whole:

crates/core/src/manager/restore.rs
crates/core/src/manager/startup.rs
crates/core/src/manager/run.rs
crates/core/src/manager/settle.rs
crates/core/src/manager/file_exists.rs
crates/core/src/manager/relocate.rs
crates/core/src/manager/added_pages.rs
crates/core/src/manager/media_run/mod.rs
crates/core/src/manager/media_run/page.rs
crates/core/src/manager/media_run/placement.rs
crates/core/src/manager/media_run/save.rs
crates/core/src/manager/media_run/workdir.rs
crates/core/src/naming.rs
crates/engine/src/target.rs
crates/engine/src/state.rs
crates/engine/src/download.rs
crates/streams/src/download/mod.rs
crates/streams/src/download/join.rs

In part, the sections on this flow:

crates/core/src/manager/downloads.rs (1-480, 518-557, 590-689)
crates/core/src/manager/state.rs (560-774)
crates/core/src/manager/mod.rs (385-469, `Manager::open`)
crates/core/src/manager/captures.rs (595-654)
crates/engine/src/storage.rs (1-300)
crates/streams/src/workdir.rs (1-220)
crates/streams/src/process/tree.rs (1-130)
crates/media/src/ytdlp/download.rs (95-229)
crates/media/src/ytdlp/cookies.rs (1-92)
crates/queues/src/schedule.rs (1-110)

Left out to stay near the size limit, relying on `contracts/03.md`, `04.md`, `15.md` and `17.md`: `crates/store/src/lib.rs`, `crates/records`, `crates/streams/src/download/fetch.rs`, `crates/core/src/batches.rs`, `crates/core/src/manager/media.rs`, `crates/core/src/manager/media_run/extras.rs`. So the store's own write and repair paths and a batch's state across a crash are not covered by this review.

Checked and found to line up: cookies files left by a crash (`Manager::open` deletes `tmp`), a torn `downloaded.json`, `placing.done` or `.done` marker (each falls back to doing the work again), stream segments and joined tracks after a kill, a question lost at restart (the row stays Paused and asks again when resumed), a scheduled queue's run continuing.

## Findings

### Xcrash-and-resume-1 [P1] A restart marks a download Completed with a file it never downloaded

- Where: `crates/core/src/manager/restore.rs`, `restore_entry`; `crates/core/src/naming.rs`, `finished_size`; `crates/core/src/manager/downloads.rs`, `insert` and `add_entry`; `crates/core/src/manager/file_exists.rs`, `replace_at`
- Category: bug
- Failure: `restore_entry` reads "settled, with a file at the path and no partial files beside it" as "this download finished". Two ordinary states match it with a file that is not the download's. (a) A video or stream with a media job, paused with Download later or waiting in its queue, whose name is already taken in the folder, for example the same video saved earlier. (b) A file download answered Replace in the file-exists question and still waiting for a slot. After any restart, a clean quit included, the row is Completed with the old file's size and its cookies and headers wiped. Nothing was downloaded, the old file is shown as the result, and no error appears. With (b) the person asked for that file to be replaced.
- Evidence: `add_entry` sets `settled: media_job.is_some()` at creation, and `insert` picks a media job's path with `unclaimed_path`, which does not look at the disk (D96 settles a taken name at placing), so the path can hold another file while no work folder exists yet. `replace_at` sets `settled = true` through `settle_at` and status `Queued` before any part file exists, and `on_existing` is not saved. `finished_size` then finds no part file, resume file or work folder and returns the size, and `mark_completed` clears `origin`. The test `a_download_whose_file_finished_is_completed_not_rerun` uses `media: None` and a stubbed `finished_size`, so it does not meet either state. A settled row at a path that had a real run differs only in having had partial files, which the row does not record.
- Confidence: certain

### Xcrash-and-resume-2 [P2] A crash between the engine's rename and deleting the resume file downloads the file again and keeps two copies

- Where: `crates/engine/src/download.rs`, `finish`; `crates/engine/src/target.rs`, `plan_exact`; `crates/core/src/naming.rs`, `finished_size`; `crates/core/src/manager/run.rs`, `finish`
- Category: wrong implementation
- Failure: the app or computer dies after `storage::finalize` renamed the part file and before `target::discard` removed `x.bin.wcstate`, or before that deletion reached the disk. `x.bin` is complete and `x.bin.wcstate` is beside it, with no part file. On restart the row is not completed. The whole file is downloaded again, then refused at the finish, and saved as `x (1).bin`; the row points there and `x.bin` stays behind as an unlisted copy. If the link has expired by then, the row ends Failed although the complete file is on disk.
- Evidence: the engine's contract names this state ("a crash between rename and deleting the resume file leaves both"), but core does not recognise it: `finished_size` returns `None` when either partial file exists. The next run has `ExistingFile::Refuse` (`on_existing` is `None`), `continues` fails as `part_len` is `None`, and `Plan::Planned(target, None)` deletes the resume file and creates an empty part file. `finalize` then gives `TargetTaken`, and `finish` with no stop calls `take_next_free_name`. A kill has only an instant here. A power cut has longer: the rename is written through on Windows (`MOVEFILE_WRITE_THROUGH`) while the deletion and the row's commit (`synchronous=NORMAL`) are not.
- Confidence: certain for the path, not run

### Xcrash-and-resume-3 [P2] A video put in place before the crash is treated as someone else's file on the next run

- Where: `crates/core/src/manager/media_run/placement.rs`, `finish` and `place`; `crates/core/src/manager/restore.rs`, `restore_entry`
- Category: wrong implementation
- Failure: nothing durable records that a media run placed its file. If the app dies after `place` and before `discard::dir(work)` has finished, the work folder keeps the row from completing, and the run starts again. A single-file video is read and downloaded again whole, since its file left the folder and `Downloaded::read` returns `None`. A converted one is converted again from the kept parts. Then `place` finds the run's own finished file at the name: under the default setting the person is asked whether to replace a file that is this download's, and under Keep both a second copy is saved. If the app dies after the folder is gone but before the row's commit, and the file went to a path other than the planned one (`final_path` changed the extension, or Keep both took `name (1)`), the row still has the planned path: with nothing there the video is downloaded again from the start, and with another file there Xcrash-and-resume-1 marks the row Completed with that file.
- Evidence: `finish` runs `place`, then the subtitles, then `discard::dir(&work)`, and only afterwards returns `MediaDone::Completed`, which `run::finish` saves. `remember_placing` writes `placing.done` only for `Placed::Asks`. `when_file_exists` defaults to `Ask` (`crates/settings/src/lib.rs`). Deleting a work folder holding the original parts of a large video is the longest part of this window.
- Confidence: likely (window depends on folder size; path certain, not run)

### Xcrash-and-resume-4 [P2] A crash during a move to another folder leaves a cut-off file under the final name, or strands the data

- Where: `crates/core/src/manager/relocate.rs`, `move_file`, `move_files` and `finish_move`; `crates/core/src/manager/file_exists.rs`, `take_next_free_name`
- Category: wrong implementation
- Failure: the row's path is saved only after every file has moved, and nothing is saved before. (a) A finished download moved to another drive is copied straight to its final name, so a crash during the copy leaves a cut-off `new\x.bin` that looks like the real file, while the row still points at the old one. A later move takes `x (1).bin`, and nothing ever removes the cut-off file. (b) An unfinished download in the same copy leaves a partial `x.bin.wcpart` in the new folder for good, which also holds the name there through `is_free_on_disk`. (c) A crash after both files moved but before the commit brings the row back at the old path with no files: it starts from zero, and all the progress sits unlisted in the new folder. (d) In `take_next_free_name`, the old path holds the other program's file, so (c) makes `finished_size` answer `Some`: the row is Completed with that file, and the real data stays in `x (1).bin.wcpart`.
- Evidence: `move_file` falls back to `create_new` on `to` and `io::copy`, and its clean-up runs only on an error the process lives to see. `Entry::stranded` is set only after a failed move, in `move_files`, so `restore` has no record of a move under way, and `Hold::Moving` is memory only. The engine keeps its rule that nothing is at the final name until the data is complete; this path does not. The copy lasts as long as the file takes to copy, so (a) and (b) have a window of minutes for a large file; (c) and (d) have a window of about one commit.
- Confidence: certain for the path, not run

### Xcrash-and-resume-5 [P2] After a restart, a captured download no longer checks that the server sent the file the browser saw

- Where: `crates/core/src/manager/restore.rs`, `restore_entry`; `crates/core/src/manager/settle.rs`, `check_expectation` and `read_as_video`; `crates/core/src/manager/state.rs`, `Entry`
- Category: wrong implementation
- Failure: a browser capture is waiting in its queue or paused, not yet answered by a server, and the app restarts or crashes. When it then runs and the server answers with a sign-in page or a different size, the check that refuses this is gone. The page is saved under the browser's name, such as `report.pdf`, and the row is Completed. In the same way, a link added in the app that was waiting loses `video_check`, so a video page downloads as an HTML file instead of being read for its video, including one whose run was killed before the server answered.
- Evidence: `restore_entry` sets `expect: None` and `video_check: false` for every row, and `StoredDownload` (`Entry::stored`) holds neither. Both matter only while `naming.settled` is false, a state that does survive the restart. `settle_path_checking` runs `check_expectation` only `if let Some(expect)`. `docs/DECISIONS.md` records no choice to drop them (no mention of `Expectation` or `video_check`).
- Confidence: certain

### Xcrash-and-resume-6 [P3] A saved download that gives way to reading its page loses its link if the app stops before the question is answered

- Where: `crates/core/src/manager/added_pages.rs`, `read_answered_page`
- Category: design
- Failure: a saved link added in the app, for example one waiting in a queue overnight, runs and its server answers with a web page. Its row is deleted from the store at once, and the only record of the link becomes a quality question in memory. If the app crashes or quits before the person answers, the link is gone from the list with no trace or notice.
- Evidence: `read_answered_page` calls `remove_in_background` and then `hold_media`, which `contracts/17.md` describes as a `MediaPending` in `state.media`; `contracts/04.md` lists open questions as not saved. `crates/core/src/manager/media.rs` was not read whole, so a path where saved choices answer at once may avoid the wait.
- Confidence: likely

## Suggestions

### Xcrash-and-resume-S1 Stop a download that crashes the app from crashing it at every start

- Today: the workspace `Cargo.toml` sets `panic = "abort"`, so in a release a panic in a run ends the process. `RunGuard`'s `RunEnd::Vanished` and `stopped_unexpectedly` cannot be reached from a panic there, and `a_run_that_panics_is_recorded_as_failed` proves them only under the test profile, which unwinds. At the next start that download gets `resume_first`, so a panic it causes would repeat at once. No such panic is known.
- Better: save a mark when a run starts and clear it when the run ends cleanly, and hold back with a notice a download that was running through several crashes in a row. Browsers offer the same kind of guard for a session that keeps crashing.
- Value: one bad download cannot make the app unusable until the list is edited by hand.

### Xcrash-and-resume-S2 Offer to clean up partial files no row owns

- Today: after `Recovery::Damaged` starts an empty list, or a row deleted just before a crash, `.wcpart`, `.wcstate` and `.wcmedia` left in download folders stay for good. The notice only says finished files are still in their folders. A plain download added again continues through `left_by`, but anything else leaves them behind.
- Better: at start, or from the repair notice, list partial files in the folders the app knows about that no row claims, and offer to delete them.
- Value: disk space comes back, and later downloads of that name do not get a ` (1)` for no visible reason.

### Xcrash-and-resume-S3 Add file corruption to the open Linux item on programs left running

- Today: `docs/ROADMAP.md` lists yt-dlp and ffmpeg outliving a crashed app on Linux, so it is not reported here as a finding. The entry reads as a leftover process. The flow adds a sharper result: `resume_first` restarts the same media run at once on the same work folder, so a surviving ffmpeg and the new one can write `output.<ext>` or `joined.<ext>` together, and `ffmpeg::run` takes exit 0 as success. Not tested; Linux is out of this audit's testing, and whether ffmpeg survives its closed pipes was not checked.
- Better: note this in the roadmap entry, and include two writers in that item's test.
- Value: the item is weighed by its worst result when the release decision is made.
