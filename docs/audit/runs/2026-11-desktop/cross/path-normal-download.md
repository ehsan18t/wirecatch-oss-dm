# Cross-module flow: path-normal-download

A normal download from its entry point (browser handover, Add dialog, CLI, second launch) through to the finished file on disk, looking only at what one layer assumes of the next.

## Files read

Whole:

crates/desktop/src/add_dialog.rs
crates/desktop/src/links.rs
crates/shell/src/launch.rs
crates/server/src/lib.rs
crates/core/src/naming.rs
crates/core/src/headers.rs
crates/core/src/manager/downloads.rs
crates/core/src/manager/arrival.rs
crates/core/src/manager/placement.rs
crates/core/src/manager/policy.rs
crates/core/src/manager/settle.rs
crates/core/src/manager/file_exists.rs
crates/core/src/manager/run.rs
crates/core/src/manager/state.rs
crates/core/src/manager/captures.rs
crates/core/src/manager/handovers.rs
crates/core/src/manager/added_pages.rs
crates/core/tests/manager/file_exists.rs
crates/engine/src/target.rs
crates/engine/src/download.rs
crates/engine/src/storage.rs

In part, for one fact: `crates/core/src/manager/restore.rs` (`finished_size` use), `crates/core/tests/manager/protocol.rs` (`a_queued_capture_that_gets_another_file_offers_a_fresh_link`), `crates/messages/src/lib.rs` (`TargetRejected`), `crates/settings/src/lib.rs` (`folder_for`), `crates/settings/src/folders.rs` (`resolve`), `crates/shell/src/api.rs` (add commands), `crates/desktop/src/download_windows/mod.rs`, `apps/extension/src/lib/native/timing.ts`.

Left out to stay within the size limit, taken from contracts 01, 02, 06, 07 and 32: the bridge's relay, `crates/cli/src/main.rs`, and the engine's `worker`, `transport`, `http` and `segment`. No mismatch was found at those boundaries from the contracts: the extension's 10 s wait outlasts the 8 s capture timeout, and an oversized frame ends in a withdrawal that keeps the browser's copy.

## Findings

### Xpath-normal-download-1 [P1] "If the file is already in the folder" is skipped when the name comes from the link or the browser

- Where: `crates/core/src/manager/downloads.rs`, `Manager::insert`; `crates/core/src/naming.rs`, `settled_name`; `crates/core/src/manager/settle.rs`, `settle_path_checking`
- Category: broken feature
- Failure: `report.pdf` is in the folder, and `https://example.com/report.pdf` is added with no name typed, from the Add dialog, a paste or the browser, on a server that sends no `Content-Disposition`. With the default Ask, and equally with Replace or Keep existing, nothing is asked or replaced: the download silently saves as `report (1).pdf`. Adding the link again gives `report (2).pdf`. This is the pile of copies D88 exists to stop, and D96 says the setting decides whenever a file of that name is there. A download window that showed `report.pdf` also saves as `report (1).pdf` when its name is left alone.
- Evidence: `insert` picks the provisional path with `unique_path`, which skips names taken on disk (only `Kind::Media` uses `unclaimed_path`), so the row is `report (1).pdf`. `settled_name` then returns the provisional name, because the suggestion equals the link's name and so `server_named_it` is false. `settle_path_checking` runs the duplicate check only on `candidate(dir, name, 0)`, now `report (1).pdf`, where `finished_size` is `None`, so `found_existing` returns `None`. The check is reached only with a typed name or a server's own name. Every test in `crates/core/tests/manager/file_exists.rs` goes through `add_into`, which sets `file_name`, so none covers this path. Traced in code, not run. Contract 05 notes that a provisional name may carry ` (1)`; this finding is what that does to D88 and D96.
- Confidence: certain

### Xpath-normal-download-2 [P2] A browser download that meets a file already in the folder under Ask is torn down instead of asked about

- Where: `crates/core/src/manager/captures.rs`, `capture_arrived` and `await_answer`; `crates/core/src/manager/state.rs`, `Entry::server_answered`; `crates/core/src/manager/file_exists.rs`, `ask`
- Category: wrong implementation
- Failure: `ask_before_download` is off, `when_file_exists` is Ask, and the server names the file (`Content-Disposition`) with a name already in the folder. The run stops for the duplicate, `ask` opens the question and pauses the row, and `exited` wakes `await_answer` at once. `server_answered()` is false because `naming.settled` is false, so the answer is `It stopped before the server answered.` The capture removes the row in the background, which closes the question, and replies `NotStarted`. The browser resumes its own copy, so the person gets a second copy from the browser and the setting is not honoured.
- Evidence: `await_answer` treats any ended run with no settled path as a failed attempt. `FileExistsRequested` reaches `download_windows::ask` and `ConfirmationClosed` reaches `closed`, and contract 24 says `closed` leaves an open window showing the closed note, so a stray window is likely too. The Replace and Keep existing settings pass, because they settle the row first. No test in `crates/core/tests/manager` combines `add_capture` with a file already in the folder. Traced in code, not run. D27's rule of one copy holds, since the browser keeps it; what breaks is D88.3.
- Confidence: certain for the removal and the reply; likely for the stray window

### Xpath-normal-download-3 [P2] Refreshing the link cannot clear "a different file than the browser saw"

