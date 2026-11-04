# Flow second-review-disk-writes: how disk writes fail

## Files read

Whole:

crates/engine/src/writer.rs
crates/engine/src/storage.rs
crates/engine/src/download.rs
crates/engine/src/checkpoint.rs
crates/engine/src/file_work.rs
crates/engine/src/abandon.rs
crates/engine/src/worker.rs
crates/engine/src/segment.rs
crates/engine/src/state.rs
crates/engine/src/target.rs
crates/engine/src/receive.rs
crates/engine/src/progress.rs
crates/files/src/whole_file.rs
crates/files/src/discard.rs
crates/streams/src/workdir.rs
crates/streams/src/download/mod.rs
crates/streams/src/download/join.rs
crates/streams/src/download/fetch.rs
crates/core/src/manager/relocate.rs
crates/core/src/manager/media_run/mod.rs
crates/core/src/manager/media_run/placement.rs
crates/core/src/manager/media_run/save.rs
crates/core/src/manager/media_run/workdir.rs
crates/core/src/manager/media_run/page.rs

In part, for the code named (the flow's size limit):

crates/engine/src/naming.rs (everything but the tests)
crates/engine/src/fetch.rs (`save`, `transfer`, `attempt`, `Sink`)
crates/engine/src/http/range.rs (`check_whole`, `check_same_file`)
crates/engine/src/http.rs (`fetch`, start)
crates/engine/src/transport.rs (`Connection::open`)
crates/engine/src/error.rs (`is_retryable`)
crates/engine/src/retry.rs (`after_failure`)
crates/streams/src/ffmpeg.rs (`run`)
crates/media/src/merge.rs (non-test code)
crates/media/src/ffmpeg.rs (`run`)
crates/core/src/naming.rs (`finished_size`, `left_by`, `is_free_on_disk`)
crates/core/src/manager/run.rs (`RunGuard`, how an engine run ends, `if_exists`)
crates/core/src/manager/file_exists.rs (`take_next_free_name`)
crates/core/src/manager/mod.rs (`stop_run`)
crates/core/src/manager/state.rs (`partial_paths`)

Also: `## Bar corrections` in `CHUNK-PLAN.md`, `fault-tests.md`, `contracts/01.md`, `03.md`, `05.md`, `15.md`.

## Findings

Checked and found sound, by reading: part file offsets never overlap or leave a counted gap (`claim` is capped by `end`, a split cuts only above `claimed`, `mark_flushed` moves only from `flushed`, a segment is released only after its writes ended); a resume file never lists a range as done before the flush that follows reading the table (except after a failed flush, Xsecond-review-disk-writes-1); a failed write is not covered by the next one and is not retried on the same stretch; a full disk ends in an error with the part and the previous resume file kept; the final rename never replaces under `Refuse` or `FreeName`; and `whole_file::replace` leaves the old file on every failure. None of this was run against a real full disk (`fault-tests.md` records it as untested).

### Xsecond-review-disk-writes-1 [P0] A failed flush at a checkpoint is forgotten, so a later flush can pass with the bytes gone

- Where: `crates/engine/src/checkpoint.rs`, `Checkpoints::spawn` and `flush_then`; `crates/engine/src/storage.rs`, `finalize`
- Category: wrong implementation
- Failure: while a download runs, one `sync_data` on the part file fails (an I/O error, or a network drive or quota that reports a failed write-back only at the flush) and later ones pass. The checkpoint logs "the next checkpoint tries again" and the download carries on. The next save, or `finalize`, then flushes without error and the state counts those ranges as on disk: a resume file lists them as done, or `Outcome::Completed` is reported, while the disk holds zeros or old bytes there. The trigger is uncommon; local disks usually refuse at the write instead.
- Evidence: `mark_flushed` already moved those ranges past `flushed`, and nothing moves it back or remembers the failure: the `Err` from `storage::blocking` in `Checkpoints::spawn` only reaches `warn!`. On Linux a failed write-back marks the pages clean and reports the error once per descriptor state, so the second `fsync` returning success does not prove the data arrived; `finalize` also flushes through a new handle, opened after the error was already seen by the saver's. Windows reports the same loss once as a "delayed write failed". The promise of `ResumeSaver` ("only ever lists as done what is on the disk itself") and of `finalize` ("a file under its final name is complete") therefore do not hold after a failed flush. Fix: after any failed flush, fail the download (or mark the ranges flushed since the last good flush as not done), so no later save or finish trusts them.
- Confidence: likely (the code path is certain; the operating system's behaviour was not reproduced here)

### Xsecond-review-disk-writes-2 [P0] A download of unknown size is reported complete and truncated when a retry's answer is shorter than what it already has

- Where: `crates/engine/src/worker.rs`, `stream`; `crates/engine/src/fetch.rs`, `Fetcher::attempt`
- Category: bug
- Failure: a download whose first answer gave no size receives 10 MiB, then the connection fails. The retry asks for the whole file again, and `skip` is 10 MiB. The server now answers `200` with a short body (an error page sent as `200`, a shorter generated file). The body ends while `skip` is still above 0. The engine treats that as the file's real end: the first 10 MiB are finalized and reported as `Outcome::Completed`, with no error. A reader of the list sees a finished file that is cut short.
- Evidence: in `stream`, `Received::End` becomes `Ended::Eof`, and for `end == UNKNOWN_END` that calls `close_unknown` and `Ended::Done`; the `skip` still left is never looked at. `check_whole` cannot catch it: `check_same_file` compares sizes only when `expected_total` is known, and it is `None` here, so a short `Content-Length` passes too, and validators absent from the first answer contradict nothing. `Fetcher::attempt` has the same hole: `position.end` is set from the short `Content-Length`, `at >= end` ends the loop at once, and the call returns `Ok` with what the earlier attempt wrote, which `wirecatch-streams` then logs as a finished segment. Fix: a body that ends with `skip > 0` is `Error::PrematureEof`, or `RemoteChanged` when no retry can help.
- Confidence: certain

### Xsecond-review-disk-writes-3 [P2] A media file made by ffmpeg or yt-dlp is put in place with its sources deleted, never flushed

- Where: `crates/core/src/manager/media_run/placement.rs`, `finish` and `place`; `crates/streams/src/download/join.rs`, `mux` and `join`
- Category: wrong implementation
- Failure: ffmpeg exits 0 or yt-dlp finishes, the file is renamed or hard-linked to its final name, the track files and parts are deleted at once (`work.remove`, `discard::dir`), and the row is marked completed. A power cut in the next seconds, before the system wrote its cache, leaves a short or empty file under the final name with every source gone. If the row's commit survived, the list shows a finished video that does not play, and a retry is not offered.
- Evidence: no `sync_all` or `sync_data` in `placement.rs`, `save.rs`, `merge.rs`, `crates/media/src/ffmpeg.rs` or on the path after `ffmpeg::run` in `join.rs`, and neither program is asked to flush. The same result from the engine is flushed before its rename (`finalize`, D30.1) and `concat` flushes each track, so only the last step lacks it. An HLS part made by `mux` gets its `.done` marker the same way. Power cuts are outside the fault tests. Fix: open the finished file for writing and `sync_all` it in `finish`, and in `join` before `work.remove`, as `crates/streams/src/download/fetch.rs` already does for segments.
- Confidence: certain for the missing flush; the result depends on a power cut

### Xsecond-review-disk-writes-4 [P2] A torn last line of `done.log` counts, and an `A` line cut short destroys the joined track

- Where: `crates/streams/src/workdir.rs`, `Log::read` and `WorkDir::open`; `crates/streams/src/download/join.rs`, `concat`
- Category: bug
- Failure: a full disk cuts `A 0 5 1234567` short as `A 0 5 12` (a write crossing a block boundary comes back short on Linux). On the next run that line wins, since its `next` is the highest, and the track file is at least 12 bytes long, so it passes. `concat` then cuts the track to 12 bytes and continues from segment 5. Segments 0 to 4 were removed earlier and are not fetched again, since `queue` starts at `next`. The result is a track missing almost everything, joined and reported complete. The window is narrow: the line must straddle a block on a disk that is full at that moment. A kill does not cause it, because the line is one write.
- Evidence: `text.lines()` also yields a last line with no newline, so a torn line is parsed. The check for `A` is `m.len() >= appended.size`, which is the only line a shorter number passes: `S` and `J` compare sizes exactly. The comment ("A line cut short by a crash, or garbage: ignored") and contract 15 ("Whole lines only count") say otherwise. The test `finished_segments_survive_only_with_the_right_size` only tears a line into something unparsable. Fix: ignore everything after the last newline, and start the next append on a new line.
- Confidence: certain for the parsing; likely for how it arises

### Xsecond-review-disk-writes-5 [P2] One failed final rename fails a complete download, and a server without ranges then downloads it all again

- Where: `crates/engine/src/storage.rs`, `finalize`; `crates/engine/src/target.rs`, `continues` and `Destination::prepare`
- Category: wrong implementation
- Failure: every byte is on disk and flushed, and the rename fails once: on Windows a virus scanner or indexer holding the new file without delete sharing, or the old file open in a player under Replace. The download ends `Failed` at once. A retry always asks the server first. For a server without ranges or of unknown size, `can_resume` is false, so `prepare` deletes the complete part file and fetches everything again, and if the link has expired the file is lost. With ranges the retry finishes only if the server still answers with the same validators, and under Replace (where the resume file is not saved as complete) it also fetches again up to the last checkpoint's ranges.
- Evidence: `finalize` calls the rename once, with no retry on a sharing or access error. `finish` saves a complete resume file only for `Finish::Refuse`. `same_file` requires `self.ranged && ranged && total.is_some()`, whatever the resume file says about completeness. The part file is `discard`ed in the `Plan::Planned(target, None)` arm. Fix: retry a rename that fails with a sharing or access error for a short bounded time, and let a later run finish a part file the resume file says is complete without discarding it.
- Confidence: likely (the code is certain; how often a rename fails was not measured)

### Xsecond-review-disk-writes-6 [P2] A saved stream is reported as failed when its temporary folder cannot be deleted

- Where: `crates/streams/src/download/join.rs`, `join`
- Category: wrong implementation
- Failure: the file is saved at its path, then `work.remove()?` fails (a scanner holding a file in the folder, a file in use). `download` returns `Error::Io` instead of `Completed`. The row shows `Failed`, in `run_stream` nothing is placed, and a retry reopens a folder that lost an arbitrary part: with `stream.txt` gone every segment is fetched again, and with the extension changed `move_to_free_name` makes another name.
- Evidence: every other cleanup step of this flow is best effort (`discarded` in `concat` and `mux`, `discard::dir` in `placement::finish`), and the doc comment of `discarded` says the folder "goes when the download ends". Only this last step turns it into a failure, after the work is done. Fix: `discarded(&dir, work.remove())` and return `Completed`.
- Confidence: certain

### Xsecond-review-disk-writes-7 [P2] A move across drives writes straight to the final name, so a kill leaves a cut-off file that looks finished

- Where: `crates/core/src/manager/relocate.rs`, `move_file`
- Category: wrong implementation
- Failure: moving a finished 8 GiB file to another drive copies it for minutes into `<folder>\name.ext` itself. A kill, crash or power cut in that time leaves a truncated file under the real name in the new folder, belonging to no row; the row still points at the old file. Nothing ever removes it. The name then counts as taken: a later move there becomes `name (1).ext`, and a later download of that name finds an existing file and can complete on it under Keep existing. Moving an unfinished download the same way leaves a partial `.wcpart` there that nothing removes either.
- Evidence: `OpenOptions::new().write(true).create_new(true).open(to)` then `io::copy`. The cleanup `discard(to)` runs only on an error returned in this process. Everywhere else a file reaches its final name only once complete (contract 01). Fix: copy to a temporary name beside `to`, flush, `rename_no_replace` to `to`, then delete the original.
- Confidence: certain

### Xsecond-review-disk-writes-8 [P3] `.wcstate.tmp` can stay beside a finished or removed download for good

- Where: `crates/engine/src/state.rs`, `ResumeState::save`; `crates/engine/src/download.rs`, `finish`; `crates/core/src/manager/state.rs`, `partial_paths`; `crates/core/src/naming.rs`, `partial_files`
- Category: bug
- Failure: `save` leaves its temporary file when a write or rename fails, or when the process is killed in the time between creating it and renaming it (which holds a flush, so it is not instant). If a checkpoint failed so (a disk that was full for a moment) and the download then completes, no later save cleans it: a `name.ext.wcstate.tmp` stays beside the finished file. Removing a killed download, or moving it, leaves one too.
- Evidence: only `save` itself removes or renames the file, and only on a run that saves again. `finish` under Replace or FreeName calls no further save, `target::discard(&target.state)` deletes only the state, and `partial_files` lists only the part and resume files, so neither removal nor `move_download_files` sees it.
- Confidence: certain

### Xsecond-review-disk-writes-9 [P3] An HLS part of a page's video is left out of the run's file work

- Where: `crates/core/src/manager/media_run/page.rs`, `fetch_part`
- Category: design
- Failure: a pause or removal during an HLS part's join, on a slow disk, passes `stop_timeout` (5 s; one 64 MiB `sync_data` can take that on a network drive), so `stop_run` aborts the task. `RunGuard` then waits for `file_work.finished()`, which does not cover this stream, so the run counts as exited while `concat` still runs. A removal then meets files still open (on Windows the folder is left behind), and a restarted run can meet the old `concat` removing segments it needs, which ends that run in an error.
- Evidence: the HLS arm builds `StreamRequest::new(url, part, private_network)` and sets `ffmpeg` and `options` but not `file_work`, so it keeps its own new `FileWork`. `stream_request` for `run_stream` and the engine arm of the same function both pass `job.file_work`. The bytes stay correct, since both runs append the same segments at the same offsets.
- Confidence: certain for the wiring; likely for the effect

## Suggestions

### Xsecond-review-disk-writes-S1 Say early when the file will not fit

- Today: Windows part files are sparse and `linux::fallocate`'s failure falls back to `set_len`, so a download with too little room runs until a write fails, possibly at 95%, and only then reports a full disk. The bytes kept are safe and it resumes once there is room.
- Better: compare the known size with the volume's free space when the download starts, and warn or hold it in line. Established download managers warn about space before or at the start.
- Value: a person learns at the start that a large file will not fit, instead of after hours of downloading.
