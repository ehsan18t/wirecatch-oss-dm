# Flow second-review-resume: how resume fails

A second, independent review of resume, looking only at failure: stale, cut short, foreign or older files, a server whose answer changed, and the app dying at each step. Read at the working tree of branch `docs/record-open-items`. Nothing was built or run; each finding comes from reading the code, so none has a failing test yet.

Checked and found sound: the engine's flush order (ranges read, part file flushed, temporary resume file flushed, rename), `mark_flushed` and the hole rule, splits above `claimed`, `ranges_fit` and the part length check, the order of finish (flush, rename, delete the resume file), a resume file left beside a finished file (the next run starts over), `left_by` for a run killed before its path was saved, and the stream log's `S` and `J` lines, whose exact size check rejects a cut line.

## Files read

Whole:

crates/engine/src/state.rs
crates/engine/src/target.rs
crates/engine/src/download.rs
crates/engine/src/checkpoint.rs
crates/engine/src/segment.rs
crates/engine/src/writer.rs
crates/engine/src/worker.rs
crates/engine/src/storage.rs
crates/core/src/manager/restore.rs
crates/core/src/manager/run.rs
crates/core/src/manager/settle.rs
crates/core/src/manager/file_exists.rs
crates/core/src/naming.rs
crates/core/src/manager/media_run/mod.rs
crates/core/src/manager/media_run/workdir.rs
crates/core/src/manager/media_run/page.rs
crates/core/src/manager/media_run/save.rs
crates/core/src/manager/media_run/placement.rs
crates/streams/src/workdir.rs
crates/streams/src/download/mod.rs
crates/streams/src/download/fetch.rs
crates/streams/src/download/join.rs

In part, for the code a finding depends on:

crates/core/src/manager/relocate.rs (moving and `move_file`)
crates/core/src/manager/downloads.rs (`retry`, `cancel`, removal, `refresh_link`, `insert`, the new row)
crates/core/src/manager/state.rs (`partial_paths`, `stop`)
crates/media/src/ytdlp/download.rs (the run and its arguments)
crates/streams/src/plan.rs (`fingerprint`)
crates/streams/src/model.rs (`StreamRequest::new`)

Also: docs/audit/AUDIT-PLAN.md, CHUNK-PLAN.md, fault-tests.md, contracts 01, 02, 04, 05, 15 and 17, docs/ENGINEERING.md, D34 in docs/DECISIONS.md. Contracts 04 and 17 name findings 04-1 and 17-1, which Xsecond-review-resume-1 and -2 may overlap; those findings themselves were not read.

## Findings

### Xsecond-review-resume-1 [P1] After a restart, a download that never ran is marked finished with a file that is not its own

- Where: `crates/core/src/manager/restore.rs`, `restore_entry`; `crates/core/src/naming.rs`, `finished_size`; `crates/core/src/manager/downloads.rs`, `insert`; `crates/core/src/manager/file_exists.rs`, `replace_at`
- Category: bug
- Failure: `video.mp4` is already in the folder. The person adds a video that takes the same name, and it waits in its queue behind the limit, or was added paused, or failed in `record_media_failure`. The app is closed normally and opened again. The row becomes Completed, pointing at the old `video.mp4`: nothing was downloaded, its cookies and headers are wiped, and no message is shown. The same happens to a plain download answered with Replace, or under the Replace setting, for as long as it waits for a slot or for the server's first answer. A crash after a video was placed under a free name (`video (1).mp4`, before the row is saved) also ends with the row completed on the other file at `video.mp4`.
- Evidence: `restore_entry` trusts any row with `naming.settled` for which `finished_size` is `Some`. A media row is settled from the moment it is added (`settled: media_job.is_some()`), and on purpose takes a name with a file on it (`unclaimed_path`, `placed_later`). `replace_at` saves `settled = true` at the found path, while `on_existing` is memory only. `finished_size` sees only that no part file, resume file or work folder is there, and before a run starts there are none, because `page_attempt`, `run_stream` and `Destination::prepare` create them later. `mark_completed` then overwrites `total` with the size found, which hides a mismatch even when the row knew the real size. `a_download_whose_file_finished_is_completed_not_rerun` covers only the plain case. The comment "A provisional path has no file of this download's yet" states the rule, but a media or Replace path is the same kind of path with the flag set.
- Confidence: certain

### Xsecond-review-resume-2 [P1] A media work folder's finished parts carry no identity, so another download's video is saved as this one

