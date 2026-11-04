# Groups: desktop run

Stage 2, steps 1 and 2. Nothing here is validated: every cause below is what the findings say, read together, and a validator may split a group. "Same bug" names reports of one bug by different reviewers; each ID stays listed, so each still gets a verdict. No product code was read.

Totals: 351 findings (6 P0, 28 P1, 185 P2, 132 P3), 63 suggestions, 93 groups, 19 batches, 66 reports merged into another report of the same bug, which leaves 285 distinct findings.

## Causes

### G01 Path claims have no owner

`Claimed` is a set of paths with no owner: an open question holds no claim on the path it asks about, `replace_at` inserts without looking, and a finished row's move or removal gives up a claim it never held. Two downloads can so end on one part file.

- Findings: `05-1`, `Xconcurrency-1`, `05-13`, `Xconcurrency-9`, `Xconcurrency-10`
- Same bug: `05-1` = `Xconcurrency-1`; `05-13` = `Xconcurrency-9`

### G02 The provisional name hides the duplicate check

`insert` picks the provisional path with `unique_path`, which skips names on disk, and `settled_name` returns that name when the server names nothing new, so the check only ever sees `report (1).pdf`. D88 and D96 are skipped in the most common case.

- Findings: `05-2`, `Xpath-normal-download-1`
- Same bug: `05-2` = `Xpath-normal-download-1`

### G03 Settling forgets what was already known

The step at the server's answer does not carry what an earlier step knew: a folder picked early, a category worked out for the same name, the browser's word that a file is audio.

- Findings: `05-8`, `05-11`, `07-4`

### G04 `settled` is read as "the server answered"

`Entry::server_answered` is `naming.settled || Completed`, and `ask` leaves `settled` false, so a capture that meets Ask looks like a run that never got an answer. The row is removed, the question closes and the browser keeps its copy.

- Findings: `05-3`, `07-1`, `Xconcurrency-3`, `Xpath-normal-download-2`
- Same bug: `05-3` = `07-1` = `Xconcurrency-3` = `Xpath-normal-download-2`

### G05 Capture answers given outside one decision

The handover's check, its record and its answer are separate lock sections or use a loose mapping, so a join can answer `Started` before any server has answered, a withdrawal can land in the gap, and two captures of one link can both pass.

- Findings: `07-2`, `07-3`, `07-5`, `Xconcurrency-6`

### G06 Stop reasons overwrite one another

`Entry::stop` lets any person's choice replace the stop in place, and `run.found` survives it, so `finish` acts on both: a cancelled download restarts, and Pause all turns a cancel into a pause.

- Findings: `04-3`, `05-12`, `Xconcurrency-7`
- Same bug: `04-3` = `05-12`

### G07 An open question outlives the state it asked about

Only `requeue`, `cancel` and `finish_removing` close a file-exists question, and `answer_file_exists` checks neither the status nor a move under way, so a later answer acts on a changed download.

- Findings: `04-4`, `05-7`, `Xconcurrency-8`
- Same bug: `04-4` = `05-7` (its case a) = `Xconcurrency-8`

### G08 The path resolver blocks an async thread on the disk

`Destination::choose` calls the resolver directly and documents that it must not block, but `settle_path` and `found_existing` touch the disk there. An unreachable share stalls the runtime that also serves every command.

- Findings: `05-9`, `Xconcurrency-5`
- Same bug: `05-9` = `Xconcurrency-5`

### G09 Removing a finished row can delete a whole folder

`delete_files` asks `path.is_dir()` and calls `remove_dir_all`, a branch meant for work folders, on a finished row's saved path, which may by then be someone's folder.

- Findings: `Xsecurity-path-handling-1`, `04-8`
- Note: `04-8` is a misplaced doc comment on the same function; it has no shared cause and joins for its area.

### G10 A removal is not durable across a quit

`Hold::Removing` is memory only, and `finish_removing` takes `lock_open` after waiting, which fails once `shutdown` set `closed`, so the delete never reaches the store. The download returns, and a capture given back to the browser is downloaded twice.

- Findings: `04-5`, `Xconcurrency-2`, `Xshutdown-2`
- Same bug: `04-5` = `Xconcurrency-2` = `Xshutdown-2`
- Note: spans core and the capture answer. Proof: `proofs/Xconcurrency-2.patch`.

### G11 Whether a row finished is guessed from the disk

The row does not record that its own run placed a file, so `restore_entry` and `finished_size` read "settled, a file there, no partial files beside it" as finished, and read a complete file beside a leftover resume file as unfinished. A row can complete on a file it never downloaded, or download again and keep two copies.

- Findings: `04-1`, `Xcrash-and-resume-1`, `Xsecond-review-resume-1`, `Xcrash-and-resume-2`, `Xpath-normal-download-6`, `Xcrash-and-resume-3`, `Xsecond-review-resume-8`
- Same bug: `04-1` = `Xcrash-and-resume-1` = `Xsecond-review-resume-1`; `Xcrash-and-resume-2` = `Xpath-normal-download-6`; `Xcrash-and-resume-3` = `Xsecond-review-resume-8`
- Note: spans core restore, core naming, the engine's finish and media placing. `second-review-resume` found the engine side of `Xcrash-and-resume-2` sound; the fault is in how core reads that state. `FT-manager-kill` did not reach these states.

### G12 Decisions kept in memory only, or never cleared

`on_existing`, `expect` and `video_check` are not in the saved row, and a page's link can live only in a question, while the same rows' `settled` state does survive. So choices and checks silently change at a restart; `expect` is also never cleared by a refresh, so that fix cannot work.

- Findings: `05-6`, `Xcrash-and-resume-5`, `Xcrash-and-resume-6`, `Xpath-normal-download-3`

### G13 A failed flush at a checkpoint is forgotten

`mark_flushed` has already moved past the ranges, and the error only reaches `warn!`, so a later flush that passes makes a resume file or a finish trust bytes the disk may not hold.

- Findings: `Xsecond-review-disk-writes-1`

### G14 What a retry skips is not counted

The retry's `skip` is ignored in two ways: a body that ends while `skip` is above 0 counts as the file's real end, and skipped bytes are not paced.

- Findings: `Xsecond-review-disk-writes-2`, `02-3`
- Note: `Xsecond-review-disk-writes-2` applies to `worker.rs` and to `Fetcher::attempt`, so streams inherit it.

### G15 Starting over deletes kept data on weak evidence

`plan_exact` and `prepare` treat every read error, a failed delete and an unresumable finish alike, as a reason to discard the part file, though nothing proved it wrong.

- Findings: `Xsecond-review-resume-4`, `Xsecond-review-resume-5`, `Xsecond-review-disk-writes-5`

### G16 `.wcstate.tmp` can stay for good

The temporary resume file's name is a private detail of `save`, removed only there, so core's `partial_files` and the engine's finish never see it.

- Findings: `01-8`, `Xsecond-review-disk-writes-8`
- Same bug: `01-8` = `Xsecond-review-disk-writes-8`

### G17 Nothing above the engine knows whether a download can continue

`Outcome::Paused` and `describe` carry no "can resume", and an unknown size travels as `UNKNOWN_END`. So the CLI prints 16 EiB and promises a continuation, and the app promises "the part already downloaded is kept".

- Findings: `01-6`, `32-1`, `28-1`
- Same bug: `01-6` = `32-1` (its size half)

### G18 Folder planning looks only at the preferred name

`plan_in_folder` builds one path and trusts or abandons what it finds there, so a numbered first run, a damaged resume file or a second running copy all strand or take over files. It reaches the CLI and the engine's public API, not the app.

- Findings: `01-1`, `01-2`, `F-1`
- Note: proof for `F-1`: `proofs/FT-kill.patch`.

### G19 Engine bookkeeping and cost

No shared cause: small faults in space reservation, checkpoint frequency, the progress count, the step-back rule, a doc's promise and how a short answer is treated.

- Findings: `01-3`, `01-4`, `01-5`, `02-5`, `02-6`, `F-2`
- Note: proof for `F-2`: `proofs/FT-sizes.patch`.

### G20 CLI ending and reporting

The CLI's `main` and its signal task do not cover an early cancel, a closed stderr, a resumed position or an option its own messages name.

- Findings: `32-2`, `32-3`, `32-4`, `32-5`

### G21 The connector checks only the pipe's owner

`connect` compares the owner SID and no integrity level, and the key file has no no-read-up label, so a lower-integrity program of the same user can stand in for the app, and the real app then hands over to it.

- Findings: `08-1`
- Note: the probe ran from an administrator's session; `08-1` itself asks for a repeat from an unelevated token.

### G22 A squatted or damaged key does not lead to the repair

D41 expects a launch to renew the key, but `bind_keyed` renews on two errors only, `attempt` treats only three errors as "start the app", and several real squatter and damage errors reach neither path.