- Where: `crates/core/src/manager/downloads.rs`, `refresh_link` and `requeue`; `crates/core/src/manager/settle.rs`, `check_expectation`; `crates/messages/src/lib.rs`, `TargetRejected`
- Category: wrong implementation
- Failure: a capture is queued, or confirmed in its window, with the browser reporting 1024 bytes. By the time it runs, the file on the site has changed to 2048 bytes. It fails with "The server sent a different file than the browser saw... refresh the link" and a Refresh link button. The person gets a fresh link from the page, which also serves 2048 bytes, and it fails again with the same message, as does every retry. Only removing the row and adding it again, or restarting the app, gets out.
- Evidence: `entry.expect` is cleared only by a successful settle (`settle_path_checking`, `settle_at`) and is not saved. `refresh_link` changes the link and leaves `expect`, and `requeue` does too. By then the browser's copy is already gone, so the check can no longer protect a handover. `a_queued_capture_that_gets_another_file_offers_a_fresh_link` passes only because its new link serves the size the browser saw.
- Confidence: certain

### Xpath-normal-download-4 [P2] The Add dialog's link rule drops or cuts links every other layer accepts

- Where: `crates/desktop/src/links.rs`, `LINK` and `extract`; `crates/shell/src/launch.rs`, `LINK_SCHEMES` and `is_launch_link`; `crates/core/src/manager/downloads.rs`, `parse_url`
- Category: wrong implementation
- Failure: (a) `ftpes://server/file`, FTP over explicit TLS, is accepted by the engine, by core's `parse_url` and by the command line, but the regex takes only `https?` and `ftps?`. Typed or pasted, the dialog says the line has no link. From a second launch (D103) the dialog opens with it left out, so the app has no way to add such a link. (b) A command-line argument `https://example.com/a b.zip` passes `is_launch_link`, which allows spaces, and reaches the dialog as a line. `extract` stops at the space, so `https://example.com/a` is offered, with no note, since the line does hold a link, and a different address is downloaded if the person presses Download.
- Evidence: the patterns differ between `links.rs` and `launch.rs`, and D51.1 says links from the command line all go through the Add dialog, so the dialog's rule is the last word. `odd_or_hostile_arguments_are_not_taken_as_links` asserts that `ftpes://` is taken from the command line. No test joins the two.
- Confidence: certain

### Xpath-normal-download-5 [P2] A file name typed in the Add dialog is lost when the server answers with a web page

- Where: `crates/core/src/manager/added_pages.rs`, `AnsweredPage::of`, `read_answered_page` and `add_page_as_file`; `crates/desktop/src/add_dialog.rs`, `submit`
- Category: wrong implementation
- Failure: with the media add-on installed, a person adds a link with the name `invoice.html`, and the server answers `text/html`. The download gives way to reading the page (D51.3). No video is found, and the page "downloads as the page it is", with `Wanted { folder, ..default }`, under the server's or the link's name rather than the one typed. The folder chosen is carried across; the name is not. The dialog offers the field with no hint that it may be ignored, and a link on a known video site drops the name the same way.
- Evidence: `AnsweredPage` has fields `fetch`, `placed` and `folder`, and `of` never reads `entry.naming.name`. `Held` has no name either. D51 does not say the name is dropped. `add_video_page`'s doc says so for a page that does show a video, whose window names the file from its title; that reason does not hold for a page with no video.
- Confidence: certain

### Xpath-normal-download-6 [P2] A finished file with a leftover resume file is downloaded again and kept twice after a restart

- Where: `crates/core/src/naming.rs`, `finished_size`; `crates/core/src/manager/restore.rs`, `restore`; `crates/engine/src/download.rs`, `finish`; `crates/engine/src/target.rs`, `plan_exact`
- Category: wrong implementation
- Failure: the app is killed or loses power after the engine's rename and before `target::discard(&target.state)` takes effect, leaving `f.bin` complete beside `f.bin.wcstate`. On restart the row is not completed. It runs again with `ExistingFile::Refuse`, finds no part file, deletes the resume file and downloads the whole file again. The finish then refuses because of its own earlier file, so the data moves to `f (1).bin` and the row points there. The result is a full second download and a stray copy of the same file, or a failure if the link has expired by then, though the file was already whole.
- Evidence: contract 01 states that a crash in that gap leaves both files. `finished_size` returns `None` when either partial file exists, so `restore` does not mark it completed, even though a part file that is absent shows the rename happened. In `plan_exact`, `continues` is false since `part_len` is `None`, giving `Plan::Planned(target, None)`. `finish` then gives `TargetTaken` and core calls `take_next_free_name`. Not reproduced: the gap is short in code, and `FT-manager-kill` did not hit it in its kills. The rename is written through while the delete is not, so a power cut can widen it.
- Confidence: likely

### Xpath-normal-download-7 [P2] The Add dialog says "By file type" for downloads going to a queue's own folder

- Where: `crates/desktop/src/add_dialog.rs`, `folder_row`; `crates/settings/src/lib.rs`, `folder_for`
- Category: wrong implementation
- Failure: the queue is left on Automatic, and the default queue, or the queue a rule picks, has its own folder. The Save to row reads "By file type (from Settings)", while the file goes to the queue's folder. The hint under Queue names that queue, so the dialog knows where the link goes yet states the wrong folder.
- Evidence: `folder_row` reads a folder only from `picked_queue`, and Automatic is `None`, which falls to `FOLDER_BY_TYPE`. Core's `folder_for` returns `Setting::Own` for the queue `State::place` chose. `route` already keeps each link's queue name for the hint.
- Confidence: certain

## Suggestions

None.