- Where: `crates/core/src/manager/media_run/page.rs`, `page_attempt` and `fetch_part`; `crates/core/src/manager/media_run/workdir.rs`, `Downloaded::read`; `crates/streams/src/plan.rs`, `Plan::fingerprint`
- Category: wrong implementation
- Failure: a `<name>.wcmedia` folder survives its download, then a new media download gets the same name, which `unclaimed_path` allows without looking at the disk. If `downloaded.json` and its files are there, the new row reads no page and downloads nothing: the old parts are joined and placed as the new video, with exit status success and no sign of it. If only `video.done` is there, the old video part is joined with the new audio. A different quality of the same title is lost the same way. `refresh_link` on a media row that failed in conversion has the same result with a link to another page. A stream job is guarded by its fingerprint, but that holds no host and drops signing values, so two sites using the same paths (`/hls/1.ts`) also share a folder.
- Evidence: `Downloaded::read` checks only that the listed names are plain files in the folder. `fetch_part` checks only that the marker names an existing file. `placing.done` in the same folder does carry `link_digest`, and `waiting_to_place` compares it, so the need was seen for that one file. Engine parts have the same gap: `exact_path` compares no link. A folder survives when the app dies between `state.remove_entry` with `store.delete_downloads` and the deletion in `finish_removing`, when it dies during `discard_partial`, or when `wirecatch_files::discard::dir` in `finish` cannot delete a locked file and only logs. Finishing also frees the name, as a completed row holds no claim, so a later download of the same title meets that folder.
- Confidence: likely (the trust is certain; a surviving folder needs one of the listed triggers)

### Xsecond-review-resume-3 [P2] A stream log line cut short is read as a smaller track, which truncates the track and silently drops its segments

- Where: `crates/streams/src/workdir.rs`, `Log::read` and `WorkDir::open`; `crates/streams/src/download/join.rs`, `concat`
- Category: bug
- Failure: the last line of `done.log` is `A 0 5 12` where `A 0 5 1234567` was being written. This takes a short write, as Linux can return on a full disk when the line crosses a block; `write_all` then fails and the download reports the disk error. The person frees space and retries. `Log::read` accepts the line, so segments before 5 count as appended at size 12. `concat` cuts the track file to 12 bytes and appends from segment 5. The joined track lacks segments 0 to 4, with no error, and ffmpeg with stream copy accepts a transport stream with a piece missing.
- Evidence: `text.lines()` also returns a final line with no newline, and it splits into the four fields `["A", "0", "5", "12"]`. The comment says "A line cut short by a crash, or garbage: ignored", but only a cut that breaks the field count or a number is ignored. `A` is the only line trusted by a lower bound (`m.len() >= appended.size`) and `later` compares only `next`, so the cut line wins over the good one before it. `S` and `J` need the exact size, so they are safe. The test `finished_segments_survive_only_with_the_right_size` tries only cuts that do not parse. A line without its newline should not count.
- Confidence: likely (the parse is certain; the short write has not been produced)

### Xsecond-review-resume-4 [P2] A passing read error on a resume file or stream header deletes everything already downloaded

- Where: `crates/engine/src/target.rs`, `saved_state`, `part_len`, `plan_exact` and `Destination::prepare`; `crates/streams/src/workdir.rs`, `WorkDir::open`
- Category: wrong implementation
- Failure: a download at 40 GiB continues while an antivirus holds `f.bin.wcstate` without sharing it, or a network drive answers one read with an error. `ResumeState::load` fails with `LoadError::Read`, `plan_exact` gets no state, and `prepare` deletes the part file and starts from zero. Nothing about the file was wrong and a retry a moment later would have worked. A failed `metadata` on the part file does the same through `part_len`. In a stream, any error reading `stream.txt` counts as not reusable, and `remove_dir` clears every segment and track.
- Evidence: only `NotFound` is told apart in `saved_state`, and it is logged and answered `None` as damage is. `part_len` likewise maps every other error to `None`, which fails `continues`. In `WorkDir::open`, `written.is_ok_and` makes an `Err` the same as another plan. The test `a_resume_file_that_cannot_be_read_is_logged_with_the_reason` fixes the logging and its comment names the antivirus case. D34.2 accepts a fresh start when the server says the file changed; this case has no such evidence. A read error other than not found should end the run with an error that can be retried and leave the files alone.
- Confidence: likely

### Xsecond-review-resume-5 [P2] Zeros are trusted as downloaded when a stale resume file could not be deleted

- Where: `crates/engine/src/target.rs`, `Destination::prepare` and `discard`
- Category: wrong implementation
- Failure: a part file has the wrong length (cut short from outside, or `part_len` failed), so the run starts over. `discard(&target.state)` fails because something holds the file, and that is only logged. `create_part` then empties the part file and sizes it to the total. The first `saver.save()` hits the same hold on its rename and returns an error, or the app dies before that save. The next run finds the old resume file readable, the same server answer and a part file of the right length, so it continues: the ranges the old state called done stay zeros, and the file finishes with exit status success.
- Evidence: the steps run in the order `discard(part)`, `discard(state)`, `create_part`, with no check of the second. The comment on `discard` says a resume file "is replaced or never matches again", which does not hold when the reason for starting over was the part file: the state still matches the server. The part length check cannot help, as a new part file has the full length from the start. It takes two faults together, which is why this is not a P0. Removing the resume file first, and ending the run if that fails, closes it.
- Confidence: likely