- Findings: `06-1`, `08-2`, `08-3`, `08-4`

### G23 A second launch at start

`hand_over_on` always says hello first and `main` shows its box on any error, so a slow list opening costs links or pops a modal box. `22-4` joins for its area: `args()` panics in the same `main`.

- Findings: `22-3`, `22-4`

### G24 Bridge values written in two crates, and a bridge test

The test overrides and `--background` are literals in two crates with no check tying them, and one test named for agreement between two readers calls only one of them.

- Findings: `06-2`, `06-4`, `22-9`

### G25 `backend::config` never names the official sources

`ManagerConfig::new` leaves `app_releases` and `proxy_test_url` empty so tests stay offline, and `config` calls only `with_official_suffix_list`. So no installed copy finds an update and every proxy test fails, each looking like a normal answer.

- Findings: `Xfeature-behaviour-1`, `Xpath-app-update-1`, `Xsecurity-update-signatures-1`, `Xfeature-behaviour-2`
- Same bug: `Xfeature-behaviour-1` = `Xpath-app-update-1` = `Xsecurity-update-signatures-1`
- Note: spans desktop, core, app updates and proxies. The update half is hidden today by the placeholder key.

### G26 An inherited `APPIMAGE` is trusted

`install_kind`, `replace_appimage`, `autostart::program` and the bridge registration trust the variable alone, without comparing it with `current_exe` or `APPDIR`, so an update can overwrite another program.

- Findings: `12-1`, `Xpath-app-update-4`, `Xsecurity-update-signatures-2`
- Same bug: `12-1` = `Xpath-app-update-4` = `Xsecurity-update-signatures-2`

### G27 The app starts the installer, then quits without knowing it worked

`install` treats a started process as success and quits at once, before the setup has elevated or the new AppImage has met the old copy's endpoint, so a failure has nobody left to report it.

- Findings: `Xpath-app-update-2`, `Xsecurity-update-signatures-5`, `12-5`, `12-3`, `Xpath-app-update-3`
- Same bug: `Xpath-app-update-2` = `Xsecurity-update-signatures-5`; `12-3` = `Xpath-app-update-3`
- Note: reviewers disagree. `12-5` assumes the shell shows the prompt and returns error 5; `Xpath-app-update-2` read `asInvoker` from the built setup and says the setup elevates itself later, so `12-5` may not occur. `security-update-signatures` also lists a new AppImage starting early as handled by `start_again_later`, against `12-3`.

### G28 What an update leaves on disk

`fetch_app_update` writes into `updates` and nothing removes it, and `replace_appimage` renames its copy with no flush.

- Findings: `12-2`, `Xpath-app-update-5`, `12-4`
- Same bug: `12-2` = `Xpath-app-update-5`

### G29 An offer's state and checks

`download: None` has four reasons that the app reads as one, the unsigned `html_url` reaches the shell unchecked, a setting change does not revisit a found offer, and two manifest rules have no test.

- Findings: `Xpath-app-update-6`, `Xsecurity-update-signatures-3`, `Xsecurity-update-signatures-4`, `Xpath-app-update-7`, `12-6`
- Same bug: `Xpath-app-update-6` = `Xsecurity-update-signatures-3`

### G30 Installers and release workflow

No shared cause: packaging scripts, the uninstaller and CI. `29-2` is read differently by `security-addon-unpacking`, which found the leaked library path harmless for the add-on's programs; `29-2` is about what `xdg-open` starts.

- Findings: `29-1`, `29-2`, `29-3`, `29-4`, `29-5`, `29-6`

### G31 A media work folder's contents carry no identity

`downloaded.json` and `.done` markers name no link, `refresh_link` keeps the folder, and a stream's fingerprint ignores hosts and signing values, while `unclaimed_path` allows the name. So another download's video is saved as this one, and Refresh link reuses the bad bytes it was meant to replace.

- Findings: `17-1`, `Xsecond-review-resume-2`, `Xpath-media-download-6`, `15-3`
- Same bug: `17-1` = `Xsecond-review-resume-2`; `15-3` = `Xpath-media-download-6` (its stream half)
- Note: spans core media runs, streams and `refresh_link`. `placing.done` already has `link_digest`, the pattern the others lack.

### G32 `final_path` and the row's path disagree

`final_path` replaces whatever follows the last dot and can differ from the planned path, while the work folder and partial paths are derived from the row's path, so words are lost, a Replace strands the finished file, and a typed extension decides the container.

- Findings: `17-2`, `Xpath-media-download-5`, `17-3`, `Xrepeated-logic-2`, `Xpath-media-download-7`
- Same bug: `17-2` = `Xpath-media-download-5`; `17-3` = `Xrepeated-logic-2`

### G33 A page's HLS part runs outside the run's file work

The HLS branch of `fetch_part` never sets `file_work`, so `RunGuard` reports the run ended while blocking work still writes.

- Findings: `17-6`, `Xpath-media-download-8`, `Xsecond-review-disk-writes-9`, `Xsecond-review-resume-6`
- Same bug: `17-6` = `Xpath-media-download-8` = `Xsecond-review-disk-writes-9` = `Xsecond-review-resume-6`

### G34 A finished media file has no durable last step

The output of ffmpeg or yt-dlp is placed and its sources deleted with no flush, and a stream's log is gone before placing, so a power cut or a failed place costs the data or a whole new download.

- Findings: `Xsecond-review-disk-writes-3`, `Xsecond-review-resume-7`, `Xpath-media-download-11`
- Same bug: `Xsecond-review-disk-writes-3` = `Xsecond-review-resume-7`

### G35 A plain segment's body is never checked

`fetch_piece` saves any 200 answer as a finished segment and `ffmpeg::run` takes exit 0 as success, so a video cut short or an HTML page is reported complete.

- Findings: `15-1`

### G36 The stream log and its last step

The stream's last cleanup turns a saved file into a failure, and `Log::read` counts a torn last line, whose `A` form then truncates the track. `15-4`, `15-5` and `15-6` join for their area.

- Findings: `15-2`, `Xsecond-review-disk-writes-6`, `Xsecond-review-disk-writes-4`, `Xsecond-review-resume-3`, `15-4`, `15-5`, `15-6`
- Same bug: `15-2` = `Xsecond-review-disk-writes-6`; `Xsecond-review-disk-writes-4` = `Xsecond-review-resume-3`

### G37 Media run docs and one test

No shared cause: a test half that cannot fail and doc comments on the wrong items.

- Findings: `17-8`, `17-10`

### G38 What is offered and what ffmpeg accepts are not tied

The offer layer lists sources `InputFormat` refuses, and `add_maps` demands audio that yt-dlp left undescribed, so the choice fails only after the whole download.

- Findings: `Xpath-media-download-1`, `Xpath-media-download-10`

### G39 yt-dlp's answer is read only in part

`Info` and `Format` leave out a format's `cookies`, the top-level `protocol` and `http_headers`, and `_type` with `entries`, so such pages fail or save the wrong thing.

- Findings: `Xpath-media-download-2`, `Xpath-media-download-3`, `Xpath-media-download-4`

### G40 A title is not made safe for what it becomes

The title reaches `sanitize`, which treats it as a server's path and cuts at the last slash, and reaches yt-dlp's `-o` template unescaped, so `%` fields and environment names move the file or stop the download.

- Findings: `16-1`, `Xsecurity-path-handling-3`, `16-2`
- Same bug: `16-1` = `Xsecurity-path-handling-3`

### G41 Quality labels, sizes and progress

No shared cause: reported per format, labelled from the wrong format, or added without a check.

- Findings: `16-3`, `16-4`, `16-5`

### G42 yt-dlp fallback and page refresh rules

The file transport and the other transports obey different rules: a refused HLS address does not refresh, a vanished format id goes to a selection that names it, and a speed limit does not reach yt-dlp.

- Findings: `17-4`, `17-5`, `Xpath-media-download-9`

### G43 A typed name is dropped when the download becomes a page

`AnsweredPage` and `Held` have no name field, so a name typed in the Add dialog is lost for a page without a video and for a video site, and nothing tells the person.

- Findings: `17-7`, `Xpath-normal-download-5`, `Xfeature-behaviour-6`
- Same bug: `17-7` = `Xpath-normal-download-5`

### G44 Stream limits count text, not the work it asks for

`MAX_MANIFEST_ENTRIES`, the manifest size and `AddressBudget` bound what is read, not what it expands to: an address hundreds of gigabytes long, quadratic selection on a runtime thread, keys outside the budget.

- Findings: `14-1`, `14-2`, `14-3`

### G45 Selection, container and the key error

No shared cause beyond the area: a default rendition with no URI, a subtitle codec read as unknown, and a key error printing the signed address.

- Findings: `14-4`, `14-5`, `14-6`

### G46 Running programs

The tail, the grace branch and the kill path. No single cause.

- Findings: `13-1`, `13-2`, `13-3`, `13-5`, `Xshutdown-4`
- Note: `docs/ROADMAP.md` already lists yt-dlp and ffmpeg outliving a crashed app on Linux, which the plan excludes; only the `SIGTERM` half of `Xshutdown-4` is new.

### G47 Stored data that cannot be read

A cell that fails conversion is not `is_damage` and stops every start, while an unreadable settings, queue or password value reads as a first start and is overwritten. Tests do not cover the stored forms.

- Findings: `03-1`, `03-2`, `04-6`, `03-6`, `Xdatabase-migrations-5`
- Same bug: `03-2` = `04-6` (its queue half)

### G48 The store has only one signal: failure, at the start of a spell

There is no "saved again", and a write that can never pass starts a new spell each batch. So the banner is wrong both ways, Try again does nothing, and notifications repeat; the docs promise otherwise.

- Findings: `03-3`, `03-4`, `23-10`, `25-2`, `Xfeature-behaviour-3`, `03-8`
- Same bug: `03-4` = `23-10` = `25-2` = `Xfeature-behaviour-3`
- Note: spans store, core settings and the banner. `03-4` and `25-2` give different reasons why Try again saves nothing, and both can hold.

### G49 Kept copies of the list

Age is read from a modified time that a rename keeps, so a set-aside copy can go at the next start, and a roaming copy that could not be deleted stays for good. Both copies hold every cookie.

- Findings: `03-5`, `Xdatabase-migrations-1`, `Xdatabase-migrations-4`
- Same bug: `03-5` = `Xdatabase-migrations-1`

### G50 What the window says while the list is not open

`Core` caches the open error for good, and `empty` tests `Failed` only, so Try again cannot work and loading reads as a first run.

- Findings: `Xdatabase-migrations-2`, `25-9`

### G51 Moves are neither crash-safe nor waited for

`move_file` copies straight to the final name, the row is saved only at the end, and `shutdown` ignores `Hold::Moving`, so a quit or kill leaves a cut-off file under the real name or a list naming the old path. The move also drops the Mark of the Web, and `place` keeps its own weaker no-replace move.

- Findings: `Xshutdown-1`, `Xconcurrency-4`, `Xcrash-and-resume-4`, `Xsecond-review-disk-writes-7`, `05-4`, `Xrepeated-logic-4`
- Same bug: `Xshutdown-1` = `Xconcurrency-4`; `Xcrash-and-resume-4` = `Xsecond-review-disk-writes-7` (its copy half)
- Note: spans relocate, shutdown, restore, the engine's rename and zone marking.

### G52 The quit limit does not end the process

`quit_core` is called with `|| ()`, and dropping the runtime waits for blocking work without limit, so a stuck file task keeps a process with no window, holding the database lock.

- Findings: `22-1`, `Xshutdown-3`, `22-7`
- Same bug: `22-1` = `Xshutdown-3`
- Note: both are "likely", read from GPUI and tokio sources; `Xshutdown-3` names the small GPUI program that would settle it.

### G53 Start and registration

No shared cause: a registration's failure reason, an `Exec` line, a trait with one implementation, and an empty window title.

- Findings: `22-5`, `22-6`, `22-8`, `U-2`

### G54 Redirect targets are kept for the whole run

`RequestContext.url` is set once from `final_url`, so an expired signed target fails a download whose own link still works.

- Findings: `02-1`

### G55 Names from headers and links

`filename*`'s charset is ignored, a raw byte loses the whole header at `to_str`, and FTP's `;type=` reaches the name.

- Findings: `02-2`, `01-7`, `21-2`

### G56 HTTP small faults

No shared cause: a 416 reported as a server error, and a sleep with no effect.

- Findings: `02-4`, `02-7`

### G57 Failure messages assume a download row or the wrong cause

`describe` writes advice for a row with a Get a new link button and reads only an error's kind, so add-on and update callers, and rename and save failures, get fixes that cannot apply.

- Findings: `28-2`, `28-3`, `28-4`, `28-5`, `10-3`

### G58 A settings file can send downloads beside programs Wirecatch runs

`protected_folders` on Windows does not know the data folder and `is_outside_user_folders` is false inside `{Home}`, while `tools` looks only at recorded files, so a site's DLL can land beside `ffmpeg.exe`.

- Findings: `Xsecurity-addon-unpacking-1`, `Xsecurity-path-handling-2`
- Same bug: `Xsecurity-addon-unpacking-1` = `Xsecurity-path-handling-2`
- Note: spans settings and add-ons. The reviewers rate it P1 and P2.

### G59 The import preview shows less than the import does

`ImportPreview` has no proxies or local sites, queue names are not checked against each other, and `SettingsImported` has no reader, so a person confirms changes they were not shown.

- Findings: `19-1`, `27-2`, `19-2`, `27-11`, `30-4`, `Xfeature-behaviour-7`
- Same bug: `19-1` = `27-2`; `27-11` = `30-4` = `Xfeature-behaviour-7` (its import half)

### G60 Writers that go around `whole_file::replace`

The settings export writes in place and the bridge's manifest write has no flush.

- Findings: `19-6`, `Xrepeated-logic-6`
- Same bug: `19-6` = `Xrepeated-logic-6` (its export half)

### G61 The add-on's plan and record act on stale or partial knowledge

`addon_plan` takes every part when `tools` is `None`, the check saves a record it cloned before its network wait, and `?` in the candidate loop stops at the first bad release, so a repair goes backwards and a removal comes back.

- Findings: `11-1`, `11-2`, `11-3`, `Xsecurity-addon-unpacking-3`
- Note: `security-addon-unpacking` adds that `whole_file::replace` has one `.tmp` name, so two writers can mix bytes under `11-2`.

### G62 Add-on release files left in `.work`

Only the installed version's file is deleted, and files are named by version, so skipped ones stay.

- Findings: `11-4`, `Xsecurity-addon-unpacking-4`, `11-5`
- Same bug: `11-4` = `Xsecurity-addon-unpacking-4`

### G63 `AddonState` mixes "an operation" with "the tools work"

`addon_info` reports the operation's state first and carries no "usable", so the quality form, About and the failure path each judge wrongly, and `mark_broken` runs only when a start fails.

- Findings: `24-1`, `24-2`, `24-3`, `27-4`, `Xsecurity-addon-unpacking-2`

### G64 The window's list order is not told by any event

`DownloadSummary` has no position and `apply` keeps a row at its old index, so a moved download is drawn in the wrong place, and a later reorder sends that wrong line.

- Findings: `23-1`, `25-5`
- Same bug: `23-1` = `25-5`

### G65 Library changes tell everyone, and a reload tells no one

`Library::apply` calls `cx.notify()` for each `DownloadsChanged`, so observers do list-sized work and reset state up to ten times a second, while `load` after `Lagged` refills startup notices and does not refresh open windows.

- Findings: `25-6`, `23-9`, `Xdatabase-migrations-3`, `24-9`, `26-7`, `27-8`
- Same bug: `23-9` = `Xdatabase-migrations-3`
- Note: spans the main window, the tray, download windows and forms. `26-S1` is the matching suggestion.

### G66 Space in the search field

The `space` binding sits on the window root and the input has no space of its own, so a typed space pauses or resumes selected rows.

- Findings: `25-1`
- Note: needs a keystroke proof; the UI review drove no such key.

### G67 The selection is not what the window shows

Commands and drags read raw `self.selected`, which is never pruned, instead of `selected_rows`, and the selection bar has no reserved space.

- Findings: `25-3`, `25-4`, `U-4`

### G68 Main window behaviour

No shared cause: stacked dialogs, the drawer, offered actions that differ and small display faults.

- Findings: `25-7`, `25-8`, `25-10`, `25-11`, `25-12`, `U-3`, `U-8`, `U-10`, `U-15`

### G69 The local network rule stops at HTTP

`download_with` makes a guard for `http` and `https` only, `ftps` hosts stay percent-encoded in the checks, and yt-dlp reads and fetches outside `network_rule`. The doc of `refuses` promises the engine refuses what it does not.

- Findings: `09-1`, `21-1`, `Xsecurity-bridge-and-pipe-input-1`, `Xsecurity-bridge-and-pipe-input-2`
- Same bug: `09-1` = `21-1` = `Xsecurity-bridge-and-pipe-input-1`
- Note: spans engine FTP, sites, core captures and yt-dlp. D34.9 is the contract.

### G70 What counts as a local page

`of_capture` does not check a page address's scheme, and a suffix held only through a wildcard reads as not public.

- Findings: `10-1`, `10-2`

### G71 The guard's edges

No shared cause: cancellation during lookups, tests that never reach the filter, an unmapped refusal, a repeated rule and stale docs.

- Findings: `09-4`, `09-5`, `09-6`, `09-8`, `09-9`

### G72 "This host is that site" is written four times