### Xsecond-review-resume-6 [P2] An HLS part of a page's video runs outside the run's file work

- Where: `crates/core/src/manager/media_run/page.rs`, `fetch_part`
- Category: bug
- Failure: a page's video downloaded by Wirecatch over HLS is stopped while its blocking file work is slow: a decrypt, a sync or a large `concat` on a slow disk. After `stop_timeout` the task is aborted, and `RunGuard` reports the run over at once, as the file work it waits for has nothing in it. A Cancel or Remove then deletes the folder while that work still writes and holds files open, so on Windows the deletion can fail and leave the folder for Xsecond-review-resume-2. A new run opens the same track file beside the old `concat`, whose batch end still records its `A` line and deletes segments the new run is about to read, so the new run fails with file not found.
- Evidence: `fetch_part` builds `StreamRequest::new(url, part, private_network)` and sets `ffmpeg` and `options` only, so `file_work` stays the `FileWork::new()` from `StreamRequest::new`. Its engine branch sets `request.file_work = job.file_work.clone()`, and so does `stream_request` for a stream job. Contract 04 relies on this: "a waiter on `exited` knows the files are closed".
- Confidence: certain for the missing field; likely for the failures, which need a stop that outlasts `stop_timeout`

### Xsecond-review-resume-7 [P2] A finished video is never flushed to the disk before the parts it came from are deleted

- Where: `crates/core/src/manager/media_run/placement.rs`, `finish` and `place`; `crates/streams/src/download/join.rs`, `mux` and `join`
- Category: wrong implementation
- Failure: ffmpeg joins or converts a video and exits. The output is moved to its final name, the track files or parts are deleted and the row is saved as Completed, while the output may still be only in the system's write cache. A power cut in the next moments leaves a Completed row with a short or zero-filled file, and the data to rebuild it is gone.
- Evidence: there is no `sync_all` or `sync_data` in `crates/media/src` or `crates/streams/src/ffmpeg.rs`, and in `crates/core/src/manager` only `move_file` has one. ffmpeg does not sync by default, and `ffmpeg::run` returns on exit status alone. For plain files `storage::finalize` flushes before its rename, and its documentation promises that "a file under its final name is complete even after a power cut"; streams sync every segment and track on the way. Only this last step drops the promise. Power cuts are untested (fault-tests.md).
- Confidence: likely

### Xsecond-review-resume-8 [P3] Dying between placing a video and deleting its work folder gives a second copy or a question about its own file

- Where: `crates/core/src/manager/media_run/placement.rs`, `finish`; `crates/core/src/naming.rs`, `finished_size`
- Category: bug
- Failure: the app dies after `place` and before `discard::dir` ends, which on a slow disk with large parts can take seconds. After the restart `finished_size` sees the work folder, so the row runs again. With `downloaded.json` intact it joins again and `place` finds its own earlier file: under Keep both it saves `video (1).mp4`, under Ask it asks about its own file. If the single part was already moved out, `Downloaded::read` fails, `fetch_part` finds its marker's file gone, and the whole video is downloaded again first.
- Evidence: nothing is recorded between the placing and the deletion. `placing.done` is written only for the Ask case. The files stay correct, so this costs a duplicate or a repeat download only.
- Confidence: likely

## Suggestions

### Xsecond-review-resume-S1 Check kept bytes when a server gives no validators

- Today: with no `ETag` and no `Last-Modified`, the same size is all that says a resume is the same file (`same_file`: both absent counts as the same), and `plan_exact` compares no link. D34.2 records this limit for answers within one run. A refreshed link to another file of one size, or a file replaced in place, is joined undetected.
- Better: when no validator proves anything, fetch a small stretch of the kept bytes at resume, such as the last 64 KiB before the first missing range, and compare it, starting over on a difference. Established managers mostly accept this risk, so this would go beyond them for the one case that has no other protection.
- Value: the case most likely to give a silently mixed file gets caught, for one small request.

### Xsecond-review-resume-S2 Finish a complete part file without asking the server

- Today: `download_guarded` calls `transport::open` before it looks at any file. A resume file listing nothing left (the app died just before the rename, or a file saved under the name moved it through `take_next_free_name`) still needs a live answer with the same validators. If the signed link has expired, the download ends Failed with all its bytes on disk, and a server without ranges downloads it again.
- Better: when the resume file lists nothing left and the part file has its saved total, finish first, and only ask the server when something is missing.
- Value: a fully downloaded file is never lost to an expired link, and the person is not told it failed.