Copies differ on trailing dots, case and addresses, so queue rules and video detection answer wrongly on forms the others handle.

- Findings: `10-4`, `Xrepeated-logic-3`
- Same bug: `10-4` = `Xrepeated-logic-3` (its first copies)

### G73 Proxy exceptions: what is accepted is not what matches

`clean_hosts` accepts anything the reader then ignores, a bracketed IPv6 never matches, `ytdlp_proxy` ignores exceptions altogether, and the size limit comes after a quadratic scan.

- Findings: `09-3`, `19-3`, `19-4`, `19-5`

### G74 Proxy forms, tests and leftovers

No single cause: a SOCKS4 address family, a placeholder that stops being true, a port made up, a cancelled test saved, unreachable test paths and an unused function.

- Findings: `09-2`, `09-7`, `19-7`, `19-8`, `19-9`, `27-1`, `27-9`, `Xrepeated-logic-7`

### G75 Secrets in links

The real link and the shown link are told apart at each use, not once: one question sends the password to the UI, `left_by` compares a real link with a saved cleaned one, and redaction stops early at an apostrophe or takes too much after a colon.

- Findings: `05-5`, `05-10`, `20-1`, `20-2`, `Xrepeated-logic-8`

### G76 A queue's run is not one thing

`has_work` looks only at running downloads and the sidebar reads `allows` while the core reads `Gate`, and `running_until_empty` is not saved, so a run ends early, the window disagrees with what runs, and a restart changes it.

- Findings: `18-1`, `18-2`, `18-3`

### G77 What frees or takes a slot

`fail_to_start` calls no `reconcile`, and `plan` lets stopping downloads' slots go in the same pass.

- Findings: `04-2`, `18-4`

### G78 Rule reading

No shared cause: repeats compared by lower case, help text against `best_site`, and dotted file types honoured in one list only.

- Findings: `18-5`, `27-10`, `Xrepeated-logic-5`

### G79 Queue forms and queue display

No shared cause: Enter in a dialog, a value rounded on save, numbering, and sidebar and header text.

- Findings: `27-3`, `27-5`, `U-5`, `U-11`, `U-12`, `U-13`

### G80 The file-exists question in download windows

The window does not read `same_file` or `size`, `became` sets `shows` without looking, and the first window turns into a progress view for a download that is only waiting.

- Findings: `24-4`, `30-1`, `24-6`, `U-7`, `U-6`
- Same bug: `24-4` = `30-1`; `24-6` = `U-7`

### G81 Download windows ignore a failure's fix

Neither file reads `error.fix` or `RETRY_FIXES`, and `Reveal::details` has no caller, so Try again is offered for failures it cannot help.

- Findings: `30-2`, `Xfeature-behaviour-5`
- Same bug: `30-2` = `Xfeature-behaviour-5`
- Note: a suggestion in the Download windows area asks for the same thing.

### G82 Download window actions

No shared cause: the proposal sent as a typed name, Cancel with no question, no way back after Hide, and a blank complete window.

- Findings: `24-5`, `24-7`, `U-14`, `24-8`, `24-10`
- Same bug: `24-7` = `U-14`

### G83 What is told and when

`Teller` only adds and `kept` is one queue for every kind of notice, so later events do not correct a pending notice, and open ones lose their buttons. `26-8` is why this went unseen: the file has no tests.

- Findings: `26-1`, `26-5`, `26-6`, `26-8`

### G84 Tray menu

No single cause: muda's own check flip, unescaped `&`, errors only logged, a literal product name.

- Findings: `26-2`, `26-3`, `26-4`, `26-9`

### G85 Public Suffix List controls were never drawn

`api::suffix_list` and `check_suffix_list` have no caller, and nothing reads `SuffixListChecked`, so D44.4's date, last check and Check now do not exist.

- Findings: `23-5`, `30-3`, `Xfeature-behaviour-4`
- Same bug: `23-5` = `30-3` = `Xfeature-behaviour-4`

### G86 The link rule is written twice with two contents

`links.rs` accepts fewer schemes than `launch.rs`, the engine and core, and its regex cuts at characters a link can hold, so a link is dropped or a different one is downloaded.

- Findings: `22-2`, `Xpath-normal-download-4`, `Xrepeated-logic-1`, `23-2`
- Same bug: `22-2` = `Xpath-normal-download-4` (its case a) = `Xrepeated-logic-1`

### G87 Status and failure text

Fixed strings and first branches say more than the row's state supports: 100% while merging, "Merging" for every step, advice with no such button, raw yt-dlp output.

- Findings: `23-3`, `23-4`, `23-8`, `U-9`

### G88 Settings pages and dialog display

No shared cause: an unlisted value, a button with no answer, a discarded chooser error, accent contrast, one stale mark, one wrong Save to row.

- Findings: `27-6`, `27-7`, `U-1`, `23-6`, `23-7`, `Xpath-normal-download-7`

### G89 Tests race the machine, and `test-timing` cannot see it

Waits and limits sit in named constants, helpers, `recv_timeout` or real-clock runs, which the check's patterns miss. ENGINEERING section 7 forbids each.

- Findings: `01-9`, `01-10`, `33-2`, `03-7`, `04-7`, `05-14`, `06-3`, `10-5`, `13-4`, `21-3`
- Same bug: `01-10` = `33-2` (its live case)

### G90 Repo scripts and the comments around them

No shared cause: a script that cannot pass since the gate needs `.git`, tests pinned to live values, a repeated parser, dead code and stale comments.

- Findings: `33-1`, `33-3`, `33-4`, `33-5`, `33-6`

### G91 Test helpers not shared or not doing what they say

Each test file builds its own values, and test servers behave unlike their docs.

- Findings: `31-1`, `31-2`, `31-3`, `31-4`, `02-8`, `07-7`, `17-9`

### G92 Leftovers from the removed web interface, and other dead code

Items kept for the Tauri and Svelte app have no reader now, and their docs describe it. `30-7` and `07-6` join for their area.

- Findings: `23-11`, `24-11`, `30-5`, `30-6`, `30-7`, `07-6`, `19-10`

### G93 Small copies

Two repeats with no shared cause: a side-file helper, and several small pieces across the forms.

- Findings: `03-9`, `27-12`

## Batches

Each batch's status is `pending`. "part" names what to read of a file that is mostly unrelated to the batch; the validator widens it when a finding needs more. Line totals are counted from the files, using the estimate given for each part.

### B01 Path claims, file-exists questions and captures

- Status: pending
- Groups: G01, G02, G03, G04, G05, G06, G07, G08
- Highest severity: P0
- Lines: about 6,000
- Proofs to use: `proofs/Xconcurrency-1.patch`, `proofs/Xconcurrency-3-and-6.patch`
- Findings: `05-1`, `Xconcurrency-1`, `05-13`, `Xconcurrency-9`, `Xconcurrency-10`, `05-2`, `Xpath-normal-download-1`, `05-8`, `05-11`, `07-4`, `05-3`, `07-1`, `Xconcurrency-3`, `Xpath-normal-download-2`, `07-2`, `07-3`, `07-5`, `Xconcurrency-6`, `04-3`, `05-12`, `Xconcurrency-7`, `04-4`, `05-7`, `Xconcurrency-8`, `05-9`, `Xconcurrency-5`
- Files: `crates/core/src/manager/file_exists.rs`, `crates/core/src/manager/settle.rs`, `crates/core/src/manager/state.rs`, `crates/core/src/manager/downloads.rs`, `crates/core/src/manager/relocate.rs`, `crates/core/src/manager/run.rs`, `crates/core/src/manager/captures.rs`, `crates/core/src/manager/handovers.rs`, `crates/core/src/manager/mod.rs` (part, ~300: `stop_run`, `shutdown`), `crates/core/src/naming.rs`, `crates/core/tests/manager/file_exists.rs`, `crates/core/tests/manager/settle.rs`, `crates/engine/src/target.rs`, `crates/server/src/lib.rs` (part, ~100: `start_request`)

### B02 Restart, removal and what is saved

- Status: pending
- Groups: G09, G10, G11, G12
- Highest severity: P0
- Lines: about 6,900
- Proofs to use: `proofs/Xconcurrency-2.patch`
- Findings: `Xsecurity-path-handling-1`, `04-8`, `04-5`, `Xconcurrency-2`, `Xshutdown-2`, `04-1`, `Xcrash-and-resume-1`, `Xsecond-review-resume-1`, `Xcrash-and-resume-2`, `Xpath-normal-download-6`, `Xcrash-and-resume-3`, `Xsecond-review-resume-8`, `05-6`, `Xcrash-and-resume-5`, `Xcrash-and-resume-6`, `Xpath-normal-download-3`
- Files: `crates/core/src/manager/restore.rs`, `crates/core/src/manager/startup.rs`, `crates/core/src/naming.rs`, `crates/core/src/manager/run.rs`, `crates/core/src/manager/downloads.rs`, `crates/core/src/manager/state.rs`, `crates/core/src/manager/mod.rs`, `crates/core/src/manager/file_exists.rs`, `crates/core/src/manager/settle.rs`, `crates/core/src/manager/added_pages.rs`, `crates/core/src/manager/captures.rs` (part, ~120: `await_answer`, `capture_arrived`), `crates/core/src/manager/media_run/mod.rs`, `crates/core/src/manager/media_run/placement.rs`, `crates/core/src/manager/media_run/workdir.rs`, `crates/core/src/manager/media.rs` (part, ~100: `hold_media`), `crates/engine/src/target.rs`, `crates/engine/src/download.rs`, `crates/files/src/discard.rs`, `crates/core/tests/manager/restart.rs`

### B03 Engine disk writes and resume

- Status: pending
- Groups: G13, G14, G15, G16, G17, G18, G19, G20
- Highest severity: P0
- Lines: about 7,500
- Proofs to use: `proofs/FT-kill.patch`, `proofs/FT-sizes.patch`
- Findings: `Xsecond-review-disk-writes-1`, `Xsecond-review-disk-writes-2`, `02-3`, `Xsecond-review-resume-4`, `Xsecond-review-resume-5`, `Xsecond-review-disk-writes-5`, `01-8`, `Xsecond-review-disk-writes-8`, `01-6`, `32-1`, `28-1`, `01-1`, `01-2`, `F-1`, `01-3`, `01-4`, `01-5`, `02-5`, `02-6`, `F-2`, `32-2`, `32-3`, `32-4`, `32-5`
- Files: `crates/engine/src/checkpoint.rs`, `crates/engine/src/storage.rs`, `crates/engine/src/writer.rs`, `crates/engine/src/worker.rs`, `crates/engine/src/fetch.rs`, `crates/engine/src/target.rs`, `crates/engine/src/state.rs`, `crates/engine/src/download.rs`, `crates/engine/src/segment.rs`, `crates/engine/src/http/range.rs`, `crates/engine/src/receive.rs`, `crates/engine/src/retry.rs`, `crates/engine/src/error.rs`, `crates/engine/src/progress.rs`, `crates/engine/src/file_work.rs`, `crates/engine/src/abandon.rs`, `crates/streams/src/workdir.rs`, `crates/core/src/naming.rs`, `crates/cli/src/main.rs`, `crates/messages/src/lib.rs`

### B04 Local endpoint and bridge

- Status: pending
- Groups: G21, G22, G23, G24
- Highest severity: P0
- Lines: about 7,100
- Findings: `08-1`, `06-1`, `08-2`, `08-3`, `08-4`, `22-3`, `22-4`, `06-2`, `06-4`, `22-9`
- Files: `crates/ipc/src/windows.rs`, `crates/ipc/src/key.rs`, `crates/ipc/src/lib.rs`, `crates/ipc/src/paths.rs`, `crates/ipc/src/unix.rs`, `crates/ipc/tests/keyed.rs`, `crates/bridge/src/app.rs`, `crates/bridge/src/main.rs`, `crates/bridge/src/raw_frame.rs`, `crates/bridge/src/shell_parent.rs`, `crates/bridge/tests/bridge.rs`, `crates/test-support/src/bridge.rs`, `crates/shell/src/launch.rs`, `crates/desktop/src/main.rs`, `crates/desktop/src/backend.rs`

### B05 App updates and packaging

- Status: pending
- Groups: G25, G26, G27, G28, G29, G30
- Highest severity: P1
- Lines: about 5,900
- Findings: `Xfeature-behaviour-1`, `Xpath-app-update-1`, `Xsecurity-update-signatures-1`, `Xfeature-behaviour-2`, `12-1`, `Xpath-app-update-4`, `Xsecurity-update-signatures-2`, `Xpath-app-update-2`, `Xsecurity-update-signatures-5`, `12-5`, `12-3`, `Xpath-app-update-3`, `12-2`, `Xpath-app-update-5`, `12-4`, `Xpath-app-update-6`, `Xsecurity-update-signatures-3`, `Xsecurity-update-signatures-4`, `Xpath-app-update-7`, `12-6`, `29-1`, `29-2`, `29-3`, `29-4`, `29-5`, `29-6`
- Files: `crates/desktop/src/backend.rs`, `crates/core/src/manager/mod.rs`, `crates/core/src/manager/app_update.rs`, `crates/core/src/manager/proxy.rs`, `crates/desktop/src/app_update.rs`, `crates/desktop/src/lifecycle.rs`, `crates/desktop/src/main.rs`, `crates/desktop/src/main_window/notices_bar.rs`, `crates/desktop/src/main_window/settings/general.rs`, `crates/shell/src/updates.rs`, `crates/shell/src/appimage.rs`, `crates/shell/src/autostart.rs`, `crates/shell/src/bridge.rs`, `crates/shell/src/launch.rs`, `crates/updates/src/manifest.rs`, `crates/updates/src/release.rs`, `crates/desktop/packaging/windows/installer.iss`, `crates/desktop/PACKAGING.md`, `crates/desktop/assets/icons/icon.svg`, `.github/workflows/release.yml`, `scripts/package.mjs`, `scripts/release/manifest.mjs`, `scripts/release/manifest.test.mjs`, `scripts/ci/linux-package-check.sh`, `scripts/ci/linux-packages.sh`

### B06 Media work folders, placing and stream joining

- Status: pending
- Groups: G31, G32, G33, G34, G35, G36, G37
- Highest severity: P1
- Lines: about 6,200
- Findings: `17-1`, `Xsecond-review-resume-2`, `Xpath-media-download-6`, `15-3`, `17-2`, `Xpath-media-download-5`, `17-3`, `Xrepeated-logic-2`, `Xpath-media-download-7`, `17-6`, `Xpath-media-download-8`, `Xsecond-review-disk-writes-9`, `Xsecond-review-resume-6`, `Xsecond-review-disk-writes-3`, `Xsecond-review-resume-7`, `Xpath-media-download-11`, `15-1`, `15-2`, `Xsecond-review-disk-writes-6`, `Xsecond-review-disk-writes-4`, `Xsecond-review-resume-3`, `15-4`, `15-5`, `15-6`, `17-8`, `17-10`
- Files: `crates/core/src/manager/media_run/workdir.rs`, `crates/core/src/manager/media_run/page.rs`, `crates/core/src/manager/media_run/placement.rs`, `crates/core/src/manager/media_run/mod.rs`, `crates/core/src/manager/media_run/save.rs`, `crates/core/src/manager/media_run/tests.rs`, `crates/core/src/manager/file_exists.rs`, `crates/core/src/manager/downloads.rs` (part, ~150: `insert`, `refresh_link`), `crates/core/src/manager/state.rs` (part, ~100: `partial_paths`), `crates/core/src/manager/run.rs` (part, ~150: `RunGuard`), `crates/core/src/manager/media.rs` (part, ~200: `insert_chosen`), `crates/streams/src/plan.rs`, `crates/streams/src/workdir.rs`, `crates/streams/src/download/mod.rs`, `crates/streams/src/download/join.rs`, `crates/streams/src/download/fetch.rs`, `crates/streams/src/model.rs`, `crates/streams/src/crypto.rs`, `crates/streams/src/ffmpeg.rs`, `crates/streams/Cargo.toml`, `crates/media/src/describe.rs`, `crates/media/src/merge.rs`, `crates/engine/src/fetch.rs` (part, ~100: `save`)

### B07 yt-dlp, what media offers, and names

- Status: pending
- Groups: G38, G39, G40, G41, G42, G43
- Highest severity: P1
- Lines: about 7,600
- Findings: `Xpath-media-download-1`, `Xpath-media-download-10`, `Xpath-media-download-2`, `Xpath-media-download-3`, `Xpath-media-download-4`, `16-1`, `Xsecurity-path-handling-3`, `16-2`, `16-3`, `16-4`, `16-5`, `17-4`, `17-5`, `Xpath-media-download-9`, `17-7`, `Xpath-normal-download-5`, `Xfeature-behaviour-6`
- Files: `crates/media/src/ytdlp/info.rs`, `crates/media/src/ytdlp/mod.rs`, `crates/media/src/ytdlp/download.rs`, `crates/media/src/ytdlp/cookies.rs`, `crates/media/src/choice.rs`, `crates/media/src/output/variants.rs`, `crates/media/src/output/arguments.rs`, `crates/media/src/lib.rs`, `crates/media/src/describe.rs`, `crates/media/src/merge.rs`, `crates/media/src/ffmpeg.rs`, `crates/streams/src/ffmpeg.rs` (part, ~200: `InputFormat`, `copy_arguments`), `crates/streams/src/extension.rs`, `crates/core/src/manager/media_run/page.rs`, `crates/core/src/manager/media_run/network.rs`, `crates/core/src/manager/media_run/save.rs`, `crates/core/src/manager/media.rs`, `crates/core/src/manager/added_pages.rs`, `crates/desktop/src/add_dialog.rs` (part, ~80: `submit`), `crates/desktop/src/download_windows/media_form.rs` (part, ~80: `answer`), `crates/engine/src/naming.rs` (part, ~150: `sanitize`, `clean`), `crates/records/src/media/mod.rs`

### B08 Hostile stream manifests and running programs

- Status: pending
- Groups: G44, G45, G46
- Highest severity: P1
- Lines: about 7,700
- Findings: `14-1`, `14-2`, `14-3`, `14-4`, `14-5`, `14-6`, `13-1`, `13-2`, `13-3`, `13-5`, `Xshutdown-4`
- Files: `crates/streams/src/hls.rs`, `crates/streams/src/dash/template.rs`, `crates/streams/src/dash/mod.rs`, `crates/streams/src/dash/list.rs`, `crates/streams/src/dash/tests.rs`, `crates/streams/src/plan.rs`, `crates/streams/src/select.rs`, `crates/streams/src/source.rs`, `crates/streams/src/codec.rs`, `crates/streams/src/model.rs`, `crates/streams/src/error.rs`, `crates/streams/src/process/run.rs`, `crates/streams/src/process/tail.rs`, `crates/streams/src/process/tree.rs`, `crates/streams/src/process/mod.rs`, `crates/streams/examples/fake_program.rs`, `crates/streams/tests/process.rs`, `crates/streams/tests/manifest_memory.rs`, `crates/media/src/describe.rs` (part, ~60: `E::Key`), `crates/files/src/zone.rs` (part, ~30: its cut loop), `docs/ROADMAP.md` (part, ~10)

### B09 The store: reading, failure signal and repair

- Status: pending
- Groups: G47, G48, G49, G50
- Highest severity: P1
- Lines: about 6,800
- Findings: `03-1`, `03-2`, `04-6`, `03-6`, `Xdatabase-migrations-5`, `03-3`, `03-4`, `23-10`, `25-2`, `Xfeature-behaviour-3`, `03-8`, `03-5`, `Xdatabase-migrations-1`, `Xdatabase-migrations-4`, `Xdatabase-migrations-2`, `25-9`
- Files: `crates/store/src/lib.rs`, `crates/store/src/error.rs`, `crates/store/tests/samples.rs`, `crates/core/src/manager/restore.rs`, `crates/core/src/manager/startup.rs`, `crates/core/src/manager/mod.rs`, `crates/core/src/manager/preferences.rs`, `crates/desktop/src/library.rs`, `crates/desktop/src/notices.rs`, `crates/desktop/src/backend.rs`, `crates/desktop/src/main_window/notices_bar.rs`, `crates/desktop/src/main_window/empty.rs`, `crates/shell/src/host.rs`

### B10 Moves, quit and start

- Status: pending
- Groups: G51, G52, G53
- Highest severity: P1
- Lines: about 5,100
- Findings: `Xshutdown-1`, `Xconcurrency-4`, `Xcrash-and-resume-4`, `Xsecond-review-disk-writes-7`, `05-4`, `Xrepeated-logic-4`, `22-1`, `Xshutdown-3`, `22-7`, `22-5`, `22-6`, `22-8`, `U-2`
- Files: `crates/core/src/manager/relocate.rs`, `crates/core/src/manager/mod.rs`, `crates/core/src/manager/run.rs`, `crates/core/src/manager/state.rs` (part, ~200: `Hold`, `stranded`), `crates/core/src/manager/file_exists.rs` (part, ~80: `take_next_free_name`), `crates/core/src/manager/media_run/placement.rs`, `crates/core/src/naming.rs` (part, ~60: `is_free_on_disk`), `crates/engine/src/naming.rs` (part, ~120: `rename_no_replace`), `crates/engine/src/file_work.rs`, `crates/files/src/zone.rs`, `crates/shell/src/quit.rs`, `crates/shell/src/bridge.rs`, `crates/shell/src/autostart.rs`, `crates/desktop/src/backend.rs`, `crates/desktop/src/lifecycle.rs`, `crates/desktop/src/main.rs`, `crates/desktop/src/closing.rs`, `crates/desktop/src/sign_in.rs`, `crates/bridge/src/install/mod.rs` (part, ~150: `run`, `print_report`), `gpui-pre app.rs, gpui-pre-windows platform.rs, tokio blocking pool` (part, ~500: outside the repo, `Application::run`, `quit`, `BlockingPool` drop)

### B11 Engine HTTP and failure messages

- Status: pending
- Groups: G54, G55, G56, G57
- Highest severity: P1
- Lines: about 6,400
- Findings: `02-1`, `02-2`, `01-7`, `21-2`, `02-4`, `02-7`, `28-2`, `28-3`, `28-4`, `28-5`, `10-3`
- Files: `crates/engine/src/transport.rs`, `crates/engine/src/download.rs`, `crates/engine/src/http.rs`, `crates/engine/src/http/client.rs`, `crates/engine/src/http/range.rs`, `crates/engine/src/retry.rs`, `crates/engine/src/error.rs`, `crates/engine/src/naming.rs`, `crates/engine/src/worker.rs` (part, ~150: failure handling), `crates/engine/src/ftp/mod.rs`, `crates/engine/src/http3.rs` (part, ~100: `check_response`), `crates/engine/examples/bench_local.rs` (part, ~30: `serve`), `crates/messages/src/lib.rs`, `crates/media/src/describe.rs`, `crates/core/src/manager/suffix_list.rs` (part, ~150: `NotTaken`, `newer_suffix_list`), `crates/core/src/manager/addons/install.rs` (part, ~100: the install download), `crates/core/src/manager/app_update.rs` (part, ~100: the installer download)

### B12 Settings files, import and the add-on

- Status: pending
- Groups: G58, G59, G60, G61, G62, G63
- Highest severity: P1
- Lines: about 7,200
- Findings: `Xsecurity-addon-unpacking-1`, `Xsecurity-path-handling-2`, `19-1`, `27-2`, `19-2`, `27-11`, `30-4`, `Xfeature-behaviour-7`, `19-6`, `Xrepeated-logic-6`, `11-1`, `11-2`, `11-3`, `Xsecurity-addon-unpacking-3`, `11-4`, `Xsecurity-addon-unpacking-4`, `11-5`, `24-1`, `24-2`, `24-3`, `27-4`, `Xsecurity-addon-unpacking-2`
- Files: `crates/settings/src/folders.rs`, `crates/settings/src/settings_file.rs`, `crates/core/src/manager/preferences.rs`, `crates/core/src/manager/queues.rs`, `crates/desktop/src/main_window/backup_dialog.rs`, `crates/protocol/src/app.rs` (part, ~200: `ImportPreview`, `ImportSummary`, `AddonInfo`), `crates/bridge/src/install/host.rs`, `crates/files/src/whole_file.rs`, `crates/addons/src/lib.rs`, `crates/core/src/manager/addons/mod.rs`, `crates/core/src/manager/addons/install.rs`, `crates/core/src/manager/addons/updates.rs`, `crates/core/src/manager/addons/tests.rs`, `crates/desktop/src/main_window/settings/addon.rs`, `crates/desktop/src/main_window/settings/about.rs`, `crates/desktop/src/download_windows/media_form.rs`, `crates/desktop/src/download_windows/view.rs` (part, ~100: `addon_step`, `install_addon`), `crates/media/src/describe.rs`, `crates/media/src/ffmpeg.rs`, `crates/streams/src/process/run.rs` (part, ~100: how a failed status returns)

### B13 Main window and Library events

- Status: pending
- Groups: G64, G65, G66, G67, G68
- Highest severity: P1
- Lines: about 6,700
- Findings: `23-1`, `25-5`, `25-6`, `23-9`, `Xdatabase-migrations-3`, `24-9`, `26-7`, `27-8`, `25-1`, `25-3`, `25-4`, `U-4`, `25-7`, `25-8`, `25-10`, `25-11`, `25-12`, `U-3`, `U-8`, `U-10`, `U-15`
- Files: `crates/desktop/src/library.rs`, `crates/desktop/src/main_window/keymap.rs`, `crates/desktop/src/main_window/mod.rs`, `crates/desktop/src/main_window/list.rs`, `crates/desktop/src/main_window/selection.rs`, `crates/desktop/src/main_window/drag.rs`, `crates/desktop/src/main_window/confirm.rs`, `crates/desktop/src/main_window/row_menu.rs`, `crates/desktop/src/main_window/queue_header.rs`, `crates/desktop/src/main_window/order.rs`, `crates/desktop/src/main_window/sort.rs`, `crates/desktop/src/main_window/empty.rs`, `crates/desktop/src/main_window/fixes.rs`, `crates/desktop/src/main_window/details.rs`, `crates/desktop/src/main_window/applies.rs`, `crates/desktop/src/main_window/rail.rs`, `crates/desktop/src/main_window/sidebar.rs` (part, ~200: `foot`, `foot_icon`), `crates/desktop/src/main_window/command_bar.rs`, `crates/desktop/src/main_window/notices_bar.rs`, `crates/desktop/src/tray.rs`, `crates/desktop/src/main_window/queue_proxy.rs` (part, ~100: `listing`), `crates/desktop/src/download_windows/mod.rs`, `crates/shell/src/download_windows.rs` (part, ~200: `Registry::restore`)

### B14 The local network rule

- Status: pending
- Groups: G69, G70, G71, G72
- Highest severity: P2
- Lines: about 6,300
- Findings: `09-1`, `21-1`, `Xsecurity-bridge-and-pipe-input-1`, `Xsecurity-bridge-and-pipe-input-2`, `10-1`, `10-2`, `09-4`, `09-5`, `09-6`, `09-8`, `09-9`, `10-4`, `Xrepeated-logic-3`
- Files: `crates/engine/src/net_guard.rs`, `crates/engine/src/download.rs`, `crates/engine/src/ftp/mod.rs`, `crates/engine/src/transport.rs`, `crates/engine/src/url.rs`, `crates/engine/src/http.rs` (part, ~150: `send`), `crates/engine/src/proxy.rs` (part, ~100: the module header), `crates/sites/src/local_network.rs`, `crates/sites/src/public_suffix.rs`, `crates/sites/src/cookie_scope.rs`, `crates/sites/src/video_sites.rs`, `crates/queues/src/sites.rs`, `crates/media/src/ytdlp/mod.rs`, `crates/core/src/manager/media_run/network.rs`, `crates/core/src/manager/media_run/page.rs`, `crates/core/src/manager/captures.rs`, `crates/core/src/manager/media.rs` (part, ~300: `hold_media`, `analyse`, `read_page`)

### B15 Proxies and secrets in links

- Status: pending
- Groups: G73, G74, G75
- Highest severity: P2
- Lines: about 6,600
- Findings: `09-3`, `19-3`, `19-4`, `19-5`, `09-2`, `09-7`, `19-7`, `19-8`, `19-9`, `27-1`, `27-9`, `Xrepeated-logic-7`, `05-5`, `05-10`, `20-1`, `20-2`, `Xrepeated-logic-8`
- Files: `crates/engine/src/proxy.rs`, `crates/engine/src/proxy/tests.rs`, `crates/engine/tests/proxy.rs`, `crates/settings/src/proxy.rs`, `crates/settings/src/proxy/tests.rs`, `crates/core/src/proxy.rs`, `crates/core/src/manager/proxy.rs`, `crates/core/tests/manager/proxy.rs`, `crates/protocol/src/proxy.rs`, `crates/desktop/src/main_window/proxy_dialog.rs`, `crates/core/src/manager/media_run/page.rs` (part, ~40: `ytdlp_proxy` caller), `crates/core/src/manager/media.rs` (part, ~40: `ytdlp_proxy` caller), `crates/logging/src/redact.rs`, `crates/logging/src/redact/named.rs`, `crates/logging/src/redact/scan.rs`, `crates/logging/src/redact/tests.rs`, `crates/protocol/src/secrets.rs`, `crates/test-support/src/redaction.rs`, `crates/core/src/naming.rs`, `crates/core/src/manager/file_exists.rs`, `crates/engine/src/target.rs` (part, ~60: `same_download`), `crates/core/src/manager/state.rs` (part, ~60: `summary`), `crates/desktop/src/text.rs` (part, ~50: `link_for_display`)

### B16 Queues and queue forms

- Status: pending
- Groups: G76, G77, G78, G79
- Highest severity: P2
- Lines: about 5,100
- Findings: `18-1`, `18-2`, `18-3`, `04-2`, `18-4`, `18-5`, `27-10`, `Xrepeated-logic-5`, `27-3`, `27-5`, `U-5`, `U-11`, `U-12`, `U-13`
- Files: `crates/core/src/manager/scheduling.rs`, `crates/queues/src/schedule.rs`, `crates/queues/src/plan.rs`, `crates/queues/src/rules.rs`, `crates/core/src/manager/restore.rs` (part, ~150: `restore_entry`), `crates/core/src/manager/run.rs` (part, ~150: `start_run`, `fail_to_start`, `finish`), `crates/core/src/manager/mod.rs` (part, ~100: `run_scheduler`), `crates/core/tests/manager/queues.rs`, `crates/core/tests/manager/schedule.rs`, `crates/desktop/src/main_window/sidebar.rs`, `crates/desktop/src/main_window/queue_header.rs`, `crates/desktop/src/main_window/queue_rules.rs`, `crates/desktop/src/main_window/queue_dialog.rs`, `crates/desktop/src/main_window/queue_delete.rs`, `crates/desktop/src/main_window/list.rs` (part, ~100: `time_cell`), `crates/settings/src/lib.rs` (part, ~120: `category_of`, `clean_extensions`)

### B17 Download windows and notifications

- Status: pending
- Groups: G80, G81, G82, G83, G84
- Highest severity: P2
- Lines: about 6,900
- Findings: `24-4`, `30-1`, `24-6`, `U-7`, `U-6`, `30-2`, `Xfeature-behaviour-5`, `24-5`, `24-7`, `U-14`, `24-8`, `24-10`, `26-1`, `26-5`, `26-6`, `26-8`, `26-2`, `26-3`, `26-4`, `26-9`
- Files: `crates/desktop/src/download_windows/view.rs`, `crates/desktop/src/download_windows/finished.rs`, `crates/desktop/src/download_windows/mod.rs`, `crates/shell/src/download_windows.rs`, `crates/shell/src/finished.rs`, `crates/protocol/src/desktop.rs`, `crates/protocol/src/app.rs` (part, ~100: `FileExistsQuestion`, `RETRY_FIXES`), `crates/desktop/src/main_window/fixes.rs`, `crates/core/src/manager/captures.rs` (part, ~80: `insert_confirmed`), `crates/core/src/naming.rs`, `crates/core/src/manager/file_exists.rs`, `crates/desktop/src/notices.rs`, `crates/desktop/src/tray.rs`, `crates/shell/src/browser_notices.rs`, `crates/shell/src/teller.rs`, `crates/shell/src/notifications.rs`, `crates/shell/src/system_notifications.rs`, `crates/shell/src/activity.rs`, `crates/shell/src/tray.rs`, `crates/shell/src/tray_speed.rs`, `crates/shell/src/tray_queues.rs`, `crates/shell/src/tray_proxy.rs`

### B18 Settings pages, links and text

- Status: pending
- Groups: G85, G86, G87, G88
- Highest severity: P2
- Lines: about 5,400
- Findings: `23-5`, `30-3`, `Xfeature-behaviour-4`, `22-2`, `Xpath-normal-download-4`, `Xrepeated-logic-1`, `23-2`, `23-3`, `23-4`, `23-8`, `U-9`, `27-6`, `27-7`, `U-1`, `23-6`, `23-7`, `Xpath-normal-download-7`
- Files: `crates/desktop/src/links.rs`, `crates/desktop/src/add_dialog.rs`, `crates/desktop/src/text.rs`, `crates/desktop/src/files.rs`, `crates/desktop/src/theme.rs`, `crates/desktop/src/main_window/settings/general.rs`, `crates/desktop/src/main_window/settings/media.rs`, `crates/desktop/src/main_window/settings/mod.rs`, `crates/desktop/src/main_window/settings/addon.rs`, `crates/desktop/src/main_window/settings/network.rs`, `crates/shell/src/api.rs`, `crates/shell/src/launch.rs` (part, ~60: `LINK_SCHEMES`, `is_launch_link`), `crates/engine/src/url.rs`, `crates/protocol/src/app.rs` (part, ~200: `SuffixListInfo`), `crates/core/src/manager/suffix_list.rs` (part, ~50: its events), `crates/core/src/manager/media.rs` (part, ~100: `remember_media_choice`), `crates/settings/src/lib.rs` (part, ~50: `folder_for`), `crates/media/src/describe.rs` (part, ~60: the reading error)

### B19 Tests, checks and dead code

- Status: pending
- Groups: G89, G90, G91, G92, G93
- Highest severity: P2
- Lines: about 6,700
- Findings: `01-9`, `01-10`, `33-2`, `03-7`, `04-7`, `05-14`, `06-3`, `10-5`, `13-4`, `21-3`, `33-1`, `33-3`, `33-4`, `33-5`, `33-6`, `31-1`, `31-2`, `31-3`, `31-4`, `02-8`, `07-7`, `17-9`, `23-11`, `24-11`, `30-5`, `30-6`, `30-7`, `07-6`, `19-10`, `03-9`, `27-12`
- Files: `scripts/gate/test-timing.mjs`, `scripts/gate/test-timing.test.mjs`, `crates/engine/tests/download.rs` (part, ~100: `pause_midway`, `another_program_saves_meanwhile`), `crates/engine/tests/options.rs`, `crates/store/src/lib.rs` (part, ~200: the named tests, `lock_path`), `crates/core/tests/manager/lifecycle.rs` (part, ~60), `crates/core/tests/manager/settle.rs`, `crates/bridge/src/app.rs` (part, ~120: the named tests), `crates/bridge/tests/bridge.rs` (part, ~60: `LINGER`), `crates/e2e/src/bin/wirecatch-e2e-app.rs`, `crates/sites/src/local_network.rs` (part, ~60: the named test), `crates/streams/tests/process.rs` (part, ~100: the named tests), `crates/engine/tests/ftp.rs` (part, ~150: the named tests), `crates/engine/src/ftp/tests.rs` (part, ~100: the named tests), `crates/engine/src/ftp/control.rs` (part, ~60: `control_reading`), `scripts/linux-test.sh`, `scripts/gate.mjs`, `scripts/gate/process.mjs`, `scripts/gate/product.test.mjs`, `scripts/gate/js-policy.mjs`, `scripts/gate/browser.mjs`, `scripts/product/read.test.mjs`, `scripts/product/read.mjs`, `scripts/product/outputs.mjs`, `scripts/product/manifests.mjs`, `config/product.toml`, `crates/test-support/src/http.rs`, `crates/test-support/src/http3.rs`, `crates/test-support/src/files.rs`, `crates/test-support/src/ftp.rs`, `crates/engine/src/http3/tests.rs` (part, ~30: `test_data`), `crates/engine/examples/bench_http3.rs` (part, ~30: `run`), `crates/core/src/manager/media_run/tests.rs` (part, ~60: the `Job` literals), `crates/core/tests/manager/media_support.rs` (part, ~50), `crates/core/tests/manager/again.rs` (part, ~30: `click`), `crates/shell/src/api.rs` (part, ~150: the named items), `crates/shell/src/download_windows.rs` (part, ~100: the named items), `crates/shell/src/finished.rs` (part, ~50: `lists`, `next_key`), `crates/protocol/src/exported.rs` (part, ~100: `timed_lists!`, the `media_start` fixtures), `crates/protocol/src/media_start.rs` (part, ~100: `start_cases`), `crates/protocol/src/languages.rs` (part, ~30: `tw`, `os`), `crates/server/src/lib.rs` (part, ~60: `serve_connection`), `crates/core/src/manager/preferences.rs` (part, ~60: `update_settings`), `crates/desktop/src/main_window/settings/mod.rs` (part, ~50: `Typing::read`), `crates/desktop/src/main_window/queue_rules.rs` (part, ~50: `lines`, `file_types`), `crates/desktop/src/main_window/queue_proxy.rs` (part, ~50: `exceptions`), `crates/desktop/src/main_window/queue_header.rs` (part, ~60: `limits`, `summary`), `crates/desktop/src/main_window/settings/speed.rs` (part, ~50: `queue_facts`), `crates/desktop/src/main_window/list.rs` (part, ~40: `found`), `crates/desktop/src/main_window/backup_dialog.rs` (part, ~40: `run`), `crates/desktop/src/main_window/link_trial.rs` (part, ~40: `try_link`), `crates/desktop/src/main_window/proxy_dialog.rs` (part, ~60: `test`, `save`, `text_field`), `crates/desktop/src/main_window/queue_dialog.rs` (part, ~60: `save`, `text_field`), `crates/files/src/lib.rs`

## Suggestions

Not validated in batches. An `=` marks the same suggestion from different reviewers.

- **Engine downloads and resume:** `02-S1`, `02-S2`, `02-S3`, `21-S1`, `Xconcurrency-S1`, `Xsecond-review-disk-writes-S1` (overlaps finding `01-3`), `Xsecond-review-resume-S1`, `Xsecond-review-resume-S2`.
- **Saved list and crash recovery:** `03-S1`, `Xcrash-and-resume-S1`, `Xcrash-and-resume-S2`, `Xdatabase-migrations-S1`, `Xdatabase-migrations-S2`, `Xsecurity-path-handling-S1`.
- **Start and quit:** `22-S1` = `Xshutdown-S1`, `22-S2`, `Xshutdown-S2`.
- **Bridge, captures and links with passwords:** `06-S1`, `07-S1`, `25-S1`.
- **App updates and release:** `12-S1`, `12-S2` = `Xpath-app-update-S1` = `Xsecurity-update-signatures-S4`, `12-S3` = `Xpath-app-update-S2` = `Xsecurity-update-signatures-S5`, `Xsecurity-update-signatures-S1`, `Xsecurity-update-signatures-S2`, `Xsecurity-update-signatures-S3`, `Xfeature-behaviour-S1`, `29-S1`.
- **Media add-on:** `11-S1`, `Xsecurity-addon-unpacking-S1` (overlaps finding `11-1`), `Xsecurity-addon-unpacking-S2`.
- **Video and audio:** `13-S1`, `17-S1`, `17-S2`, `30-S2`, `Xpath-media-download-S1`, `Xpath-media-download-S2`, `Xcrash-and-resume-S3`.
- **Queues:** `18-S1`.
- **Proxies:** `19-S1`.
- **Main window:** `25-S2`, `U-S4`, `U-S5`, `U-S8`, `U-S12`.
- **Download windows:** `U-S6` (the same request as finding `30-2`), `U-S7`, `U-S11`.
- **Settings and forms:** `27-S1`, `27-S2`, `U-S1`, `U-S2`, `U-S3`, `U-S9`, `U-S10`.
- **Notifications:** `26-S1`.
- **Repository:** `30-S1`, `33-S1`.

## Coverage notes

Left out or only searched by the flow reviewers. Stage 3 reports these as untested or not reviewed, not as passing.

- **Concurrency:** races inside media questions and add-on installs: `core/manager/media.rs`, `core/manager/addons/`, `media_run/page.rs`, and the desktop's window code.
- **Crash and resume:** `store/src/lib.rs`, `records`, `streams/download/fetch.rs`, `core/batches.rs`, `core/manager/media.rs`, `media_run/extras.rs`, so a batch's state across a crash is not covered. The store's own repair is covered by the migrations flow.
- **Database migrations:** no build or test was run.
- **Feature behaviour:** connections were followed, not files read whole, and nothing ran in the app. `crates/server`, `apps/extension`, `release.yml` and the packaging scripts were searched only.
- **App update path:** `manifest.test.mjs`, `updates/samples/update.json`, the engine's download loop. No real update ran.
- **Media download path:** `streams` `hls`, `dash`, `plan`, `select`, `workdir`, `download/fetch`; `media` `ffmpeg`, `encoders`, `metadata`, `cookies`; most of `media_form`, `media_output`, `added_pages`, `captures`.
- **Normal download path:** the bridge's relay, `cli/src/main.rs`, the engine's `worker`, `transport`, `http` and `segment`, all taken from contracts.
- **Repeated logic:** only suspected copies were read; the rest of each crate was searched.
- **Second reviews:** parts of `engine/naming.rs`, `fetch.rs`, `http`, `transport`, `core/manager/run.rs`, `downloads.rs`, `state.rs`, `relocate.rs`. Nothing was built or run against a full disk.
- **Add-on security:** add-on tests searched only; `process/tree.rs` from its contract; Linux read, not run. SHA-256 values and the Linux tar's contents were not checked (chunk 11).
- **Bridge and pipe security:** `bridge/relay.rs`, `raw_frame.rs`, `caller.rs`, `shell_parent.rs` not read; how `streams` uses headers on segments not read; nothing was run against a real app, FTP server or yt-dlp.
- **Path handling security:** `relocate.rs`, `file_exists.rs`, `restore.rs` from contracts; `addons/archive.rs` left to the add-on flow; `streams/workdir.rs` and `download/join.rs` searched for path joins only.
- **Update signatures:** `scripts/package.mjs` and the engine from contracts; nothing on Linux, and no update ran.
- **Shutdown:** the engine's `FileWork` and writer, `core/manager/addons/`, `media.rs`, `crates/server` and `crates/ipc`'s endpoint release from contracts; GPUI's Linux platform not read.
- **UI states not reached:** the storage failure banner (M19), system notifications (N1, N2), the backup dialog (Q6), the refresh link dialog (Q7), the update banner (O2), tray submenus, and any shortcut with Ctrl or Alt.
- **Fault tests not run:** full disk, power cuts, Linux, kills and size faults over HTTP/2, HTTP/3 and FTP, and files over 4 GiB through `Manager`.
- **Packaging:** no installer was built or run (chunk 29).
