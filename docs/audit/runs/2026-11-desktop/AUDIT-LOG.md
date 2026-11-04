# Audit log: desktop run

The result of the code audit of the `desktop` scope, on branch `docs/record-open-items`, code at `02284bbd0`. How it was run is in `docs/audit/AUDIT-PLAN.md`. Fix down `## Fix order`, one item at a time; `## Items` holds what each one needs.

## Summary

- **Reported:** 403 findings. 351 from the chunks, flows, UI review and fault tests, 48 new ones the validators found, 4 from the mutation run.
- **Distinct bugs after merging:** 334. 66 reports repeated another report of the same bug, and `B05-2` repeats `Xrepeated-logic-7`.
- **Disproved:** 2 (`12-5`, `B01-2`), listed at the end.
- **Downgraded:** 29. Also, 10 reports were raised to match the severity of the bug they repeat.
- **Final severity of the 401 reports left:** 4 P0, 34 P1, 180 P2, 183 P3.
- **Fix items:** 244. 3 P0, 19 P1, 116 P2, 106 P3. An item takes the highest severity of its findings.

**What the audit established.** Every chunk and flow was read by one reader and checked by a second who tried to disprove each finding. All 25 patches behind the P0 and P1 findings were rerun in one clean build at `5cb4b35f5`, with the same code the verdicts judged, and each failed as claimed. The real downloader survived dropped connections, ranges that stop working, lying sizes, a file changing mid-download, files over 4 GiB and killed processes, with two small exceptions (`F-1`, `F-2`).

**What it did not establish.** P0 and P1 verdicts rest on failing tests rerun in a clean build where the verdict says so, while most P2 and P3 verdicts rest on a second reader's judgement. Only 2 reports out of about 400 were disproved, so a few lower findings may still be wrong. That is why "Before starting an item" in the plan matters: check that the item still applies, and that the finding is real, before fixing it. Full disk, power loss, Linux, HTTP/2, HTTP/3 and FTP faults, a real update and real installers were not tested; `## Coverage` states this in full.

**How the order was made.** The plan's five rules, applied in turn: blockers first; P0 before all else; a change that reshapes shared code before fixes inside it; then severity, findings resolved and items unblocked; then effort. Within a severity this puts items with more findings first even when a single-finding item looks more serious to a reader, as with item 20 below item 15. Where a P2 or P3 sits above its severity, "Why here" says what it clears the way for.

**Already in `docs/ROADMAP.md`'s open items.** Not listed twice there: item 164 is "A failed video shows the add-on's raw output", and item 199 extends "On Linux, yt-dlp and ffmpeg keep running after the app crashes". Item 51 takes in part of "The wrong-size update message is awkward", and suggestion `U-S1` is "The Download folder field may read as a placeholder". "A real update" and "Real installers" are in `## Coverage`.

## Fix order

This is the order of the fixes; `WORK-LIST.md` groups them into pull requests, and fixing works from that list, starting from each proof. Items marked with a reason in "Why here" sit above their severity only because they clear the way for something below. Status is `open`, `fixed` or `won't fix (reason)`.

| #   | Item                                                                                              | Findings                                                                                           | Severity | Effort | Why here                                                                                                         | Proof                                         | Status |
| --- | ------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------- | -------- | ------ | ---------------------------------------------------------------------------------------------------------------- | --------------------------------------------- | ------ |
| 1   | Two downloads can end up writing the same file                                                    | `05-1` `Xconcurrency-1` `05-13` `Xconcurrency-9` `B01-3`                                           | P0       | large  | P0 with the most findings; reshapes the path claims that later file-exists fixes sit on                          | `05-1` `Xconcurrency-1`                       | fixed  |
| 2   | Removing a finished download can delete a whole folder                                            | `Xsecurity-path-handling-1` `04-8`                                                                 | P0       | small  | P0, permanent loss from one click; depends on nothing                                                            | `Xsecurity-path-handling-1`                   | fixed  |
| 3   | A cut-off download of unknown size is reported as complete                                        | `Xsecond-review-disk-writes-2`                                                                     | P0       | small  | P0, silent cut file on ordinary conditions; depends on nothing                                                   | `Xsecond-review-disk-writes-2`                | fixed  |
| 4   | A restart can mark a download finished on a file it never downloaded                              | `04-1` `Xcrash-and-resume-1` `Xsecond-review-resume-1` `B02-2`                                     | P1       | medium | P1 that reshapes what a saved row records, which several later items use                                         | `04-1`                                        | fixed  |
| 5   | A browser download that meets Ask is dropped and goes back to the browser                         | `05-3` `07-1` `Xconcurrency-3` `Xpath-normal-download-2`                                           | P1       | small  | P1, 4 findings; must land before item 11, whose fix makes this path common                                       | `05-3` `Xconcurrency-3-and-6`                 | fixed  |
| 6   | An inherited APPIMAGE variable can make the app overwrite or start another program                | `12-1` `Xpath-app-update-4` `Xsecurity-update-signatures-2` `B05-1`                                | P1       | small  | P1, 4 findings; must land before item 7 switches updates on                                                      | `12-1`                                        | fixed  |
| 7   | The app never looks for updates, and every proxy test fails                                       | `Xfeature-behaviour-1` `Xpath-app-update-1` `Xsecurity-update-signatures-1` `Xfeature-behaviour-2` | P1       | small  | P1, 4 findings; right after the item that makes updates safe to switch on                                        | `Xfeature-behaviour-1` `Xfeature-behaviour-2` | fixed  |
| 8   | A move across drives can leave a cut-off file under the final name                                | `Xsecond-review-disk-writes-7` `Xcrash-and-resume-4`                                               | P2       | medium | P2 placed early: ending the process at the quit limit would make this cut-off file reachable on an ordinary quit | none                                          | fixed  |
| 9   | Quitting during a move leaves the list pointing at the wrong place                                | `Xshutdown-1` `Xconcurrency-4`                                                                     | P1       | medium | P1; needs the staged copy, and shapes what the quit limit waits for                                              | `Xshutdown-1`                                 | open   |
| 10  | The quit time limit does not end the process                                                      | `Xshutdown-3` `22-1` `B10-2` `22-7`                                                                | P1       | medium | P1, 4 findings; its two blockers sit just above it                                                               | `Xshutdown-3`                                 | open   |
| 11  | "If the file is already in the folder" is skipped for names from the link or browser              | `05-2` `Xpath-normal-download-1` `B01-1`                                                           | P1       | medium | P1, 3 findings; after the items its fix would otherwise make common                                              | `05-2`                                        | open   |
| 12  | A settings file can send downloads into folders whose programs the app runs                       | `Xsecurity-path-handling-2` `Xsecurity-addon-unpacking-1` `B12-1`                                  | P1       | medium | P1, 3 findings, medium                                                                                           | `Xsecurity-path-handling-2`                   | fixed  |
| 13  | Replace on a file with another extension downloads everything again and strands the old folder    | `17-2` `Xpath-media-download-5`                                                                    | P2       | medium | P2 placed early: it strands the folders that the next item trusts                                                | `17-2`                                        | fixed  |
| 14  | A new media download can save another download's video                                            | `17-1` `Xsecond-review-resume-2` `Xpath-media-download-6`                                          | P1       | medium | P1, 3 findings, medium; after item 13                                                                            | `17-1`                                        | open   |
| 15  | Typing a space in search pauses downloads, and Enter can open a file                              | `25-1` `B13-1`                                                                                     | P1       | small  | P1, 2 findings, small                                                                                            | `25-1`                                        | open   |
| 16  | A very large HLS playlist freezes inspection for minutes to hours                                 | `14-2` `B08-1`                                                                                     | P1       | small  | P1, 2 findings, small                                                                                            | `14-2`                                        | fixed  |
| 17  | A video with a web page or nothing in place of a segment is reported as complete                  | `15-1` `B06-1`                                                                                     | P1       | medium | P1, 2 findings, medium                                                                                           | `15-1`                                        | fixed  |
| 18  | A lower-integrity program can stand in for the app and receive downloads' cookies                 | `08-1` `08-2`                                                                                      | P1       | medium | P1, 2 findings, medium                                                                                           | `08-1` `B04-p2-checks`                        | open   |
| 19  | A DASH template can ask for an address of hundreds of gigabytes and abort the app                 | `14-1`                                                                                             | P1       | small  | P1, 1 finding, small                                                                                             | `14-1`                                        | fixed  |
| 20  | A failed flush while saving progress is forgotten                                                 | `Xsecond-review-disk-writes-1`                                                                     | P1       | small  | P1, 1 finding, small                                                                                             | `Xsecond-review-disk-writes-1`                | fixed  |
| 21  | An expired redirect target fails a download whose link still works                                | `02-1`                                                                                             | P1       | small  | P1, 1 finding, small                                                                                             | `02-1`                                        | fixed  |
| 22  | One unreadable value in the list stops the app opening at every start                             | `03-1`                                                                                             | P1       | medium | P1, 1 finding, medium                                                                                            | `03-1`                                        | open   |
| 23  | Sources are offered that ffmpeg then refuses after the whole download                             | `Xpath-media-download-1`                                                                           | P1       | medium | P1, 1 finding, medium                                                                                            | `Xpath-media-download-1`                      | fixed  |
| 24  | A video format that needs cookies always fails when the engine fetches it                         | `Xpath-media-download-2`                                                                           | P1       | medium | P1, 1 finding, medium                                                                                            | `Xpath-media-download-2`                      | fixed  |
| 25  | The could-not-save banner is wrong both ways, and its Try again saves nothing                     | `03-4` `23-10` `25-2` `Xfeature-behaviour-3` `03-8`                                                | P2       | medium | P2, 5 findings, medium                                                                                           | `25-2`                                        | open   |
| 26  | Settings, queues and proxies that cannot be read are silently replaced                            | `03-2` `04-6` `B09-1` `B09-2`                                                                      | P2       | medium | P2, 4 findings, medium                                                                                           | none                                          | open   |
| 27  | A removal under way at quit comes back, and a capture is downloaded twice                         | `04-5` `Xconcurrency-2` `Xshutdown-2` `B02-3`                                                      | P2       | medium | P2, 4 findings, medium                                                                                           | `Xconcurrency-2` `Xshutdown-2`                | fixed  |
| 28  | Nothing knows whether a download can continue, so messages promise what will not happen           | `01-6` `32-1` `28-1` `B03-1`                                                                       | P2       | medium | P2, 4 findings, medium                                                                                           | none                                          | open   |
| 29  | The Public Suffix List's date and Check now were never put in Settings                            | `23-5` `30-3` `Xfeature-behaviour-4` `B19-4`                                                       | P2       | medium | P2, 4 findings, medium                                                                                           | none                                          | open   |
| 30  | The window redoes whole-list work on every frame and progress tick                                | `25-6` `26-7` `27-8` `B13-3`                                                                       | P2       | large  | P2, 4 findings, large                                                                                            | none                                          | open   |
| 31  | The Add dialog's link rule leaves out ftpes, so such links cannot be added                        | `22-2` `Xrepeated-logic-1` `Xpath-normal-download-4`                                               | P2       | small  | P2, 3 findings, small                                                                                            | none                                          | open   |
| 32  | A raw non-ASCII byte or another charset in a header loses the file name or fails a redirect       | `02-2` `B11-1` `01-7`                                                                              | P2       | small  | P2, 3 findings, small                                                                                            | `02-2` `B11-1` `01-7`                         | fixed  |
| 33  | Failing to delete a temporary file turns a saved video into a failure                             | `15-2` `Xsecond-review-disk-writes-6` `B06-2`                                                      | P2       | small  | P2, 3 findings, small                                                                                            | none                                          | fixed  |
| 34  | A damaged or squatted key does not lead the bridge to start the app and renew it                  | `06-1` `08-3` `B04-2`                                                                              | P2       | small  | P2, 3 findings, small                                                                                            | `B04-p2-checks`                               | open   |
| 35  | An open file-exists question outlives the download state it asked about                           | `04-4` `05-7` `Xconcurrency-8`                                                                     | P2       | medium | P2, 3 findings, medium                                                                                           | `04-4`                                        | open   |
| 36  | Proxy exceptions are accepted that can never match                                                | `09-3` `19-3` `19-5`                                                                               | P2       | medium | P2, 3 findings, medium                                                                                           | none                                          | fixed  |
| 37  | A typed file name is dropped when the download turns out to be a page                             | `17-7` `Xpath-normal-download-5` `Xfeature-behaviour-6`                                            | P2       | medium | P2, 3 findings, medium                                                                                           | `B07-p2-tests`                                | fixed  |
| 38  | Pages yt-dlp describes at the top level, or with several videos, are misread                      | `Xpath-media-download-3` `Xpath-media-download-4` `B07-1`                                          | P2       | medium | P2, 3 findings, medium                                                                                           | `B07-p2-tests`                                | fixed  |
| 39  | The CLI strands files under another name when a resume file is damaged or the name is numbered    | `01-1` `F-1` `M-2`                                                                                 | P2       | medium | P2, 3 findings, medium                                                                                           | `FT-kill`                                     | fixed  |
| 40  | The add-on's state mixes an operation with whether the tools work                                 | `24-3` `27-4` `B12-2`                                                                              | P2       | medium | P2, 3 findings, medium; clears the way for item 53                                                               | none                                          | open   |
| 41  | A download retried or removed just after it ended is still told as ended                          | `26-5` `26-8` `B17-2`                                                                              | P2       | medium | P2, 3 findings, medium                                                                                           | none                                          | open   |
| 42  | The network guard's local-address rule is written twice                                           | `09-8`                                                                                             | P3       | small  | P3 placed here: merging the copies comes before the FTP fix that needs the rule                                  | none                                          | open   |
| 43  | The local network rule does not apply to FTP                                                      | `09-1` `21-1` `Xsecurity-bridge-and-pipe-input-1`                                                  | P2       | medium | P2, 3 findings, medium; after item 42                                                                            | none                                          | open   |
| 44  | "This host is under that site" is written four times, and the copies differ                       | `10-4` `Xrepeated-logic-3`                                                                         | P3       | medium | P3 placed here: merging the copies comes before the host fix in the next item                                    | none                                          | fixed  |
| 45  | ftps and ftpes hosts are read raw by every check                                                  | `21-1` `Xsecurity-bridge-and-pipe-input-1` `Xrepeated-logic-3` `B14-1`                             | P2       | small  | P2, 4 findings, small; after item 44                                                                             | none                                          | open   |
| 46  | An apostrophe in a link leaves its password or signed query in the log                            | `20-1`                                                                                             | P2       | small  | P2; must land before item 47 removes the mask that hides this today                                              | none                                          | fixed  |
| 47  | A link's password reaches the browser extension, and a crash's partial file is not picked up      | `05-5` `05-10` `Xrepeated-logic-8`                                                                 | P2       | medium | P2, 3 findings, medium; after item 46                                                                            | none                                          | open   |
| 48  | A finished file with a leftover resume file is downloaded again                                   | `Xcrash-and-resume-2` `Xpath-normal-download-6`                                                    | P2       | small  | P2, 2 findings, small; after item 4; clears the way for item 174                                                 | `Xcrash-and-resume-2`                         | open   |
| 49  | A title holding % or $ sends yt-dlp's file to another folder or stops it                          | `16-1` `Xsecurity-path-handling-3`                                                                 | P2       | small  | P2, 2 findings, small                                                                                            | none                                          | fixed  |
| 50  | Path parameters such as ;type=i or ;jsessionid end up in the file name                            | `21-2` `B11-2`                                                                                     | P2       | small  | P2, 2 findings, small                                                                                            | `21-2` `B11-2`                                | fixed  |
| 51  | Add-on installs and app updates get advice about links and folders they do not have               | `28-3` `B11-3`                                                                                     | P2       | small  | P2, 2 findings, small                                                                                            | none                                          | open   |
| 52  | A damaged list set aside can be deleted at the next start                                         | `03-5` `Xdatabase-migrations-1`                                                                    | P2       | small  | P2, 2 findings, small                                                                                            | `03-5`                                        | open   |
| 53  | The quality form loses the person's choice or says nothing around an add-on install               | `24-1` `24-2`                                                                                      | P2       | small  | P2, 2 findings, small; after item 40                                                                             | none                                          | open   |
| 54  | The file-exists question never says whether it is the same file                                   | `24-4` `30-1`                                                                                      | P2       | small  | P2, 2 findings, small                                                                                            | none                                          | open   |
| 55  | Cancel in a download window discards the partial file with no question                            | `24-7` `U-14`                                                                                      | P2       | small  | P2, 2 findings, small                                                                                            | none                                          | open   |
| 56  | A pasted link is cut at an apostrophe, a space or a bracket, so another address is downloaded     | `23-2` `Xpath-normal-download-4`                                                                   | P2       | small  | P2, 2 findings, small                                                                                            | none                                          | open   |
| 57  | Choosing a file or folder without a file chooser portal does nothing and says nothing             | `23-6` `B18-1`                                                                                     | P2       | small  | P2, 2 findings, small                                                                                            | none                                          | open   |
| 58  | A typed file name with a dot loses the words after it                                             | `17-3` `Xrepeated-logic-2`                                                                         | P2       | small  | P2, 2 findings, small                                                                                            | `17-3`                                        | open   |
| 59  | Downloaded installers are never removed                                                           | `12-2` `Xpath-app-update-5`                                                                        | P2       | small  | P2, 2 findings, small                                                                                            | none                                          | fixed  |
| 60  | An update with no usable file is shown as a package manager update or opens the release page      | `Xpath-app-update-6` `Xsecurity-update-signatures-3`                                               | P2       | small  | P2, 2 findings, small                                                                                            | none                                          | open   |
| 61  | A conversion demands audio or video that a format did not describe                                | `Xpath-media-download-10` `B07-4`                                                                  | P2       | small  | P2, 2 findings, small                                                                                            | none                                          | fixed  |
| 62  | A torn last line of a stream's log can truncate its joined track                                  | `Xsecond-review-disk-writes-4` `Xsecond-review-resume-3`                                           | P2       | small  | P2, 2 findings, small                                                                                            | `Xsecond-review-disk-writes-4`                | fixed  |
| 63  | Blank lines hide a program's failure reason and grow memory without limit                         | `13-1` `13-5`                                                                                      | P2       | small  | P2, 2 findings, small                                                                                            | `13-1`                                        | open   |
| 64  | No test covers control characters in names or a part file that cannot be created                  | `M-1` `M-3`                                                                                        | P2       | small  | P2, 2 findings, small                                                                                            | none                                          | fixed  |
| 65  | Ctrl+C in the CLI can exit with the wrong code or abort                                           | `32-2` `32-3`                                                                                      | P2       | small  | P2, 2 findings, small                                                                                            | none                                          | fixed  |
| 66  | Keep existing, Replace and captures leave the wrong category on the row                           | `05-11` `07-4`                                                                                     | P2       | small  | P2, 2 findings, small                                                                                            | none                                          | open   |
| 67  | A finished media file is not flushed before its sources are deleted                               | `Xsecond-review-disk-writes-3` `Xsecond-review-resume-7`                                           | P2       | small  | P2, 2 findings, small                                                                                            | none                                          | fixed  |
| 68  | Leftover proxy code from the earlier form has no caller                                           | `19-8` `Xrepeated-logic-7` `B05-2`                                                                 | P3       | small  | P3 placed here: removing the third copy comes before the fix to the live one                                     | none                                          | fixed  |
| 69  | yt-dlp ignores proxy exceptions and the page's scheme                                             | `19-4` `B15-1`                                                                                     | P2       | small  | P2, 2 findings, small; after item 68                                                                             | none                                          | fixed  |
| 70  | Choosing a path blocks an async thread on the disk, so a dead share stalls every command          | `05-9` `Xconcurrency-5`                                                                            | P2       | medium | P2, 2 findings, medium                                                                                           | none                                          | fixed  |
| 71  | A passing read error or a failed rename deletes a download's data                                 | `Xsecond-review-resume-4` `Xsecond-review-disk-writes-5`                                           | P2       | medium | P2, 2 findings, medium                                                                                           | none                                          | open   |
| 72  | The app quits before the Windows setup has asked for approval, so a refusal reaches no one        | `Xpath-app-update-2` `Xsecurity-update-signatures-5`                                               | P2       | medium | P2, 2 findings, medium                                                                                           | none                                          | fixed  |
| 73  | An AppImage restart can be lost while the old copy exits                                          | `12-3` `Xpath-app-update-3`                                                                        | P2       | medium | P2, 2 findings, medium                                                                                           | none                                          | fixed  |
| 74  | A stored bad part can never be thrown away, so Refresh link cannot help                           | `15-3` `Xpath-media-download-6`                                                                    | P2       | medium | P2, 2 findings, medium                                                                                           | none                                          | fixed  |
| 75  | A refused HLS or yt-dlp address fails instead of reading the page again                           | `17-5` `B07-3`                                                                                     | P2       | medium | P2, 2 findings, medium                                                                                           | `B07-p2-tests`                                | open   |
| 76  | The import preview hides proxy and local site changes                                             | `19-1` `27-2`                                                                                      | P2       | medium | P2, 2 findings, medium                                                                                           | none                                          | open   |
| 77  | A missing library is not seen as a broken add-on, and can raise a system dialog                   | `Xsecurity-addon-unpacking-2` `B12-3`                                                              | P2       | medium | P2, 2 findings, medium                                                                                           | none                                          | open   |
| 78  | A download moved to another queue shows in the wrong place                                        | `23-1` `25-5`                                                                                      | P2       | medium | P2, 2 findings, medium                                                                                           | none                                          | open   |
| 79  | The window and a restart disagree with what a queue's run really is                               | `18-2` `18-3`                                                                                      | P2       | medium | P2, 2 findings, medium; clears the way for item 138                                                              | none                                          | open   |
| 80  | Answering the file-exists question opens a second window for the same download                    | `24-6` `U-7`                                                                                       | P2       | medium | P2, 2 findings, medium                                                                                           | none                                          | open   |
| 81  | Download windows offer only Try again, whatever fix the failure needs                             | `30-2` `Xfeature-behaviour-5`                                                                      | P2       | medium | P2, 2 findings, medium                                                                                           | none                                          | open   |
| 82  | Refreshing the link cannot clear the check against what the browser saw                           | `Xpath-normal-download-3`                                                                          | P2       | small  | P2 with one finding; placed here because saving this check alone would remove the only way out of it             | `Xpath-normal-download-3`                     | open   |
| 83  | An answered Replace and a capture's size check are lost at a restart                              | `05-6` `Xcrash-and-resume-5`                                                                       | P2       | medium | P2, 2 findings, medium; after items 4, 82                                                                        | `05-6` `Xcrash-and-resume-5`                  | open   |
| 84  | A write that can never pass raises a notification for every batch                                 | `03-3`                                                                                             | P2       | small  | P2, 1 finding, small                                                                                             | `03-3`                                        | open   |
| 85  | Choosing the folder the guess already points at is not remembered                                 | `05-8`                                                                                             | P2       | small  | P2, 1 finding, small                                                                                             | none                                          | open   |
| 86  | A capture joining an unfinished download is answered Started too early                            | `07-2`                                                                                             | P2       | small  | P2, 1 finding, small                                                                                             | none                                          | fixed  |
| 87  | Skipped bytes are not counted against the speed limit                                             | `02-3`                                                                                             | P2       | small  | P2, 1 finding, small                                                                                             | none                                          | open   |
| 88  | A stale resume file that could not be deleted makes zeros count as downloaded                     | `Xsecond-review-resume-5`                                                                          | P2       | small  | P2, 1 finding, small                                                                                             | none                                          | open   |
| 89  | A resume file is rewritten every second even when nothing changed                                 | `01-4`                                                                                             | P2       | small  | P2, 1 finding, small                                                                                             | none                                          | open   |
| 90  | A valid short partial answer is waited out as a failed connection                                 | `F-2`                                                                                              | P2       | small  | P2, 1 finding, small                                                                                             | `FT-sizes`                                    | open   |
| 91  | CLI speed and ETA are wrong for half a minute after a resume                                      | `32-4`                                                                                             | P2       | small  | P2, 1 finding, small                                                                                             | none                                          | fixed  |
| 92  | A command-line argument that is not valid Unicode aborts the app                                  | `22-4`                                                                                             | P2       | small  | P2, 1 finding, small                                                                                             | none                                          | fixed  |
| 93  | Uninstalling while the app runs leaves its program behind                                         | `29-1`                                                                                             | P2       | small  | P2, 1 finding, small                                                                                             | none                                          | open   |
| 94  | The AppImage's library folder leaks into programs the app starts                                  | `29-2`                                                                                             | P2       | small  | P2, 1 finding, small                                                                                             | none                                          | open   |
| 95  | A video's title is cut at its last slash                                                          | `16-2`                                                                                             | P2       | small  | P2, 1 finding, small                                                                                             | `B07-p2-tests`                                | fixed  |
| 96  | yt-dlp progress goes backwards for a video with separate audio                                    | `16-3`                                                                                             | P2       | small  | P2, 1 finding, small                                                                                             | none                                          | open   |
| 97  | A quality says 60 fps but may download 30 fps                                                     | `16-4`                                                                                             | P2       | small  | P2, 1 finding, small                                                                                             | `B07-p2-tests`                                | open   |
| 98  | Speed limits do not reach yt-dlp                                                                  | `17-4`                                                                                             | P2       | small  | P2, 1 finding, small                                                                                             | none                                          | open   |
| 99  | The best audio is picked by rate alone, so a dubbed track can replace the original                | `B07-2`                                                                                            | P2       | small  | P2, 1 finding, small                                                                                             | `B07-p2-tests`                                | open   |
| 100 | HLS key addresses count against no memory budget                                                  | `14-3`                                                                                             | P2       | small  | P2, 1 finding, small                                                                                             | none                                          | open   |
| 101 | Automatic audio picks another language when the default is inside the video                       | `14-4`                                                                                             | P2       | small  | P2, 1 finding, small                                                                                             | `14-4-and-14-5`                               | open   |
| 102 | A subtitle codec turns an MP4-ready video into MKV                                                | `14-5`                                                                                             | P2       | small  | P2, 1 finding, small                                                                                             | `14-4-and-14-5`                               | open   |
| 103 | While the list loads, the window says nothing was ever downloaded                                 | `25-9`                                                                                             | P2       | small  | P2, 1 finding, small                                                                                             | none                                          | open   |
| 104 | Moving a file to another drive drops its Mark of the Web                                          | `05-4`                                                                                             | P2       | small  | P2, 1 finding, small                                                                                             | `05-4`                                        | fixed  |
| 105 | Replacing a file another program has open blames the folder                                       | `28-2`                                                                                             | P2       | small  | P2, 1 finding, small                                                                                             | `28-2`                                        | open   |
| 106 | A stream manifest over the limit offers a retry that cannot help                                  | `28-4`                                                                                             | P2       | small  | P2, 1 finding, small                                                                                             | none                                          | open   |
| 107 | A suffix list that could not be saved blames the internet connection                              | `10-3`                                                                                             | P2       | small  | P2, 1 finding, small                                                                                             | none                                          | open   |
| 108 | An add-on repair downloads everything and puts yt-dlp back to the pinned version                  | `11-1`                                                                                             | P2       | small  | P2, small; the next item's damaged-record route needs it                                                         | none                                          | fixed  |
| 109 | The add-on check acts on what it read before its wait, so a removed add-on comes back             | `11-2`                                                                                             | P2       | small  | P2, 1 finding, small; after item 108                                                                             | none                                          | fixed  |
| 110 | One bad yt-dlp release stops the check, and a failed check forgets its offers                     | `Xsecurity-addon-unpacking-3`                                                                      | P2       | small  | P2, 1 finding, small                                                                                             | none                                          | fixed  |
| 111 | Cancel on a selection stops at a finished download                                                | `25-3`                                                                                             | P2       | small  | P2, 1 finding, small                                                                                             | none                                          | open   |
| 112 | The drawer stays open over a page opened from its foot                                            | `25-7`                                                                                             | P2       | small  | P2, 1 finding, small                                                                                             | none                                          | open   |
| 113 | Change queue inside the details opens the same details again                                      | `25-8`                                                                                             | P2       | small  | P2, 1 finding, small                                                                                             | none                                          | open   |
| 114 | A page address that is not a web address counts as on the person's network                        | `10-1`                                                                                             | P2       | small  | P2, 1 finding, small                                                                                             | none                                          | open   |
| 115 | A Windows socks= proxy fails on sites whose first address is IPv6                                 | `09-2`                                                                                             | P2       | small  | P2, 1 finding, small                                                                                             | none                                          | fixed  |
| 116 | The proxy form says a saved password is kept, then forgets it                                     | `27-1`                                                                                             | P2       | small  | P2, 1 finding, small                                                                                             | none                                          | open   |
| 117 | A download that fails to start leaves its slot empty                                              | `04-2`                                                                                             | P2       | small  | P2, 1 finding, small                                                                                             | none                                          | fixed  |
| 118 | A closing queue's slots are given away at once, exceeding the limit                               | `18-4`                                                                                             | P2       | small  | P2, 1 finding, small                                                                                             | none                                          | fixed  |
| 119 | Regular expressions differing only in case count as one rule                                      | `18-5`                                                                                             | P2       | small  | P2, 1 finding, small                                                                                             | none                                          | fixed  |
| 120 | Enter in the delete-queue field closes the question and deletes nothing                           | `27-3`                                                                                             | P2       | small  | P2, 1 finding, small                                                                                             | none                                          | open   |
| 121 | Saving a queue changes its speed limit                                                            | `27-5`                                                                                             | P2       | small  | P2, 1 finding, small                                                                                             | none                                          | open   |
| 122 | Confirming a file unedited stops the server from naming it                                        | `24-5`                                                                                             | P2       | small  | P2, 1 finding, small                                                                                             | none                                          | open   |
| 123 | A failure notice in the browser loses its buttons after 32 later notices                          | `26-1`                                                                                             | P2       | small  | P2, 1 finding, small                                                                                             | none                                          | open   |
| 124 | A tray action that fails tells the person nothing                                                 | `26-4`                                                                                             | P2       | small  | P2, 1 finding, small                                                                                             | none                                          | open   |
| 125 | An unfinished download reads 100%                                                                 | `23-3`                                                                                             | P2       | small  | P2, 1 finding, small                                                                                             | none                                          | open   |
| 126 | A finished download whose file is gone gets advice it cannot follow                               | `23-8`                                                                                             | P2       | small  | P2, 1 finding, small                                                                                             | none                                          | open   |
| 127 | Settings, Media shows an empty Resolution after a rarer quality                                   | `27-6`                                                                                             | P2       | small  | P2, 1 finding, small                                                                                             | none                                          | open   |
| 128 | The Add dialog says By file type for downloads going to a queue's folder                          | `Xpath-normal-download-7`                                                                          | P2       | small  | P2, 1 finding, small                                                                                             | none                                          | open   |
| 129 | scripts/linux-test.sh can never pass                                                              | `33-1`                                                                                             | P2       | small  | P2, 1 finding, small                                                                                             | none                                          | open   |
| 130 | A second running copy of one CLI download takes over the first one's files                        | `01-2`                                                                                             | P2       | medium | P2, 1 finding, medium                                                                                            | none                                          | fixed  |
| 131 | A launch handed over while the list is opening pops a box and loses links                         | `22-3`                                                                                             | P2       | medium | P2, 1 finding, medium                                                                                            | none                                          | fixed  |
| 132 | A typed extension picks a container ffmpeg refuses, with no way out                               | `Xpath-media-download-7`                                                                           | P2       | medium | P2, 1 finding, medium                                                                                            | none                                          | open   |
| 133 | A stream whose placing fails is downloaded again                                                  | `Xpath-media-download-11`                                                                          | P2       | medium | P2, 1 finding, medium                                                                                            | none                                          | open   |
| 134 | A saved format the page no longer lists can never succeed                                         | `Xpath-media-download-9`                                                                           | P2       | medium | P2, 1 finding, medium                                                                                            | none                                          | open   |
| 135 | Try again can never succeed after the list failed to open                                         | `Xdatabase-migrations-2`                                                                           | P2       | medium | P2, 1 finding, medium                                                                                            | none                                          | open   |
| 136 | Dragging moves rows the selection no longer shows                                                 | `25-4`                                                                                             | P2       | medium | P2, 1 finding, medium                                                                                            | none                                          | open   |
| 137 | Reading a website's page with yt-dlp escapes the local network rule                               | `Xsecurity-bridge-and-pipe-input-2`                                                                | P2       | medium | P2, 1 finding, medium                                                                                            | none                                          | open   |
| 138 | A queue's run ends while its last download only waits                                             | `18-1`                                                                                             | P2       | medium | P2, 1 finding, medium; after item 79                                                                             | none                                          | open   |
| 139 | Nothing opens a download's window again after Hide                                                | `24-8`                                                                                             | P2       | medium | P2, 1 finding, medium                                                                                            | none                                          | open   |
| 140 | A file too large for the disk is not noticed until it is full                                     | `01-3`                                                                                             | P2       | medium | P2, 1 finding, medium                                                                                            | none                                          | open   |
| 141 | A crash at the end of a move leaves the list and the disk disagreeing                             | `Xcrash-and-resume-4`                                                                              | P2       | medium | P2, 1 finding, medium; after item 8                                                                              | none                                          | open   |
| 142 | Commands and items left from the removed web interface are dead                                   | `23-11` `24-11` `30-5` `30-6` `B19-5`                                                              | P3       | medium | P3, 5 findings, medium                                                                                           | none                                          | open   |
| 143 | Tests give waits a fixed short limit, and the timing check cannot see them                        | `01-10` `33-2` `04-7` `06-3` `21-3`                                                                | P3       | medium | P3, 5 findings, medium                                                                                           | none                                          | open   |
| 144 | Tests race the machine against a real-clock limit of the code under test                          | `03-7` `10-5` `13-4` `B19-1` `B19-2`                                                               | P3       | medium | P3, 5 findings, medium                                                                                           | none                                          | open   |
| 145 | A page's HLS part runs outside the run's file work                                                | `17-6` `Xpath-media-download-8` `Xsecond-review-disk-writes-9` `Xsecond-review-resume-6`           | P3       | small  | P3, 4 findings, small                                                                                            | none                                          | open   |
| 146 | Bridge and test values are written separately in several crates                                   | `06-4` `22-9` `B04-3` `B04-4`                                                                      | P3       | small  | P3, 4 findings, small                                                                                            | none                                          | open   |
| 147 | Test servers repeat their range parsing and behave unlike their docs                              | `31-1` `31-2` `31-3` `31-4`                                                                        | P3       | small  | P3, 4 findings, small                                                                                            | none                                          | open   |
| 148 | An import's summary event and the every-video answer have no reader                               | `27-11` `30-4` `Xfeature-behaviour-7`                                                              | P3       | small  | P3, 3 findings, small                                                                                            | none                                          | open   |
| 149 | Tests repeat their fixtures and data generators                                                   | `02-8` `07-7` `17-9`                                                                               | P3       | medium | P3, 3 findings, medium                                                                                           | none                                          | open   |
| 150 | A reload after missed events repeats notices and leaves open windows stale                        | `23-9` `Xdatabase-migrations-3` `24-9`                                                             | P3       | medium | P3, 3 findings, medium                                                                                           | none                                          | open   |
| 151 | A capture's check and its record sit in separate lock sections                                    | `07-5` `Xconcurrency-6`                                                                            | P3       | small  | P3, 2 findings, small                                                                                            | `Xconcurrency-3-and-6`                        | fixed  |
| 152 | A found duplicate overrides a stop the person chose in the same moment                            | `04-3` `05-12`                                                                                     | P3       | small  | P3, 2 findings, small                                                                                            | none                                          | fixed  |
| 153 | A link or folder in place of the key gets a raw system error                                      | `08-4` `B04-1`                                                                                     | P3       | small  | P3, 2 findings, small                                                                                            | `B04-p2-checks`                               | open   |
| 154 | Cancelling does not interrupt name lookups                                                        | `09-4` `B14-2`                                                                                     | P3       | small  | P3, 2 findings, small                                                                                            | none                                          | open   |
| 155 | The rebinding tests never reach the filtering resolver, and its refusal ends as a network failure | `09-5` `09-6`                                                                                      | P3       | small  | P3, 2 findings, small                                                                                            | none                                          | open   |
| 156 | Tests do not cover the stored forms of enums and old settings                                     | `03-6` `Xdatabase-migrations-5`                                                                    | P3       | small  | P3, 2 findings, small                                                                                            | none                                          | open   |
| 157 | A held-open output loses a program's collected output, and its test cannot fail                   | `13-2` `13-3`                                                                                      | P3       | small  | P3, 2 findings, small                                                                                            | none                                          | open   |
| 158 | A resume file's temporary file can stay for good                                                  | `01-8` `Xsecond-review-disk-writes-8`                                                              | P3       | small  | P3, 2 findings, small                                                                                            | none                                          | open   |
| 159 | Add-on release files of skipped versions stay in the work folder                                  | `11-4` `Xsecurity-addon-unpacking-4`                                                               | P3       | small  | P3, 2 findings, small                                                                                            | none                                          | fixed  |
| 160 | Tray ticks flip on a repeat click, and & in a name shows wrongly                                  | `26-2` `26-3`                                                                                      | P3       | small  | P3, 2 findings, small                                                                                            | none                                          | open   |
| 161 | Messages name the product with a literal                                                          | `26-9` `B17-1`                                                                                     | P3       | small  | P3, 2 findings, small                                                                                            | none                                          | open   |
| 162 | A navigation item left behind keeps its hover colour until the mouse moves                        | `U-1` `B18-2`                                                                                      | P3       | small  | P3, 2 findings, small                                                                                            | none                                          | open   |
| 163 | Empty pages offer Pause all or Resume all                                                         | `U-3` `B13-2`                                                                                      | P3       | small  | P3, 2 findings, small                                                                                            | none                                          | open   |
| 164 | A failed video or conversion shows the program's raw output                                       | `U-9` `B18-3`                                                                                      | P3       | small  | P3, 2 findings, small                                                                                            | none                                          | open   |
| 165 | New queue and New proxy do not put the cursor in the Name field                                   | `U-13` `B16-1`                                                                                     | P3       | small  | P3, 2 findings, small                                                                                            | none                                          | open   |
| 166 | The row, the menu and the details decide offers and words on their own                            | `25-11` `U-10`                                                                                     | P3       | small  | P3, 2 findings, small                                                                                            | none                                          | open   |
| 167 | Two test files depend on an order only timing decides                                             | `05-14` `21-3`                                                                                     | P3       | small  | P3, 2 findings, small                                                                                            | none                                          | open   |
| 168 | The same small pieces of logic are written many times in the forms                                | `27-12` `B19-3`                                                                                    | P3       | medium | P3, 2 findings, medium                                                                                           | none                                          | open   |
| 169 | Dying between placing a video and deleting its work folder gives a second copy                    | `Xcrash-and-resume-3` `Xsecond-review-resume-8`                                                    | P3       | medium | P3, 2 findings, medium; after item 4                                                                             | none                                          | open   |
| 170 | Exporting settings over an earlier file is not written whole                                      | `19-6` `Xrepeated-logic-6`                                                                         | P3       | small  | P3, 2 findings, small                                                                                            | none                                          | open   |
| 171 | The bridge's manifest write has no flush                                                          | `Xrepeated-logic-6`                                                                                | P3       | small  | P3, 1 finding, small                                                                                             | none                                          | open   |
| 172 | Pause all while a cancel is stopping keeps the download                                           | `Xconcurrency-7`                                                                                   | P3       | small  | P3, 1 finding, small                                                                                             | none                                          | fixed  |
| 173 | The same link with another #fragment opens a second question                                      | `07-3`                                                                                             | P3       | small  | P3, 1 finding, small                                                                                             | none                                          | fixed  |
| 174 | A file beside a leftover part or resume file is never asked about                                 | `B02-1`                                                                                            | P3       | small  | P3, 1 finding, small; after item 48                                                                              | none                                          | open   |
| 175 | A saved download that gives way to its page loses its link if the app stops first                 | `Xcrash-and-resume-6`                                                                              | P3       | small  | P3, 1 finding, small                                                                                             | none                                          | fixed  |
| 176 | A removal gives up the path before it deletes the files                                           | `Xconcurrency-10`                                                                                  | P3       | small  | P3, 1 finding, small                                                                                             | none                                          | fixed  |
| 177 | A failed write takes too much back from the progress count                                        | `01-5`                                                                                             | P3       | small  | P3, 1 finding, small                                                                                             | none                                          | open   |
| 178 | A range past the end of a shrunken file is reported as a server error                             | `02-4`                                                                                             | P3       | small  | P3, 1 finding, small                                                                                             | none                                          | open   |
| 179 | The step-back rule's long-wait test is true only because of jitter                                | `02-5`                                                                                             | P3       | small  | P3, 1 finding, small                                                                                             | none                                          | fixed  |
| 180 | Fetcher::save promises data survives a drop, but up to 256 KiB is lost                            | `02-6`                                                                                             | P3       | small  | P3, 1 finding, small                                                                                             | none                                          | open   |
| 181 | bench_local sleeps for no time after each accept                                                  | `02-7`                                                                                             | P3       | small  | P3, 1 finding, small                                                                                             | none                                          | fixed  |
| 182 | A redirect error tells the CLI user to allow something the CLI cannot                             | `32-5`                                                                                             | P3       | small  | P3, 1 finding, small                                                                                             | none                                          | fixed  |
| 183 | async_and_blocking_reads_agree never does a blocking read                                         | `06-2`                                                                                             | P3       | small  | P3, 1 finding, small                                                                                             | none                                          | open   |
| 184 | The AppImage replacement is renamed into place without a flush                                    | `12-4`                                                                                             | P3       | small  | P3, 1 finding, small                                                                                             | none                                          | fixed  |
| 185 | The unsigned release page address reaches the system shell unchecked                              | `Xsecurity-update-signatures-4`                                                                    | P3       | small  | P3, 1 finding, small                                                                                             | none                                          | open   |
| 186 | Turning off pre-release versions keeps offering the one already found                             | `Xpath-app-update-7`                                                                               | P3       | small  | P3, 1 finding, small                                                                                             | none                                          | open   |
| 187 | No test holds a manifest's version and size rules                                                 | `12-6`                                                                                             | P3       | small  | P3, 1 finding, small                                                                                             | none                                          | open   |
| 188 | The packaging example for ARM64 on x64 cannot work                                                | `29-3`                                                                                             | P3       | small  | P3, 1 finding, small                                                                                             | none                                          | open   |
| 189 | ARM64 Linux packages are drafted without the fresh-system check                                   | `29-4`                                                                                             | P3       | small  | P3, 1 finding, small                                                                                             | none                                          | open   |
| 190 | The .deb and .rpm state no C library version                                                      | `29-5`                                                                                             | P3       | small  | P3, 1 finding, small                                                                                             | none                                          | open   |
| 191 | The icon holds its own unchecked copy of the mark                                                 | `29-6`                                                                                             | P3       | small  | P3, 1 finding, small                                                                                             | none                                          | open   |
| 192 | Two copies of the read-until-full loop have drifted                                               | `15-4`                                                                                             | P3       | small  | P3, 1 finding, small                                                                                             | none                                          | open   |
| 193 | Code keeps work folders of a layout that was never released                                       | `15-5`                                                                                             | P3       | small  | P3, 1 finding, small                                                                                             | none                                          | open   |
| 194 | axum is a dev-dependency nothing in streams uses                                                  | `15-6`                                                                                             | P3       | small  | P3, 1 finding, small                                                                                             | none                                          | open   |
| 195 | The path-escape half of a media test cannot fail                                                  | `17-8`                                                                                             | P3       | small  | P3, 1 finding, small                                                                                             | none                                          | open   |
| 196 | Doc comments on media runs sit on the wrong items                                                 | `17-10`                                                                                            | P3       | small  | P3, 1 finding, small                                                                                             | none                                          | open   |
| 197 | Sizes and bit rates a page chooses are added without a check                                      | `16-5`                                                                                             | P3       | small  | P3, 1 finding, small                                                                                             | `B07-p2-tests`                                | open   |
| 198 | The key error prints the whole signed address                                                     | `14-6`                                                                                             | P3       | small  | P3, 1 finding, small                                                                                             | none                                          | open   |
| 199 | On Linux, media programs may outlive an app ended by a signal                                     | `Xshutdown-4`                                                                                      | P3       | medium | P3, 1 finding, medium                                                                                            | none                                          | open   |
| 200 | Placing a finished video has its own weaker no-replace move                                       | `Xrepeated-logic-4`                                                                                | P3       | small  | P3, 1 finding, small                                                                                             | none                                          | fixed  |
| 201 | Moving a finished video leaves its subtitle files behind                                          | `B10-1`                                                                                            | P3       | small  | P3, 1 finding, small                                                                                             | none                                          | open   |
| 202 | A failed browser registration logs no reason                                                      | `22-5`                                                                                             | P3       | small  | P3, 1 finding, small                                                                                             | none                                          | fixed  |
| 203 | The autostart Exec line escapes reserved characters once                                          | `22-6`                                                                                             | P3       | small  | P3, 1 finding, small                                                                                             | none                                          | fixed  |
| 204 | The autostart Launcher trait has one implementation and no test                                   | `22-8`                                                                                             | P3       | small  | P3, 1 finding, small                                                                                             | none                                          | fixed  |
| 205 | Every window has an empty native title                                                            | `U-2`                                                                                              | P3       | small  | P3, 1 finding, small                                                                                             | none                                          | fixed  |
| 206 | invalid_link ignores its argument, and its doc and test claim otherwise                           | `28-5`                                                                                             | P3       | small  | P3, 1 finding, small                                                                                             | none                                          | open   |
| 207 | An import can leave two queues with the same name                                                 | `19-2`                                                                                             | P3       | small  | P3, 1 finding, small                                                                                             | `19-2`                                        | open   |
| 208 | An update offer names a yt-dlp older than the one installed                                       | `11-3`                                                                                             | P3       | small  | P3, 1 finding, small                                                                                             | none                                          | fixed  |
| 209 | swap_in spells out the release file's name                                                        | `11-5`                                                                                             | P3       | small  | P3, 1 finding, small                                                                                             | none                                          | fixed  |
| 210 | Ticking the first row pushes the list down under the pointer                                      | `U-4`                                                                                              | P3       | small  | P3, 1 finding, small                                                                                             | none                                          | open   |
| 211 | The empty queue's Add button opens the dialog past the window's record                            | `25-10`                                                                                            | P3       | small  | P3, 1 finding, small                                                                                             | none                                          | open   |
| 212 | `let _theme = cx.theme()` does nothing                                                            | `25-12`                                                                                            | P3       | small  | P3, 1 finding, small                                                                                             | none                                          | open   |
| 213 | The link's hover popover covers the row's menu                                                    | `U-8`                                                                                              | P3       | small  | P3, 1 finding, small                                                                                             | none                                          | open   |
| 214 | The damaged-list notice squeezes its text and breaks the path                                     | `U-15`                                                                                             | P3       | small  | P3, 1 finding, small                                                                                             | none                                          | open   |
| 215 | A top-level domain held only through a wildcard counts as a local name                            | `10-2`                                                                                             | P3       | small  | P3, 1 finding, small                                                                                             | none                                          | open   |
| 216 | direct_uses_no_proxy passes whatever Direct does                                                  | `09-7`                                                                                             | P3       | small  | P3, 1 finding, small                                                                                             | none                                          | open   |
| 217 | Module docs describe proxy behaviour that has changed                                             | `09-9`                                                                                             | P3       | small  | P3, 1 finding, small                                                                                             | none                                          | open   |
| 218 | A proxy test cancelled by quitting is saved as its last result                                    | `19-7`                                                                                             | P3       | small  | P3, 1 finding, small                                                                                             | none                                          | fixed  |
| 219 | A test named for keeping the last result does not check it                                        | `19-9`                                                                                             | P3       | small  | P3, 1 finding, small                                                                                             | none                                          | fixed  |
| 220 | Copy from system turns a SOCKS4 server into a SOCKS5 profile                                      | `B15-2`                                                                                            | P3       | small  | P3, 1 finding, small                                                                                             | none                                          | fixed  |
| 221 | An impossible port is saved as the usual port without a word                                      | `27-9`                                                                                             | P3       | small  | P3, 1 finding, small                                                                                             | none                                          | open   |
| 222 | A file name or host holding a secret word loses the rest of its log line                          | `20-2`                                                                                             | P3       | small  | P3, 1 finding, small                                                                                             | none                                          | fixed  |
| 223 | The rules help says the highest queue wins, but for sites the most specific does                  | `27-10`                                                                                            | P3       | small  | P3, 1 finding, small                                                                                             | none                                          | fixed  |
| 224 | A dotted file type is accepted in a category rule and never matches                               | `Xrepeated-logic-5`                                                                                | P3       | small  | P3, 1 finding, small                                                                                             | none                                          | fixed  |
| 225 | Queue page numbers start at 2 and number failed rows                                              | `U-5`                                                                                              | P3       | small  | P3, 1 finding, small                                                                                             | none                                          | open   |
| 226 | A queue using a proxy with no name reads "via" and nothing after it                               | `U-11`                                                                                             | P3       | small  | P3, 1 finding, small                                                                                             | none                                          | open   |
| 227 | The queue page's facts line is cut off with no way to read the end                                | `U-12`                                                                                             | P3       | small  | P3, 1 finding, small                                                                                             | none                                          | open   |
| 228 | The details panel says Merging for every processing step                                          | `23-4`                                                                                             | P3       | small  | P3, 1 finding, small                                                                                             | none                                          | open   |
| 229 | The system accent breaks the promised contrast                                                    | `23-7`                                                                                             | P3       | small  | P3, 1 finding, small                                                                                             | none                                          | open   |
| 230 | Check for updates and Copy from system give no answer when there is nothing to do                 | `27-7`                                                                                             | P3       | small  | P3, 1 finding, small                                                                                             | none                                          | open   |
| 231 | A download waiting on the question reads Paused at Size unknown                                   | `U-6`                                                                                              | P3       | small  | P3, 1 finding, small                                                                                             | none                                          | open   |
| 232 | The complete window stays open and blank once its rows are removed                                | `24-10`                                                                                            | P3       | small  | P3, 1 finding, small                                                                                             | none                                          | open   |
| 233 | The notice of a failed save ignores Do Not Disturb                                                | `26-6`                                                                                             | P3       | small  | P3, 1 finding, small                                                                                             | none                                          | open   |
| 234 | The language list names Twi "Akan" and gives Ossetic itself as its other name                     | `30-7`                                                                                             | P3       | small  | P3, 1 finding, small                                                                                             | none                                          | open   |
| 235 | The comment on RecvError::Closed in the server's loop is wrong                                    | `07-6`                                                                                             | P3       | small  | P3, 1 finding, small                                                                                             | none                                          | open   |
| 236 | Manager::update_settings has no caller outside tests                                              | `19-10`                                                                                            | P3       | small  | P3, 1 finding, small                                                                                             | none                                          | open   |
| 237 | lock_path repeats with_suffix                                                                     | `03-9`                                                                                             | P3       | small  | P3, 1 finding, small                                                                                             | none                                          | open   |
| 238 | Product tests are pinned to today's config values                                                 | `33-3`                                                                                             | P3       | small  | P3, 1 finding, small                                                                                             | none                                          | open   |
| 239 | Two copies of the pnpm-workspace parser                                                           | `33-4`                                                                                             | P3       | small  | P3, 1 finding, small                                                                                             | none                                          | open   |
| 240 | Dead code in the product generator                                                                | `33-5`                                                                                             | P3       | small  | P3, 1 finding, small                                                                                             | none                                          | open   |
| 241 | Comments in the checked configuration describe things that are gone                               | `33-6`                                                                                             | P3       | small  | P3, 1 finding, small                                                                                             | none                                          | open   |
| 242 | Two test helpers wait for progress with no deadline                                               | `01-9`                                                                                             | P3       | small  | P3, 1 finding, small                                                                                             | none                                          | open   |
| 243 | A roaming data folder that could not be deleted keeps the list with its cookies                   | `Xdatabase-migrations-4`                                                                           | P3       | small  | P3, 1 finding, small                                                                                             | none                                          | fixed  |
| 244 | The `<local>` queue rule is never tested against the real network check                           | `M-4`                                                                                              | P3       | small  | P3, 1 finding, small                                                                                             | none                                          | open   |

## Items

### 1. Two downloads can end up writing the same file

P0, large, fixed.

`Claimed` is a set of paths with no owner: an open question holds no claim on the path it asks about, and `replace_at` inserts without looking. So two same-named links, a file already there and Replace on both put two downloads on one part file. The first row then reads Completed with none of its own bytes in the file, and the second fails with a misleading folder error. The same missing owner lets a finished row give up a claim another download holds, and leaves two finished rows on one path, where moving or deleting through one strands the other.

**Fix:** Give each claim an owner, so that a question reserves its path and a row can only release what it holds. Decide what two finished rows on one path should do. Start the regression test from `proofs/05-1.patch`: `proofs/Xconcurrency-1.patch` sleeps a fixed time, against ENGINEERING section 7. Mixed bytes in one file were not reproduced; the wrong result does not depend on them.

**Blocks:** item 11. **Blocked by:** nothing.

**Touches:** crates/core/src/manager/file_exists.rs, replace_at (called from answer_file_exists and on_found); crates/core/src/manager/file_exists.rs, replace_at, answer_file_exists, on_found; found_existing for the check that misses it; crates/core/src/manager/relocate.rs, finish_move; crates/core/src/manager/state.rs, remove_entry; crates/core/src/manager/downloads.rs, finish_removing.

**Findings:**

- `05-1`: confirmed, P0. Replace puts two downloads on one path, so they write the same part file. Full text in `chunks/05.md`, verdict in `verdicts/B01.md`.
- `Xconcurrency-1`: confirmed, P0. Two questions about one file, both answered Replace, run two downloads on one path. Full text in `cross/concurrency.md`, verdict in `verdicts/B01.md`.
- `05-13`: confirmed, P3. A finished row gives up a claim that another download holds. Full text in `chunks/05.md`, verdict in `verdicts/B01.md`.
- `Xconcurrency-9`: confirmed, P3. Moving a finished file can give up another download's claim on its old path. Full text in `cross/concurrency.md`, verdict in `verdicts/B01.md`.
- `B01-3`: confirmed, P3. Two finished rows on one path: moving or deleting through one leaves the other pointing at nothing. Full text in `verdicts/B01.md`, verdict in `verdicts/new-findings.md`.

### 2. Removing a finished download can delete a whole folder

P0, small, fixed.

`delete_files` asks only `path.is_dir()` and calls `remove_dir_all`, a branch meant for work folders. A finished row's path may by then be someone's folder, most easily after an extensionless file, so one "Remove and delete" removes the whole folder with no Recycle Bin. The doc comment above `worked_before` also belongs to this function.

**Fix:** Only `partial_paths` needs the folder branch, so a Completed path should not go through it. Move the misplaced doc line in the same change.

**Blocks:** nothing. **Blocked by:** nothing.

**Check to add:** A `disallowed-methods` entry in `clippy.toml` for `std::fs::remove_dir_all`, allowed only in the helper that removes work folders.

**Touches:** crates/core/src/manager/run.rs, delete_files; crates/core/src/manager/downloads.rs, finish_removing; crates/core/src/manager/run.rs, worked_before and delete_files.

**Findings:**

- `Xsecurity-path-handling-1`: confirmed, P0. Removing a finished download with its file deletes a whole folder now at that path. Full text in `cross/security-path-handling.md`, verdict in `verdicts/B02.md`.
- `04-8`: confirmed, P3. The doc comment of delete_files sits on another function. Full text in `chunks/04.md`, verdict in `verdicts/B02.md`.

### 3. A cut-off download of unknown size is reported as complete

P0, small, fixed.

A size-unknown HTTP answer is always unranged, so every retry is a whole request that skips what is already saved. A body that ends while bytes are still to skip counts as the file's real end. So a chunked download, a dropped connection, then a login or captive portal page sent as `200` ends Completed with the kept bytes and no error.

**Fix:** At the end of a body, treat leftover skip as a failed attempt. `worker.rs` `stream` and `Fetcher::attempt` are the two places, which also covers streams, playlists and FTP without `REST`.

**Blocks:** nothing. **Blocked by:** nothing.

**Touches:** crates/engine/src/worker.rs, stream; crates/engine/src/fetch.rs, Fetcher::attempt.

**Findings:**

- `Xsecond-review-disk-writes-2`: confirmed, P0. A download of unknown size is reported complete and truncated when a retry's answer is shorter than what it already has. Full text in `cross/second-review-disk-writes.md`, verdict in `verdicts/B03.md`.

### 4. A restart can mark a download finished on a file it never downloaded

P1, medium, fixed.

The saved row does not record that its own run placed a file, so `restore_entry` reads "settled, a file there, no partial files beside it" as finished. A stream added for later on a taken name, or a Replace still waiting for a slot, turns Completed at the next start with bytes it never fetched. It also holds for a Failed or Cancelled row retried after Replace, and "Remove and delete" on such a row then deletes the older file (`B02-2`).

**Fix:** Record in the saved row that this row's run wrote the file, and base the restore on that. The test `a_file_that_finished_before_the_restart_is_not_downloaded_again` writes 5 bytes for a 512 KiB download and expects Completed, so it has to change. The fix must still tell this state apart from item 48, a real finished file with a leftover resume file.

**Blocks:** item 48; item 83; item 169; shares the row record with the items that save more of a download's choices. **Blocked by:** nothing.

**Touches:** crates/core/src/manager/restore.rs, restore_entry; crates/core/src/naming.rs, finished_size; crates/core/src/manager/downloads.rs, insert and add_entry; crates/core/src/manager/file_exists.rs, replace_at; crates/core/src/manager/downloads.rs, insert; crates/core/src/manager/downloads.rs, finish_removing.

**Findings:**

- `04-1`: confirmed, P1. A waiting video whose name is already taken is marked finished at the next start. Full text in `chunks/04.md`, verdict in `verdicts/B02.md`.
- `Xcrash-and-resume-1`: confirmed, P1. A restart marks a download Completed with a file it never downloaded. Full text in `cross/crash-and-resume.md`, verdict in `verdicts/B02.md`.
- `Xsecond-review-resume-1`: confirmed, P1. After a restart, a download that never ran is marked finished with a file that is not its own. Full text in `cross/second-review-resume.md`, verdict in `verdicts/B02.md`.
- `B02-2`: confirmed, P2. "Remove and delete" on a falsely completed row deletes the older file. Full text in `verdicts/B02.md`, verdict in `verdicts/new-findings.md`.

### 5. A browser download that meets Ask is dropped and goes back to the browser

P1, small, fixed.

`ask` leaves `naming.settled` false, and `Entry::server_answered` reads that as "no answer yet". So a capture that meets Ask with no confirmation window is removed: the question opens and closes at once and the browser keeps its copy. It happens every time with `ask_before_download` off. Nothing is lost.

**Fix:** Let the capture's wait tell "the server answered and a question is open" from "the run stopped first". Start from `proofs/05-3.patch`, which waits for events. The stray window part of `Xpath-normal-download-2` was not checked and needs the desktop window code.

**Blocks:** item 11; item 11 sends many more captures down this path, so this comes first. **Blocked by:** nothing.

**Touches:** crates/core/src/manager/captures.rs, capture_arrived and await_answer; crates/core/src/manager/file_exists.rs, ask; crates/core/src/manager/state.rs, Entry::server_answered; crates/core/src/manager/captures.rs, await_answer and capture_arrived; crates/core/src/manager/run.rs, finish.

**Findings:**

- `05-3`: confirmed, P1. A captured download that meets Ask is removed, and the browser saves its own copy. Full text in `chunks/05.md`, verdict in `verdicts/B01.md`.
- `07-1`: confirmed, P1. With no window, a capture of a file already in the folder closes its own question and goes back to the browser. Full text in `chunks/07.md`, verdict in `verdicts/B01.md`.
- `Xconcurrency-3`: confirmed, P1. A browser capture of a file already in the folder opens its question and closes it at once. Full text in `cross/concurrency.md`, verdict in `verdicts/B01.md`.
- `Xpath-normal-download-2`: confirmed, P1. A browser download that meets a file already in the folder under Ask is torn down instead of asked about. Full text in `cross/path-normal-download.md`, verdict in `verdicts/B01.md`.

### 6. An inherited APPIMAGE variable can make the app overwrite or start another program

P1, small, fixed.

`install_kind`, `replace_appimage`, `autostart::program` and the bridge registration trust the `APPIMAGE` variable alone. A system package started by another AppImage inherits it, so an update can overwrite that program, and sign-in, the bridge launcher and a restart during quit can start it instead of Wirecatch. The bridge and sign-in halves are live today; the update half waits on the next item.

**Fix:** Check once, in `crates/shell/src/appimage.rs`, that this process really runs from that file, by comparing with `current_exe` or `APPDIR`. The `replace_appimage` half of the proof is Unix only and has not been run.

**Blocks:** item 7. **Blocked by:** nothing.

**Touches:** crates/shell/src/updates.rs, install_kind and replace_appimage; crates/shell/src/updates.rs install_kind, replace_appimage; same cause in crates/shell/src/autostart.rs program and crates/shell/src/bridge.rs registration; crates/desktop/src/lifecycle.rs, start_again; crates/shell/src/autostart.rs, program.

**Findings:**

- `12-1`: confirmed, P1. An APPIMAGE variable inherited from another program makes the update overwrite that program. Full text in `chunks/12.md`, verdict in `verdicts/B05.md`.
- `Xpath-app-update-4`: confirmed, P1. An inherited APPIMAGE variable makes the update overwrite another program. Full text in `cross/path-app-update.md`, verdict in `verdicts/B05.md`.
- `Xsecurity-update-signatures-2`: confirmed, P1. A stray APPIMAGE variable makes the update overwrite another program. Full text in `cross/security-update-signatures.md`, verdict in `verdicts/B05.md`.
- `B05-1`: confirmed, P2. A launch during a quit can start the program named by an inherited APPIMAGE. Full text in `verdicts/B05.md`, verdict in `verdicts/new-findings.md`.

### 7. The app never looks for updates, and every proxy test fails

P1, small, fixed.

`backend::config` calls only `with_official_suffix_list`, and `ManagerConfig::new` leaves the others empty so tests stay offline. So no installed copy looks for updates, and it says "You have the newest version"; only the placeholder key hides that today. Every proxy test reports Unreachable with no request, in every build today.

**Fix:** Name the official sources in `config`, once the previous item is in. Items 72, 73, 59 and 60 become reachable by real people from here on.

**Blocks:** nothing. **Blocked by:** item 6.

**Check to add:** A test on `config()` that every official source is set. `Xfeature-behaviour-S1` asks for the same.

**Touches:** crates/desktop/src/backend.rs, config; crates/core/src/manager/mod.rs, ManagerConfig::new, with_official_app_releases; crates/core/src/manager/app_update.rs, check_app_update; crates/core/src/manager/mod.rs, ManagerConfig::with_official_app_releases; crates/desktop/src/backend.rs config; crates/core/src/manager/mod.rs with_official_app_releases; crates/core/src/manager/app_update.rs check_app_update, and the other places its findings name.

**Findings:**

- `Xfeature-behaviour-1`: confirmed, P1. The desktop app never looks for updates and always says it has the newest version. Full text in `cross/feature-behaviour.md`, verdict in `verdicts/B05.md`.
- `Xpath-app-update-1`: confirmed, P1. The app never looks for updates: no build sets where its releases are. Full text in `cross/path-app-update.md`, verdict in `verdicts/B05.md`.
- `Xsecurity-update-signatures-1`: confirmed, P1. The shipped app is never told where its releases are, so it never finds an update. Full text in `cross/security-update-signatures.md`, verdict in `verdicts/B05.md`.
- `Xfeature-behaviour-2`: confirmed, P1. Every proxy connection test reports the proxy as unreachable. Full text in `cross/feature-behaviour.md`, verdict in `verdicts/B05.md`.

### 8. A move across drives can leave a cut-off file under the final name

P2, medium, fixed.

`move_file` opens the final name and copies straight into it, and `discard` runs only in a process that lives. A kill mid-copy leaves a cut-off file under the real name, which `unique_path` then skips and Keep existing accepts. The original stays on disk, so nothing is lost.

**Fix:** Copy to a temporary name and rename at the end, using the engine's no-replace rename. This fixes paths a and b of `Xcrash-and-resume-4`; paths c and d are their own item.

**Blocks:** item 9; item 10; item 141. **Blocked by:** nothing.

**Touches:** crates/core/src/manager/relocate.rs, move_file; crates/core/src/manager/relocate.rs, move_file, move_files and finish_move; crates/core/src/manager/file_exists.rs, take_next_free_name.

**Findings:**

- `Xsecond-review-disk-writes-7`: confirmed, P2. A move across drives writes straight to the final name, so a kill leaves a cut-off file that looks finished. Full text in `cross/second-review-disk-writes.md`, verdict in `verdicts/B10.md`.
- `Xcrash-and-resume-4` (paths a and b): confirmed, P2. A crash during a move to another folder leaves a cut-off file under the final name, or strands the data. Full text in `cross/crash-and-resume.md`, verdict in `verdicts/B10.md`.

### 9. Quitting during a move leaves the list pointing at the wrong place

P1, medium, open.

`shutdown` collects only `run.exited` and does not wait for `Hold::Moving`, and `finish_move`'s save reaches a closed store. The reopened row names a path with no file (finished) or the old folder with its part file in the new one (partial). The file still exists.

**Fix:** Make `shutdown` wait for moves. This needs a decision on what the quit limit does to a long copy, which is why the staged copy comes first.

**Blocks:** item 10. **Blocked by:** item 8.

**Touches:** crates/core/src/manager/mod.rs, Manager::shutdown; crates/core/src/manager/relocate.rs, finish_moving, move_files, finish_move; crates/core/src/manager/mod.rs, shutdown; crates/core/src/manager/relocate.rs, finish_moving, move_file.

**Findings:**

- `Xshutdown-1`: confirmed, P1. Quitting during a move to another folder leaves the list pointing at the old path. Full text in `cross/shutdown.md`, verdict in `verdicts/B10.md`.
- `Xconcurrency-4`: confirmed, P1. A quit during a move to another folder neither waits for it nor records it. Full text in `cross/concurrency.md`, verdict in `verdicts/B10.md`.

### 10. The quit time limit does not end the process

P1, medium, open.

`quit_core` runs with `|| ()`, and dropping the runtime waits for blocking work with no limit. One stuck file task keeps a process with no window alive, proven at 30 s in the real app, and it holds the database lock because `store.close` is skipped. A launch during such a quit starts a copy that cannot open the list, and its links are lost. `quit_core` and `quit_sequence` also take an `Option` and an `exit` that nothing uses, which is what hides this.

**Fix:** End the process when the limit runs out, and give the unused `exit` a real use or drop it. A task stuck inside a run's `FileWork` was not run, so the 15 s limit and the skipped close are read from the code.

**Blocks:** nothing. **Blocked by:** item 8, item 9.

**Touches:** crates/shell/src/quit.rs, quit_sequence; crates/desktop/src/backend.rs, Backend (owns tokio::runtime::Runtime as a GPUI global); crates/desktop/src/lifecycle.rs, quit; crates/shell/src/quit.rs (quit_sequence, QUIT_PATIENCE), crates/desktop/src/backend.rs (Backend::stop, the runtime field), crates/desktop/src/lifecycle.rs (quit); crates/desktop/src/lifecycle.rs, quit, start_again, and the other places its findings name.

**Findings:**

- `Xshutdown-3`: confirmed, P1. The quit time limit does not end the process: a stuck blocking task keeps it alive with no window or tray icon. Full text in `cross/shutdown.md`, verdict in `verdicts/B10.md`.
- `22-1`: confirmed, P1. A quit past its time limit does not exit: dropping the runtime waits for stuck file work. Full text in `chunks/22.md`, verdict in `verdicts/B10.md`.
- `B10-2`: confirmed, P2. A launch that arrives during a stuck quit starts a copy that cannot open the list. Full text in `verdicts/B10.md`, verdict in `verdicts/new-findings.md`.
- `22-7`: confirmed, P3. quit_core and quit_sequence take an Option and an exit that nothing uses. Full text in `chunks/22.md`, verdict in `verdicts/B10.md`.

### 11. "If the file is already in the folder" is skipped for names from the link or browser

P1, medium, open.

`insert` picks the provisional name with `unique_path`, which skips names on disk, and `settled_name` returns it when the server names nothing new. So the check only ever sees `report (1).pdf`: under Ask no question comes, under Replace the old file stays. The setting does nothing for a name taken from the link or the browser, the common case. The ` (1)` also follows the file into another folder where the name is free (`B01-1`).

**Fix:** Judge the name the file would have had, not the free one. Moves are not part of this: `B01-2` was disproved, so making moves follow the setting would be a new decision. After this lands the question appears far more often, which raises the value of items 54 and 80.

**Blocks:** nothing. **Blocked by:** item 1, item 5.

**Touches:** crates/core/src/naming.rs, settled_name; with crates/core/src/manager/downloads.rs, insert (naming::unique_path) and crates/core/src/manager/settle.rs, settle_path_checking; crates/core/src/manager/downloads.rs, Manager::insert; crates/core/src/manager/settle.rs, settle_path_checking.

**Findings:**

- `05-2`: confirmed, P1. "If the file is already in the folder" is skipped when the server gives no name of its own. Full text in `chunks/05.md`, verdict in `verdicts/B01.md`.
- `Xpath-normal-download-1`: confirmed, P1. "If the file is already in the folder" is skipped when the name comes from the link or the browser. Full text in `cross/path-normal-download.md`, verdict in `verdicts/B01.md`.
- `B01-1`: confirmed, P3. The (1) of a provisional name follows the download into a folder where the name is free. Full text in `verdicts/B01.md`, verdict in `verdicts/new-findings.md`.

### 12. A settings file can send downloads into folders whose programs the app runs

P1, medium, fixed.

On Windows `protected_folders` does not know the app's data folder, and `is_outside_user_folders` is false inside `{Home}`, so a settings file can point a download folder at the add-on's ffmpeg folder with no mark in the preview. A site can then land a DLL beside `ffmpeg.exe`, and the loader was shown to prefer a file in that folder. The same rule lets in `.ssh`, `AppData\Local\Programs` and `WindowsPowerShell` (`B12-1`).

**Fix:** Protect the app's data folder and the other run or trusted folders on Windows, as the hidden folder rule does on Linux, and mark them in the import preview. A working planted library was not built, so code running is inferred from the loader choosing the file.

**Blocks:** nothing. **Blocked by:** nothing.

**Touches:** crates/settings/src/folders.rs, protected_folders (Windows) and KnownFolders::is_protected; crates/settings/src/settings_file.rs, localize_folders; crates/settings/src/folders.rs, protected_folders (Windows) and KnownFolders::resolve_local; crates/addons/src/lib.rs, tools; crates/settings/src/folders.rs, protected_folders (Windows) and is_protected.

**Findings:**

- `Xsecurity-path-handling-2`: confirmed, P1. On Windows a settings file can send downloads into a folder whose programs Wirecatch runs. Full text in `cross/security-path-handling.md`, verdict in `verdicts/B12.md`.
- `Xsecurity-addon-unpacking-1`: confirmed, P1. A settings file can send downloads into the add-on's folders, and nothing checks what lands there before the next run. Full text in `cross/security-addon-unpacking.md`, verdict in `verdicts/B12.md`.
- `B12-1`: confirmed, P2. The folder rule on Windows also lets a settings file into the app's data folder and other folders whose files get run or trusted. Full text in `verdicts/B12.md`, verdict in `verdicts/new-findings.md`.

### 13. Replace on a file with another extension downloads everything again and strands the old folder

P2, medium, fixed.

`replace_at` moves the row's path while the work folder is derived from that path. So when the finished file's extension differs from the plan (mp4 planned, mkv finished), Replace fetches the segments again and leaves `clip.wcmedia` on disk.

**Fix:** Keep the work folder tied to the run, not to whatever the row's path has become.

**Blocks:** item 14. **Blocked by:** nothing.

**Touches:** crates/core/src/manager/media_run/placement.rs, place; crates/core/src/manager/file_exists.rs, replace_at; media_run/mod.rs, run; crates/core/src/manager/media_run/placement.rs place, final_path, crates/core/src/manager/file_exists.rs replace_at, crates/core/src/manager/media_run/mod.rs run.

**Findings:**

- `17-2`: confirmed, P2. Answering Replace when the file's extension differs from the planned name downloads everything again and leaves the old work folder behind. Full text in `chunks/17.md`, verdict in `verdicts/B06.md`.
- `Xpath-media-download-5`: confirmed, P2. Answering Replace for a file with another extension abandons the finished video and downloads it again. Full text in `cross/path-media-download.md`, verdict in `verdicts/B06.md`.

### 14. A new media download can save another download's video

P1, medium, open.

`downloaded.json` and `.done` markers name no link, and `unclaimed_path` allows the name. A link never read completed by using what another download left in `Clip.mp4.wcmedia`, with no yt-dlp call or request, and overwrote the file at that name under Replace. A stream's fingerprint has the same gap, though it needs two sites with the same paths, piece count and final name.

**Fix:** Give the folder's contents an identity, following `placing.done`, which already has `link_digest`. A refreshed stream must keep its segments (a test requires it), and a page's address usually stays the same on refresh, so this does not fix the bad part item.

**Blocks:** nothing. **Blocked by:** item 13.

**Touches:** crates/core/src/manager/media_run/workdir.rs, Downloaded::read; media_run/page.rs, page_attempt and fetch_part; manager/downloads.rs, insert; crates/core/src/manager/media_run/page.rs, page_attempt and fetch_part; crates/streams/src/plan.rs, Plan::fingerprint, and the other places its findings name.

**Findings:**

- `17-1`: confirmed, P1. A new media download takes over parts another download left in the same work folder. Full text in `chunks/17.md`, verdict in `verdicts/B06.md`.
- `Xsecond-review-resume-2`: confirmed, P1. A media work folder's finished parts carry no identity, so another download's video is saved as this one. Full text in `cross/second-review-resume.md`, verdict in `verdicts/B06.md`.
- `Xpath-media-download-6` (the other-video half): confirmed, P2. The Refresh link fix cannot work on media, whose parts are reused whatever the link is. Full text in `cross/path-media-download.md`, verdict in `verdicts/B06.md`.

### 15. Typing a space in search pauses downloads, and Enter can open a file

P1, small, open.

The `space` and `enter` bindings sit on the window root, so they reach a focused field. Typing "a b" in the main search leaves "ab" and runs Pause or Resume on selected rows. Enter in the same field opens a selected finished file (a program still asks first). On Windows a handled key never becomes `WM_CHAR`.

**Fix:** Stop window bindings reaching a focused text field, at the key context rather than per key. The comment on `KEY_CONTEXT` says a field's own keys come first, which does not hold. Start from `proofs/25-1.patch`.

**Blocks:** nothing. **Blocked by:** nothing.

**Touches:** crates/desktop/src/main_window/keymap.rs, shortcuts (on("space", PauseOrResume)), and crates/desktop/src/main_window/mod.rs, list_keys; crates/desktop/src/main_window/keymap.rs, shortcuts (on("enter", OpenSelected)); crates/desktop/src/main_window/mod.rs, list_keys.

**Findings:**

- `25-1`: confirmed, P1. A space cannot be typed in the main search field, and pressing it pauses or resumes the selected downloads. Full text in `chunks/25.md`, verdict in `verdicts/B13.md`.
- `B13-1`: confirmed, P2. Enter in the search field also opens the selected finished download. Full text in `verdicts/B13.md`, verdict in `verdicts/new-findings.md`.

### 16. A very large HLS playlist freezes inspection for minutes to hours

P1, small, fixed.

`auto_picks` has no await and calls `select` for each quality, and its lookups are linear, so the work is quadratic with no limit on HLS qualities. A 2 MiB playlist took 375 s, and the largest size projects to at least 100 minutes on a runtime thread the cancel token cannot reach. Listing the qualities afterwards repeats the cost (`B08-1`), and that many offers go on to the window.

**Fix:** Limit HLS qualities the way `MAX_REPRESENTATIONS` limits DASH. A faster `auto_picks` alone does not fix `B08-1`.

**Blocks:** nothing. **Blocked by:** nothing.

**Touches:** crates/streams/src/source.rs, Source::auto_picks; crates/streams/src/select.rs, variant_named and paired_audio; crates/streams/src/plan.rs, MAX_MANIFEST_ENTRIES; crates/media/src/choice.rs, from_stream; crates/streams/src/model.rs, StreamInfo::extension and listed.

**Findings:**

- `14-2`: confirmed, P1. A large HLS master playlist blocks inspection for minutes to hours on a runtime thread. Full text in `chunks/14.md`, verdict in `verdicts/B08.md`.
- `B08-1`: confirmed, P2. Listing a large master's qualities repeats the quadratic work after inspection. Full text in `verdicts/B08.md`, verdict in `verdicts/new-findings.md`.

### 17. A video with a web page or nothing in place of a segment is reported as complete

P1, medium, fixed.

`fetch_piece` saves any 200 answer as a finished segment, and `ffmpeg::run` takes exit 0 as success. A web page in place of the middle of three 2-second segments gave Completed, with a 2-second MP4 or a 6-second MPEG-TS holding a hole. A page's single-format part goes the same way with no ffmpeg at all (`B06-1`).

**Fix:** Check a segment's body, and detect the format on the single-format path as the two-part path already does. Only the bytes delivered are affected; nothing already saved is damaged.

**Blocks:** nothing. **Blocked by:** nothing.

**Touches:** crates/streams/src/download/fetch.rs fetch_piece, crates/streams/src/download/join.rs mux, move_single_track, crates/streams/src/ffmpeg.rs InputFormat::detect, run; crates/core/src/manager/media_run/save.rs, join_original; crates/core/src/manager/media_run/page.rs, fetch_part.

**Findings:**

- `15-1`: confirmed, P1. A segment answered with a page or nothing instead of media gives a damaged video reported as complete. Full text in `chunks/15.md`, verdict in `verdicts/B06.md`.
- `B06-1`: confirmed, P2. A page's single-format part answered with a web page is saved as the finished video. Full text in `verdicts/B06.md`, verdict in `verdicts/new-findings.md`.

### 18. A lower-integrity program can stand in for the app and receive downloads' cookies

P1, medium, open.

`connect` compares only the pipe owner's SID, and the key file has no no-read-up label. So a low-integrity program of the same user can read the key, make the pipe, and receive what the bridge sends, cookies included. The real app then gets `AlreadyInUse` and does not recover, the same cause as `08-2`, where a name held under the key lock is not always renewed.

**Fix:** Two parts that only close the finding together: check the integrity level in `connect` and label the key file; and renew on any existing name while holding the key lock. The conditions are narrow (code already running at low integrity outside a restricted token, and Wirecatch closed). The proof uses an impersonating thread, not a separate process or the real bridge.

**Blocks:** nothing. **Blocked by:** nothing.

**Touches:** crates/ipc/src/windows.rs, connect and verify_owner; crates/ipc/src/key.rs, write_private through create_private_file and private_sddl; crates/ipc/src/lib.rs, Listener::bind_keyed and IpcError::is_taken_by_someone_else; crates/ipc/src/windows.rs, PipeListener::bind_with and classify_taken.

**Findings:**

- `08-1`: downgraded to P1. The connector trusts a pipe made by a lower-integrity program of the same user. Full text in `chunks/08.md`, verdict in `verdicts/B04.md`.
- `08-2`: confirmed, P2. A name held under the key lock is not always renewed, so another account can still stop the bridge endpoint. Full text in `chunks/08.md`, verdict in `verdicts/B04.md`.

### 19. A DASH template can ask for an address of hundreds of gigabytes and abort the app

P1, small, fixed.

`substitute_into` pushes the id for each `$RepresentationID$` with no limit, and `MAX_URL_LEN` is checked only on the finished text. A 1 MiB manifest held 258 MiB at reduced size, and the full form asks for about 977 GiB, past any machine's memory. The abort itself was not run and is inferred.

**Fix:** Check the length while building the address, not after.

**Blocks:** nothing. **Blocked by:** nothing.

**Touches:** crates/streams/src/dash/template.rs, substitute_into and TrackPieces::push.

**Findings:**

- `14-1`: confirmed, P1. A DASH template can ask for an address of hundreds of gigabytes, which aborts the app. Full text in `chunks/14.md`, verdict in `verdicts/B08.md`.

### 20. A failed flush while saving progress is forgotten

P1, small, fixed.

`Checkpoints::spawn` only logs the error from `save`, and `mark_flushed` has already counted the ranges. So one failed flush is forgotten and the run ends Completed. That a later flush then passes with the bytes gone is documented Linux behaviour, not reproduced here; it needs a network drive, a quota or a failing disk. The same run fails outright when the first save's flush fails, so the handling is inconsistent.

**Fix:** Carry a failed flush through to the end of the run rather than logging it.

**Blocks:** nothing. **Blocked by:** nothing.

**Touches:** crates/engine/src/checkpoint.rs, Checkpoints::spawn and flush_then; crates/engine/src/storage.rs, finalize.

**Findings:**

- `Xsecond-review-disk-writes-1`: downgraded to P1. A failed flush at a checkpoint is forgotten, so a later flush can pass with the bytes gone. Full text in `cross/second-review-disk-writes.md`, verdict in `verdicts/B03.md`.

### 21. An expired redirect target fails a download whose link still works

P1, small, fixed.

`RequestContext.url` is set once from `final_url` and kept for the whole run. When a signed redirect target expires mid-download, every retry asks that dead address and the download fails with 403, although the link the person gave would still give a working one. The part file stays, and a new run recovers.

**Fix:** On a refusal from a redirect target, go back through the original link. A released segment does not help, since any worker asks the same address.

**Blocks:** nothing. **Blocked by:** nothing.

**Touches:** crates/engine/src/transport.rs (Opened::final_url), crates/engine/src/download.rs (RequestContext { url: final_url, .. }), crates/engine/src/http.rs (fetch).

**Findings:**

- `02-1`: confirmed, P1. An expired redirect target fails the download although the link the person gave still works. Full text in `chunks/02.md`, verdict in `verdicts/B11.md`.

### 22. One unreadable value in the list stops the app opening at every start

P1, medium, open.

`is_damage` looks only at `sqlite_error_code()`, which is `None` for a failed conversion. One flipped bit in a stored link leaves `PRAGMA integrity_check` saying `ok`, then `Store::open` fails at every start with nothing set aside. Six other stored forms do the same, and the message points at folder rights or a full disk. The file itself is untouched.

**Fix:** Treat a value that cannot be converted as damage to that row or value, not as a failed open. `03-S1` suggests going further and saving what can be read.

**Blocks:** nothing. **Blocked by:** nothing.

**Touches:** crates/store/src/lib.rs, Store::open_with, is_damage, load, read_download, read_queue, read_value, migrate.

**Findings:**

- `03-1`: confirmed, P1. One unreadable cell stops the app opening at every start. Full text in `chunks/03.md`, verdict in `verdicts/B09.md`.

### 23. Sources are offered that ffmpeg then refuses after the whole download

P1, medium, fixed.

The offer layer lists sources `InputFormat` refuses. A FLAC offered as MP3 downloads whole, then the row fails with `RefreshLink`, so Keep the original is out of reach. Real `a.flac`, `a.ogg` and `a.wav` start with bytes `from_head` does not know.

**Fix:** Tie what is offered to what ffmpeg is allowed to read, in one place. `Xpath-media-download-S2` asks for a test that holds it.

**Blocks:** nothing. **Blocked by:** nothing.

**Check to add:** One test that ties every offered source to `InputFormat`.

**Touches:** crates/media/src/choice.rs from_page, crates/media/src/output/variants.rs audio_variants and video_variants, crates/streams/src/ffmpeg.rs InputFormat::detect, crates/media/src/describe.rs ffmpeg_common.

**Findings:**

- `Xpath-media-download-1`: confirmed, P1. Sources the ffmpeg gate refuses are still offered for joining and conversion, and fail after the whole download. Full text in `cross/path-media-download.md`, verdict in `verdicts/B07.md`.

### 24. A video format that needs cookies always fails when the engine fetches it

P1, medium, fixed.

`Info` and `Format` do not read a format's `cookies`, which real yt-dlp returns outside `http_headers`, and `part_options` keeps the page's cookies off the engine on purpose. So a cookie-gated format gets four refused requests and the row ends "kept refusing", though yt-dlp itself saved the file.

**Fix:** A fix may fall back to yt-dlp after a refusal rather than send the cookies. The comment on `a_format_never_prints_its_header_values` is wrong for this yt-dlp. The 403 in the proof is simulated by the test server.

**Blocks:** nothing. **Blocked by:** nothing.

**Touches:** crates/media/src/ytdlp/info.rs Format, crates/core/src/manager/media_run/page.rs part_options, page_attempt, run_page.

**Findings:**

- `Xpath-media-download-2`: confirmed, P1. A format that needs cookies always fails when the engine fetches it, though yt-dlp could download it. Full text in `cross/path-media-download.md`, verdict in `verdicts/B07.md`.

### 25. The could-not-save banner is wrong both ways, and its Try again saves nothing

P2, medium, open.

The store has one signal, failure at the start of a spell, with no "saved again". So the banner stays after saving recovers, and Try again sends an empty patch that saves nothing, then hides the warning while the failure lasts. Kept writes are retried every 5 s by themselves and at close, so a person who frees the disk does not lose them. Docs also say failed writes are never dropped, while the code drops them (`03-8`).

**Fix:** Add a recovered signal, and base the banner on the kept writes, not on a settings retry. Correct the docs in the same change.

**Blocks:** nothing. **Blocked by:** nothing.

**Touches:** crates/store/src/lib.rs, FailureHandler, write_until_closed; crates/desktop/src/library.rs, storage_problem; crates/desktop/src/main_window/notices_bar.rs, storage_banner; crates/desktop/src/library.rs, Library::storage_problem; crates/core/src/manager/preferences.rs, apply_settings; crates/store/src/lib.rs, write_until_closed, and the other places its findings name.

**Findings:**

- `03-4`: confirmed, P2. The "could not save" banner is wrong in both directions: Try again hides it while saving still fails, and it stays after saving recovers. Full text in `chunks/03.md`, verdict in `verdicts/B09.md`.
- `23-10`: confirmed, P3. The "could not save" banner stays after a later save succeeds. Full text in `chunks/23.md`, verdict in `verdicts/B09.md`.
- `25-2`: downgraded to P2. "Try again" on the could-not-save banner saves nothing and hides the problem. Full text in `chunks/25.md`, verdict in `verdicts/B09.md`.
- `Xfeature-behaviour-3`: confirmed, P2. Try again on the storage banner saves nothing and hides a failure that continues. Full text in `cross/feature-behaviour.md`, verdict in `verdicts/B09.md`.
- `03-8`: confirmed, P3. Docs say failed writes are never dropped, and the code drops them. Full text in `chunks/03.md`, verdict in `verdicts/B09.md`.

### 26. Settings, queues and proxies that cannot be read are silently replaced

P2, medium, open.

`read_value` and `read_queue` turn a failed parse into absence, and absence is then written over. Unreadable settings read as a first start and are saved as defaults, and a queue is erased at open. A proxy profile that cannot be read moves downloads to the system proxy or to no proxy with no notice (`B09-1`), and id counters and batches are lost too (`B09-2`). Proxy passwords read as empty for that session, and the stored value goes at the next proxy change.

**Fix:** Keep what cannot be read, tell the person through `Recovery`, and do not save over it. `categories` holds the plain enum with no `known_or_default`, so one new variant fails all of `AppSettings`.

**Blocks:** nothing. **Blocked by:** nothing.

**Touches:** crates/store/src/lib.rs, read_value, read_queue, load, Snapshot, Recovery; crates/core/src/manager/restore.rs, restore; crates/core/src/manager/mod.rs, Manager::open; crates/store/src/lib.rs, read_queue and save_queues; crates/store/src/lib.rs, load_proxy_servers; crates/core/src/manager/restore.rs, forget_missing_profiles; crates/store/src/lib.rs, load (LAST_IDS_KEY, BATCHES_KEY).

**Findings:**

- `03-2`: confirmed, P2. Settings, queues and proxy passwords that cannot be read are replaced for good, without telling the person. Full text in `chunks/03.md`, verdict in `verdicts/B09.md`.
- `04-6`: confirmed, P3. Opening the app deletes a queue this build cannot read. Full text in `chunks/04.md`, verdict in `verdicts/B09.md`.
- `B09-1`: confirmed, P2. A proxy server that cannot be read silently moves downloads off it. Full text in `verdicts/B09.md`, verdict in `verdicts/new-findings.md`.
- `B09-2`: confirmed, P3. Id counters and queue batches that cannot be read are lost without a notice. Full text in `verdicts/B09.md`, verdict in `verdicts/new-findings.md`.

### 27. A removal under way at quit comes back, and a capture is downloaded twice

P2, medium, fixed.

`Hold::Removing` is memory only, and `finish_removing` fails on `lock_open` once `shutdown` set `closed`. So a removal stopping when the app quits never reaches the store: the download returns at the next start. A capture the browser was told to keep is then downloaded by both, against D27, and a withdrawal has the same third way in (`B02-3`).

**Fix:** Make the removal durable before answering. The window is at most `stop_timeout`, and nothing is lost.

**Blocks:** nothing. **Blocked by:** nothing.

**Touches:** crates/core/src/manager/downloads.rs, finish_removing; crates/core/src/manager/mod.rs, shutdown; crates/core/src/manager/downloads.rs, finish_removing, remove_in_background; crates/core/src/manager/captures.rs, add_capture's Err arm of await_answer; crates/core/src/manager/captures.rs, withdraw_capture; crates/core/src/manager/downloads.rs, remove_in_background.

**Findings:**

- `04-5`: confirmed, P2. A removal under way when the app closes is forgotten. Full text in `chunks/04.md`, verdict in `verdicts/B02.md`.
- `Xconcurrency-2`: confirmed, P2. A removal still stopping its run when the app quits is undone, and the download starts again at the next start. Full text in `cross/concurrency.md`, verdict in `verdicts/B02.md`.
- `Xshutdown-2`: confirmed, P2. A removal in flight at quit is dropped, so the download returns and a refused capture is downloaded twice. Full text in `cross/shutdown.md`, verdict in `verdicts/B02.md`.
- `B02-3`: confirmed, P3. A withdrawn capture comes back if the app quits while it stops. Full text in `verdicts/B02.md`, verdict in `verdicts/new-findings.md`.

### 28. Nothing knows whether a download can continue, so messages promise what will not happen

P2, medium, open.

`Outcome::Paused` and `describe` carry no "can continue", and an unknown size travels as `UNKNOWN_END`. So the CLI prints 16 EiB left and promises a continuation, the app says "the part already downloaded is kept", and the refresh dialog says "What is already downloaded is kept". On a server with no ranges or no size, the next run deletes both files and starts at byte 0.

**Fix:** One type change covers both causes: stop `UNKNOWN_END` leaking as a count, and carry whether a download can continue. `RemoteFile::resumable` already exists at start time, so core could store it.

**Blocks:** nothing. **Blocked by:** nothing.

**Touches:** crates/engine/src/download.rs, finish; crates/engine/src/segment.rs, remaining_bytes; crates/cli/src/main.rs, summary (and the module doc); crates/engine/src/download.rs, Outcome::Paused; crates/messages/src/lib.rs, describe (arms Stalled and PrematureEof), http_status (the Worked::Before arm) and stopped_unexpectedly; crates/desktop/src/main_window/refresh_dialog.rs, the note text.

**Findings:**

- `01-6`: confirmed, P2. A paused download of unknown size reports about 16 EiB left. Full text in `chunks/01.md`, verdict in `verdicts/B03.md`.
- `32-1`: confirmed, P2. The pause line gives a size of 16 EiB and promises a continuation that will not happen. Full text in `chunks/32.md`, verdict in `verdicts/B03.md`.
- `28-1`: confirmed, P2. "The part already downloaded is kept" is told for downloads that cannot resume. Full text in `chunks/28.md`, verdict in `verdicts/B03.md`.
- `B03-1`: confirmed, P2. The refresh dialog promises "What is already downloaded is kept" for downloads that cannot resume. Full text in `verdicts/B03.md`, verdict in `verdicts/new-findings.md`.

### 29. The Public Suffix List's date and Check now were never put in Settings

P2, medium, open.

D44.4 promises the Public Suffix List's date, last check and Check now, and only the on and off switch was drawn. `api::suffix_list` and `api::check_suffix_list` have no caller, and nothing reads `SuffixListChecked`, so a failed check never reaches a person. The core's check and its tests work.

**Fix:** Draw the controls, or record a decision that drops them and remove the two commands (`B19-4`).

**Blocks:** nothing. **Blocked by:** nothing.

**Touches:** crates/shell/src/api.rs, suffix_list, check_suffix_list; the missing part belongs in main_window/settings/general.rs; crates/protocol/src/app.rs, SuffixListInfo, AppSettings::update_suffix_list, UiEvent::SuffixListChecked; crates/desktop/src/main_window/settings/general.rs; crates/desktop/src/main_window/settings/general.rs, updates.

**Findings:**

- `23-5`: confirmed, P2. Settings has no Check now, publication day or last check for the Public Suffix List that D44.4 records. Full text in `chunks/23.md`, verdict in `verdicts/B18.md`.
- `30-3`: confirmed, P2. The Public Suffix List's date, last check, failure note and Check now are not in Settings. Full text in `chunks/30.md`, verdict in `verdicts/B18.md`.
- `Xfeature-behaviour-4`: confirmed, P2. The Public Suffix List's date, last check and Check now do not exist in Settings. Full text in `cross/feature-behaviour.md`, verdict in `verdicts/B18.md`.
- `B19-4`: confirmed, P3. api::suffix_list and api::check_suffix_list have no caller. Full text in `verdicts/B19.md`, verdict in `verdicts/new-findings.md`.

### 30. The window redoes whole-list work on every frame and progress tick

P2, large, open.

`Library::apply` notifies for each `DownloadsChanged`, up to ten times a second, and everything that observes it does list-sized work. `render` runs `lines` at least twice a frame, `selected_rows` is quadratic for a large selection, the tray rebuilds a whole-list map, and the sidebar walks the list seven times. The queue editor's proxy list also loses its cursor on each notify (`27-8`). The figures come from a bench of the same shape at 5,000 rows and up, not from the app.

**Fix:** Cache `lines` and give observers a finer signal than one notify for everything. A finer notify alone helps `25-6` only in part. `27-8` is the second call of `set_selected_value`, a one-line part of this.

**Blocks:** nothing. **Blocked by:** nothing.

**Touches:** crates/desktop/src/main_window/selection.rs, selected_rows, shown_ids, selection_bar, every_circle; crates/desktop/src/main_window/list.rs, lines, members; crates/desktop/src/main_window/sort.rs, compare; crates/desktop/src/tray.rs, follow; crates/desktop/src/main_window/queue_proxy.rs, ProxyForm::new (listing); crates/desktop/src/library.rs, apply (DownloadsChanged), and the other places its findings name.

**Findings:**

- `25-6`: confirmed, P2. Every frame recomputes the whole list several times, quadratically for a large selection. Full text in `chunks/25.md`, verdict in `verdicts/B13.md`.
- `26-7`: confirmed, P3. The tray rebuilds a whole-list map on the interface thread at every progress tick. Full text in `chunks/26.md`, verdict in `verdicts/B13.md`.
- `27-8`: confirmed, P3. The queue editor's proxy list moves its cursor back ten times a second while downloads run. Full text in `chunks/27.md`, verdict in `verdicts/B13.md`.
- `B13-3`: confirmed, P3. More whole-list walks on each progress tick. Full text in `verdicts/B13.md`, verdict in `verdicts/new-findings.md`.

### 31. The Add dialog's link rule leaves out ftpes, so such links cannot be added

P2, small, open.

`links.rs` accepts `https?` and `ftps?`, while `launch.rs`, the engine and core also take `ftpes`. So an `ftpes://` link from a command line stays in the Add dialog, counts as a line with no link and is refused, and no window can add one. `ftps://host:21` asks for the same TLS, though nothing says so.

**Fix:** Read the engine's `WEB_SCHEMES` and `FTP_SCHEMES` instead of a second list, including the `NO_LINK` text that names only three.

**Blocks:** nothing. **Blocked by:** nothing.

**Check to add:** A test that `links` and `launch` agree on every scheme.

**Touches:** crates/shell/src/launch.rs (LINK_SCHEMES), crates/desktop/src/links.rs (LINK), crates/desktop/src/add_dialog.rs (urls, hint text); crates/desktop/src/links.rs, LINK; against crates/engine/src/url.rs, WEB_SCHEMES and FTP_SCHEMES, and crates/shell/src/launch.rs, LINK_SCHEMES; crates/desktop/src/links.rs, LINK and extract; crates/shell/src/launch.rs, LINK_SCHEMES and is_launch_link, and the other places its findings name.

**Findings:**

- `22-2`: confirmed, P2. ftpes:// links from a command line reach an Add dialog that cannot read them; no window of the app can add one. Full text in `chunks/22.md`, verdict in `verdicts/B18.md`.
- `Xrepeated-logic-1`: confirmed, P2. The app's own list of link schemes leaves out ftpes, so such a link cannot be added from any window. Full text in `cross/repeated-logic.md`, verdict in `verdicts/B18.md`.
- `Xpath-normal-download-4` (case a): confirmed, P2. The Add dialog's link rule drops or cuts links every other layer accepts. Full text in `cross/path-normal-download.md`, verdict in `verdicts/B18.md`.

### 32. A raw non-ASCII byte or another charset in a header loses the file name or fails a redirect

P2, small, fixed.

`to_str` refuses a header holding one raw byte, so a raw UTF-8 plain name beside a good `filename*` saves as `get.php`, and a redirect to `/café.zip` fails with "not a valid web address". Separately, `from_content_disposition` ignores the charset, so a Latin-1 `filename*` saves as `caf<U+FFFD>.pdf` over a good plain `filename`.

**Fix:** Read header bytes rather than requiring ASCII, and honour the charset or fall back to the plain name. The two sit on neighbouring lines.

**Blocks:** nothing. **Blocked by:** nothing.

**Check to add:** A `disallowed-methods` entry for `HeaderValue::to_str` in the engine.

**Touches:** crates/engine/src/http.rs, probe_range; crates/engine/src/http.rs, send; crates/engine/src/naming.rs, from_content_disposition.

**Findings:**

- `02-2`: confirmed, P2. A Content-Disposition holding one raw non-ASCII byte loses the whole file name, filename* included. Full text in `chunks/02.md`, verdict in `verdicts/B11.md`.
- `B11-1`: confirmed, P2. A redirect to an address written in raw UTF-8 fails the download. Full text in `verdicts/B11.md`, verdict in `verdicts/new-findings.md`.
- `01-7`: confirmed, P2. filename* in another charset gives a damaged name, and beats a good plain filename. Full text in `chunks/01.md`, verdict in `verdicts/B11.md`.

### 33. Failing to delete a temporary file turns a saved video into a failure

P2, small, fixed.

`work.remove()?` follows the move, so failing to delete the temporary folder after a video is saved turns the row Failed. Failing to delete a decrypted segment's part file also stops the download with a disk error (`B06-2`). The likely trigger on Windows is a scanner holding a file.

**Fix:** Use `discarded`, whose doc says a failed delete stops nothing, in both places.

**Blocks:** nothing. **Blocked by:** nothing.

**Touches:** crates/streams/src/download/join.rs join; crates/streams/src/download/join.rs, join; crates/streams/src/download/fetch.rs, fetch_piece.

**Findings:**

- `15-2`: confirmed, P2. Failing to delete the temporary folder turns a saved video into a failed download. Full text in `chunks/15.md`, verdict in `verdicts/B06.md`.
- `Xsecond-review-disk-writes-6`: confirmed, P2. A saved stream is reported as failed when its temporary folder cannot be deleted. Full text in `cross/second-review-disk-writes.md`, verdict in `verdicts/B06.md`.
- `B06-2`: confirmed, P3. Failing to delete a decrypted segment's part file fails the download. Full text in `verdicts/B06.md`, verdict in `verdicts/new-findings.md`.

### 34. A damaged or squatted key does not lead the bridge to start the app and renew it

P2, small, open.

`attempt` treats only three errors as "start the app", so a damaged key, a squatter that refuses access and a squatter with all instances busy all end with no launch. D41 expects a launch to renew the key. The bridge's tests use a made-up `WrongOwner`, and the Linux half is read, not run.

**Fix:** Widen the list of errors in `attempt` that lead to a launch.

**Blocks:** nothing. **Blocked by:** nothing.

**Touches:** crates/bridge/src/app.rs, attempt and connect_or_launch; Unavailable::from_ipc; crates/ipc/src/windows.rs, connect; crates/ipc/src/unix.rs, endpoint; crates/bridge/src/app.rs, attempt.

**Findings:**

- `06-1`: confirmed, P2. A damaged endpoint key stops the bridge from starting the app that would repair it. Full text in `chunks/06.md`, verdict in `verdicts/B04.md`.
- `08-3`: confirmed, P2. A client that meets a squatter usually gets a plain error, so the bridge never starts the app to renew the key. Full text in `chunks/08.md`, verdict in `verdicts/B04.md`.
- `B04-2`: downgraded to P3. A squatter whose instances are all busy also gets a plain error, after a two-second wait. Full text in `verdicts/B04.md`, verdict in `verdicts/new-findings.md`.

### 35. An open file-exists question outlives the download state it asked about

P2, medium, open.

Only `requeue`, `cancel` and `finish_removing` close a file-exists question, and `answer_file_exists` checks neither status nor a move under way. After Resume all, a late Replace returns `Ok` and sets a finished row back to Queued. The usual path is harmless, since `ask` closes an older question when the duplicate is found again.

**Fix:** Close the question in `resume_all` and `refresh_link`, and have the answer check the status. Cases b and c of `05-7` also need a close in `set_download_folder` and a move-under-way check.

**Blocks:** nothing. **Blocked by:** nothing.

**Touches:** crates/core/src/manager/downloads.rs, resume_all and refresh_link; crates/core/src/manager/file_exists.rs, answer_file_exists; missing close_file_exists in downloads.rs (resume_all, refresh_link) and relocate.rs (set_download_folder); crates/core/src/manager/downloads.rs, resume_all, refresh_link.

**Findings:**

- `04-4`: confirmed, P2. Resume all and Refresh link leave the file-exists question open. Full text in `chunks/04.md`, verdict in `verdicts/B01.md`.
- `05-7`: confirmed, P2. An open question outlives Resume all, a new link and a move, and answering it later acts on the changed download. Full text in `chunks/05.md`, verdict in `verdicts/B01.md`.
- `Xconcurrency-8`: confirmed, P2. Resume all and a new link leave a file-exists question open while the download runs. Full text in `cross/concurrency.md`, verdict in `verdicts/B01.md`.

### 36. Proxy exceptions are accepted that can never match

P2, medium, fixed.

`clean_hosts` accepts anything the matcher then ignores: `https://intranet.example/`, `host:8443` and `bücher.example` never match, and a bracketed IPv6 never matches either, also when it comes from the Windows bypass list. The count limit comes after a quadratic scan under the manager's lock, 76 seconds for a 1 MB file.

**Fix:** Read entries into the matcher's grammar when accepted, as D73.8 and D72.13 ask, with the bracket fix in the engine's `exceptions` because the system list skips `clean_hosts`. Check the limit first. `19-S1` suggests saving a pasted address as its host.

**Blocks:** nothing. **Blocked by:** nothing.

**Touches:** crates/engine/src/proxy.rs, exceptions; crates/settings/src/proxy.rs clean_hosts; crates/settings/src/proxy.rs clean_hosts, reached from crates/settings/src/settings_file.rs merge_localized through validate and validate_queue_exceptions.

**Findings:**

- `09-3`: confirmed, P2. An IPv6 exception written in brackets never matches. Full text in `chunks/09.md`, verdict in `verdicts/B15.md`.
- `19-3`: confirmed, P2. A proxy exception that can never match is accepted and saved without a word. Full text in `chunks/19.md`, verdict in `verdicts/B15.md`.
- `19-5`: confirmed, P2. The limit on proxy exceptions is checked after a quadratic scan, under the manager's lock. Full text in `chunks/19.md`, verdict in `verdicts/B15.md`.

### 37. A typed file name is dropped when the download turns out to be a page

P2, medium, fixed.

`AnsweredPage` and `Held` have no name field, so a name typed in the Add dialog is lost for a page without a video and for a video site. `report.html` typed gives a file named `about`, and with saved choices the name goes silently, while the dialog says nothing.

**Fix:** Carry the name in one field. What a named video should do is a choice for `Xfeature-behaviour-6`, whose doc mentions it but D51 does not.

**Blocks:** nothing. **Blocked by:** nothing.

**Touches:** crates/core/src/manager/added_pages.rs, AnsweredPage::of, add_page_as_file; crates/core/src/manager/added_pages.rs, AnsweredPage::of, read_answered_page and add_page_as_file; crates/desktop/src/add_dialog.rs, submit; crates/core/src/manager/added_pages.rs, add_video_page.

**Findings:**

- `17-7`: confirmed, P2. A name typed for a link that turns out to be a web page without a video is dropped. Full text in `chunks/17.md`, verdict in `verdicts/B07.md`.
- `Xpath-normal-download-5`: confirmed, P2. A file name typed in the Add dialog is lost when the server answers with a web page. Full text in `cross/path-normal-download.md`, verdict in `verdicts/B07.md`.
- `Xfeature-behaviour-6`: confirmed, P2. A file name typed in the Add dialog is dropped for a link to a video site. Full text in `cross/feature-behaviour.md`, verdict in `verdicts/B07.md`.

### 38. Pages yt-dlp describes at the top level, or with several videos, are misread

P2, medium, fixed.

`Info` does not read the top-level `protocol`, `http_headers`, `vcodec` or `acodec`, nor `_type` and `entries`. So a top-level HLS page reads as a plain file, top-level audio is offered as a video in the video folder (`B07-1`), and a page with two `<video>` tags is reported as having none. Real yt-dlp prints all of these.

**Fix:** Read the whole shape in one change to `Info`. No decision records several videos as a limit, so decide what to offer.

**Blocks:** nothing. **Blocked by:** nothing.

**Touches:** crates/media/src/ytdlp/info.rs Info, Info::all_formats, Format::transport; crates/media/src/ytdlp/info.rs Info, crates/media/src/choice.rs from_page, nothing, crates/core/src/manager/media.rs analyse; crates/media/src/ytdlp/info.rs, Info, Info::all_formats.

**Findings:**

- `Xpath-media-download-3`: confirmed, P2. A page yt-dlp describes at the top level loses its protocol and headers. Full text in `cross/path-media-download.md`, verdict in `verdicts/B07.md`.
- `Xpath-media-download-4`: confirmed, P2. A page with several videos is reported as having none. Full text in `cross/path-media-download.md`, verdict in `verdicts/B07.md`.
- `B07-1`: confirmed, P2. A page described at the top level as audio is offered as a video. Full text in `verdicts/B07.md`, verdict in `verdicts/new-findings.md`.

### 39. The CLI strands files under another name when a resume file is damaged or the name is numbered

P2, medium, fixed.

`plan_in_folder` builds one path and trusts or abandons what it finds there. A numbered first run starts over on the second, and a damaged resume file saves as `f (1).bin` and leaves `f.bin.wcpart` at full length for good, against `saved_state`'s doc. It reaches the CLI and the public API, not the app. No test checks that another link's partial file is left alone (`M-2`).

**Fix:** Write the `M-2` test first, then look past the preferred name. `proofs/FT-kill.patch` holds the fault harness, which the plan turns into a permanent test here.

**Blocks:** nothing. **Blocked by:** nothing.

**Touches:** crates/engine/src/target.rs, plan_in_folder; crates/engine/src/naming.rs, create_free_part; crates/engine/src/target.rs, plan_in_folder and saved_state; crates/engine/src/target.rs, same_download, as used by plan_in_folder.

**Findings:**

- `01-1`: confirmed, P2. Running the same request again starts over when the first run took a numbered name. Full text in `chunks/01.md`, verdict in `verdicts/B03.md`.
- `F-1`: confirmed, P2. An unreadable or missing resume file makes the CLI save under another name and strands the old files. Full text in `fault-tests.md`, verdict in `verdicts/B03.md`.
- `M-2`: real gap, P3. Nothing tests that another link's partial download is left alone. Full text in `verdicts/mutation.md`, verdict in `verdicts/mutation.md`.

### 40. The add-on's state mixes an operation with whether the tools work

P2, medium, open.

`addon_info` reports the operation's state first and carries no "usable". So a failed update blocks video qualities the installed add-on can still download, About says it is not installed, and the card offers only Try again, which can do nothing for good (`B12-2`).

**Fix:** Add a "usable" fact to `AddonInfo` and have the form, About and the card read it.

**Blocks:** item 53. **Blocked by:** nothing.

**Touches:** crates/desktop/src/download_windows/media_form.rs, MediaForm::needs_install; crates/desktop/src/main_window/settings/about.rs, addon_versions; crates/desktop/src/main_window/settings/addon.rs, buttons; crates/core/src/manager/addons/mod.rs, install_addon; crates/desktop/src/download_windows/media_form.rs, install_then.

**Findings:**

- `24-3`: confirmed, P2. A failed add-on update blocks video qualities that the installed add-on can still download. Full text in `chunks/24.md`, verdict in `verdicts/B12.md`.
- `27-4`: downgraded to P3. About says the media add-on is not installed while it updates or after a failed update. Full text in `chunks/27.md`, verdict in `verdicts/B12.md`.
- `B12-2`: confirmed, P3. A failed update hides Remove and Check for updates, and Try again can do nothing for good. Full text in `verdicts/B12.md`, verdict in `verdicts/new-findings.md`.

### 41. A download retried or removed just after it ended is still told as ended

P2, medium, open.

`Teller` only adds, so a later event does not correct a pending notice. A download retried inside the batch is told twice ("a.zip and a.zip"), a removed one is still told, and the complete window gains a row for a download no longer in the list (`B17-2`). `teller.rs` has no tests, which is why this went unseen.

**Fix:** Add the tests, then let later events change what is pending.

**Blocks:** nothing. **Blocked by:** nothing.

**Touches:** crates/shell/src/teller.rs, Teller::ended, take_due; crates/desktop/src/notices.rs, watch, ask_browser; crates/shell/src/browser_notices.rs, outdated_by; crates/shell/src/teller.rs, whole file; crates/desktop/src/download_windows/finished.rs, add; crates/shell/src/teller.rs, Teller::take_due.

**Findings:**

- `26-5`: confirmed, P2. A download that is retried or removed just after it ended is still told as ended, and can be counted twice. Full text in `chunks/26.md`, verdict in `verdicts/B17.md`.
- `26-8`: confirmed, P3. The decision logic of what is told when has no tests. Full text in `chunks/26.md`, verdict in `verdicts/B17.md`.
- `B17-2`: confirmed, P3. The download complete window lists a download retried or removed during the batch, and a removed one stays. Full text in `verdicts/B17.md`, verdict in `verdicts/new-findings.md`.

### 42. The network guard's local-address rule is written twice

P3, small, open.

`check` and `all_private` in `net_guard.rs` hold the same rule, and `check` repeats the host match of `host_addresses`.

**Fix:** Keep one, before the next item adds a caller.

**Blocks:** item 43. **Blocked by:** nothing.

**Touches:** crates/engine/src/net_guard.rs, NetworkGuard::check and all_private.

**Findings:**

- `09-8`: confirmed, P3. NetworkGuard::check repeats all_private. Full text in `chunks/09.md`, verdict in `verdicts/B14.md`.

### 43. The local network rule does not apply to FTP

P2, medium, open.

`download_with` makes a guard for `http` and `https` only, and `Target::resolve` uses a bare `lookup_host`. So a website's FTP link that rebinds or mixes answers reaches the local network, logs in and saves, while the doc of `refuses` promises otherwise. Nothing read goes back to the site. D34.9 and D39.1 state the rule for every start.

**Fix:** A filtered lookup in `Target::resolve`, plus a check of each address under `Block`.

**Blocks:** nothing. **Blocked by:** item 42.

**Touches:** crates/engine/src/download.rs, download_with; crates/engine/src/ftp/mod.rs, Target::resolve and probe; the promise in crates/sites/src/local_network.rs, refuses; crates/engine/src/download.rs download_with (the guard is built only for http and https), crates/engine/src/transport.rs open (the FTP arm ignores guard), crates/engine/src/ftp/mod.rs Target::new and Target::resolve, and the other places its findings name.

**Findings:**

- `09-1`: confirmed, P2. A website's FTP link is not kept off the local network. Full text in `chunks/09.md`, verdict in `verdicts/B14.md`.
- `21-1` (first form): confirmed, P2. A website's FTP link is not held to the Block rule, so it can reach the local network. Full text in `chunks/21.md`, verdict in `verdicts/B14.md`.
- `Xsecurity-bridge-and-pipe-input-1` (rebinding form): confirmed, P2. A website's FTP link reaches the local network, past both refusals and with no guard. Full text in `cross/security-bridge-and-pipe-input.md`, verdict in `verdicts/B14.md`.

### 44. "This host is under that site" is written four times, and the copies differ

P3, medium, fixed.

Whether a host is covered by a site is written separately in `on_site`, `local_network::is_under`, `queues` and `SitePattern::covers`. They differ on trailing dots, case and addresses, so `example.com.` misses a queue rule and `youtube.com.` misses video detection.

**Fix:** One function. `queues::is_under` also serves the `*.` rule and needs a label before the name, so it is not a plain copy.

**Blocks:** item 45. **Blocked by:** nothing.

**Touches:** crates/sites/src/video_sites.rs, on_site; crates/sites/src/local_network.rs, is_under; crates/queues/src/sites.rs, SitePattern::covers with its is_under; crates/queues/src/sites.rs, SitePattern::covers and is_under; crates/sites/src/local_network.rs, is_local_by_itself, is_listed, is_under and address_in; crates/media/src/ytdlp/mod.rs, cookies_first.

**Findings:**

- `10-4`: confirmed, P3. The test for "this host is that name or under it" is written three times. Full text in `chunks/10.md`, verdict in `verdicts/B14.md`.
- `Xrepeated-logic-3` (the copies): confirmed, P3. Whether a host is covered by a site is written separately in four places, which differ on trailing dots, case and addresses. Full text in `cross/repeated-logic.md`, verdict in `verdicts/B14.md`.

### 45. ftps and ftpes hosts are read raw by every check

P2, small, open.

`ftps` and `ftpes` hosts are opaque to the URL parser and stay percent-encoded and mixed-case, and only `Target::new` decodes them. So `%31%32%37.0.0.1` passes the local checks and connects to `127.0.0.1`, though TLS normally stops it after the connection opens, and a `<local>` queue rule misses `ftps://FILES.Home.Arpa/a.zip` (`B14-1`).

**Fix:** One normalised host at `parse_url`, running `Host::parse` on what it decodes, so that `ftps://2130706433/a` also reads as an address.

**Blocks:** nothing. **Blocked by:** item 44.

**Touches:** crates/engine/src/download.rs download_with (the guard is built only for http and https), crates/engine/src/transport.rs open (the FTP arm ignores guard), crates/engine/src/ftp/mod.rs Target::new and Target::resolve, and the other places its findings name.

**Findings:**

- `21-1` (second form): confirmed, P2. A website's FTP link is not held to the Block rule, so it can reach the local network. Full text in `chunks/21.md`, verdict in `verdicts/B14.md`.
- `Xsecurity-bridge-and-pipe-input-1` (percent-encoded form): confirmed, P2. A website's FTP link reaches the local network, past both refusals and with no guard. Full text in `cross/security-bridge-and-pipe-input.md`, verdict in `verdicts/B14.md`.
- `Xrepeated-logic-3` (its ftps cases): confirmed, P3. Whether a host is covered by a site is written separately in four places, which differ on trailing dots, case and addresses. Full text in `cross/repeated-logic.md`, verdict in `verdicts/B14.md`.
- `B14-1`: confirmed, P3. The own-network check reads an ftps host raw, so a <local> queue rule misses it. Full text in `verdicts/B14.md`, verdict in `verdicts/new-findings.md`.

### 46. An apostrophe in a link leaves its password or signed query in the log

P2, small, fixed.

`ends_a_link` stops at an apostrophe, so a password or signed query after one stays in the log. `redaction.json` has no such sample. The desktop's `link_for_display` happens to hide it on screen today, which is why the next item needs this first.

**Fix:** Fix the scanners' end of link.

**Blocks:** item 47. **Blocked by:** nothing.

**Check to add:** Add apostrophe samples to `crates/test-support/data/redaction.json`.

**Touches:** crates/logging/src/redact.rs, ends_a_link, as used by hide_link_credentials and hide_queries.

**Findings:**

- `20-1`: confirmed, P2. An apostrophe in a link ends the link for the redaction, so the password or signed query after it stays in the log. Full text in `chunks/20.md`, verdict in `verdicts/B15.md`.

### 47. A link's password reaches the browser extension, and a crash's partial file is not picked up

P2, medium, open.

The real link and the shown link are told apart at each use. `ask` puts the real link in the question, whose doc says the password is removed, and `crates/server` and the bridge relay send every event on to the extension. `left_by` compares a real link with a saved cleaned one, so a partial file from a crash is not picked up and the run takes `a (1).zip`. `link_for_display` is a second mask over links already masked.

**Fix:** Clean the link once where it leaves the manager, compare with a matching form, and remove the second mask after item 46.

**Blocks:** nothing. **Blocked by:** item 46.

**Touches:** crates/core/src/manager/file_exists.rs, ask; crates/core/src/naming.rs, left_by and normalized_url; crates/desktop/src/text.rs, link_for_display; used by main_window/list.rs and main_window/details.rs; against crates/logging/src/redact.rs, url_for_display.

**Findings:**

- `05-5`: confirmed, P2. The file-exists question carries the link's password to the UI. Full text in `chunks/05.md`, verdict in `verdicts/B15.md`.
- `05-10`: confirmed, P2. A partial file left by a crash is not picked up again when the link has a password. Full text in `chunks/05.md`, verdict in `verdicts/B15.md`.
- `Xrepeated-logic-8`: confirmed, P3. text::link_for_display is a hand-written second password mask over links already masked. Full text in `cross/repeated-logic.md`, verdict in `verdicts/B15.md`.

### 48. A finished file with a leftover resume file is downloaded again

P2, small, open.

`finished_size` treats a resume file alone like a part file. A finished file beside a leftover resume file reads as unfinished after a restart, and is fetched again as `f (1).bin`, leaving two copies. It may not need a crash: `abandoned.check()` runs between `finalize` and discarding the resume file, which is not confirmed by a run.

**Fix:** Local to core's reading of "file and resume file, no part file". The engine's side is sound.

**Blocks:** item 174. **Blocked by:** item 4.

**Touches:** crates/engine/src/download.rs, finish; crates/engine/src/target.rs, plan_exact; crates/core/src/naming.rs, finished_size; crates/core/src/manager/run.rs, finish; crates/core/src/manager/restore.rs, restore.

**Findings:**

- `Xcrash-and-resume-2`: confirmed, P2. A crash between the engine's rename and deleting the resume file downloads the file again and keeps two copies. Full text in `cross/crash-and-resume.md`, verdict in `verdicts/B02.md`.
- `Xpath-normal-download-6`: confirmed, P2. A finished file with a leftover resume file is downloaded again and kept twice after a restart. Full text in `cross/path-normal-download.md`, verdict in `verdicts/B02.md`.

### 49. A title holding % or $ sends yt-dlp's file to another folder or stops it

P2, small, fixed.

A file name holding `%(`, `%%`, `$NAME` or `%NAME%` reaches yt-dlp's `-o` template unescaped. `a%(ext)s` gives `amp4.mp4.wcmedia`, `%TEMP%` moves the folder, and `grow (5%(approx))` is an option error. A download folder holding such text fails the same way. No way out of the download folder was found, so it is not a hole.

**Fix:** A relative template, since `process::run` starts in `work`.

**Blocks:** nothing. **Blocked by:** nothing.

**Touches:** crates/media/src/ytdlp/download.rs, download_args; the title reaches the path through crates/media/src/lib.rs, file_name, and work_dir.

**Findings:**

- `16-1`: confirmed, P2. A file name holding %(, %%, $NAME or %NAME% sends yt-dlp's download to another folder or stops it. Full text in `chunks/16.md`, verdict in `verdicts/B07.md`.
- `Xsecurity-path-handling-3`: confirmed, P2. A video title with a yt-dlp field sends the file to another folder and the download fails. Full text in `cross/security-path-handling.md`, verdict in `verdicts/B07.md`.

### 50. Path parameters such as ;type=i or ;jsessionid end up in the file name

P2, small, fixed.

`from_url` returns the whole last path segment, so `ftp://host/pub/file.bin;type=i` and `a.zip;jsessionid=ABC123` keep their parameters in the name. The extension is lost, so the file does not open by double click and category rules see the wrong type.

**Fix:** Cut path parameters once, in `from_url`. The CLI, core's `file_name` and `settled_name` all read it.

**Blocks:** nothing. **Blocked by:** nothing.

**Touches:** crates/engine/src/ftp/mod.rs Target::new (display_url), crates/engine/src/transport.rs open (final_url), crates/engine/src/naming.rs from_url, callers crates/engine/src/target.rs suggested_name and crates/core/src/naming.rs file_name and settled_name; crates/engine/src/naming.rs, from_url; callers crates/engine/src/target.rs suggested_name and crates/core/src/naming.rs file_name and settled_name.

**Findings:**

- `21-2`: confirmed, P2. ;type=i in an FTP link ends up in the saved file's name. Full text in `chunks/21.md`, verdict in `verdicts/B11.md`.
- `B11-2`: confirmed, P2. Path parameters of a web link end up in the saved file's name. Full text in `verdicts/B11.md`, verdict in `verdicts/new-findings.md`.

### 51. Add-on installs and app updates get advice about links and folders they do not have

P2, small, open.

`describe` writes advice for a row with a Get a new link button. Add-on installs and app updates keep only `.message`, so a 404 tells them to refresh a link, and a folder they cannot write says "Choose another folder" (`B11-3`).

**Fix:** Give callers with no row wording of their own.

**Blocks:** nothing. **Blocked by:** nothing.

**Roadmap:** Touches the open item "The wrong-size update message is awkward": `fits` and the size check in `app_update.rs` set `resolve_path` with lower-case fragments. Fix that wording here.

**Touches:** crates/messages/src/lib.rs, describe; callers crates/core/src/manager/addons/install.rs (the install download) and crates/core/src/manager/app_update.rs (the installer download); crates/messages/src/lib.rs, disk_failure; callers crates/core/src/manager/addons/install.rs and crates/core/src/manager/app_update.rs.

**Findings:**

- `28-3`: confirmed, P2. Add-on installs and app updates tell the person to refresh a link they do not have. Full text in `chunks/28.md`, verdict in `verdicts/B11.md`.
- `B11-3`: confirmed, P2. Add-on installs and app updates are told to choose another folder they cannot choose. Full text in `verdicts/B11.md`, verdict in `verdicts/new-findings.md`.

### 52. A damaged list set aside can be deleted at the next start

P2, small, open.

`remove_old_copies` reads age from a modified time that the rename keeps. A damaged list last written over a month ago is kept at the first start and deleted at the next, instead of being kept a month. The stamp in the name is never read.

**Fix:** Read age from the stamp in the name.

**Blocks:** nothing. **Blocked by:** nothing.

**Touches:** crates/store/src/lib.rs, remove_old_copies, set_aside, KEEP_COPIES_FOR; crates/store/src/lib.rs, Store::open_with, set_aside, remove_old_copies.

**Findings:**

- `03-5`: confirmed, P2. A damaged list set aside can be deleted at the next start instead of being kept a month. Full text in `chunks/03.md`, verdict in `verdicts/B09.md`.
- `Xdatabase-migrations-1`: confirmed, P2. A damaged list last written over a month ago is deleted on the next start. Full text in `cross/database-migrations.md`, verdict in `verdicts/B09.md`.

### 53. The quality form loses the person's choice or says nothing around an add-on install

P2, small, open.

The quality form's own wait for an install is cleared by any library change, so the person's choice is lost before the install starts. When the install fails the form shows nothing, because `addon_awaited` is set only for an add-on question. `view.rs` has the guard and the form does not.

**Fix:** Use the same guard, and show the reason.

**Blocks:** nothing. **Blocked by:** item 40.

**Touches:** crates/desktop/src/download_windows/media_form.rs, MediaForm::addon_moved and install_then; crates/desktop/src/download_windows/media_form.rs, MediaForm::install_then and addon_moved.

**Findings:**

- `24-1`: confirmed, P2. Install and download loses the person's choice when anything in the list changes before the install starts. Full text in `chunks/24.md`, verdict in `verdicts/B12.md`.
- `24-2`: confirmed, P2. The quality form shows nothing when the add-on install fails. Full text in `chunks/24.md`, verdict in `verdicts/B12.md`.

### 54. The file-exists question never says whether it is the same file

P2, small, open.

`file_exists` reads only id, name, link and folder. The window never shows `same_file` or `size`, so D96's "the person always knows which case it is" is not met, with Replace as the primary button.

**Fix:** Read the question's fields in the window.

**Blocks:** nothing. **Blocked by:** nothing.

**Touches:** crates/desktop/src/download_windows/view.rs, DownloadWindow::file_exists; crates/protocol/src/app.rs, FileExistsQuestion (same_file, size, saved_at).

**Findings:**

- `24-4`: confirmed, P2. The "already in the folder" question hides whether it is the same file. Full text in `chunks/24.md`, verdict in `verdicts/B17.md`.
- `30-1`: confirmed, P2. The file-exists question never says whether the file is the same one. Full text in `chunks/30.md`, verdict in `verdicts/B17.md`.

### 55. Cancel in a download window discards the partial file with no question

P2, small, open.

`progress-cancel` is wired straight to `api::cancel`, next to Hide, in the running and paused states. The partial file goes at one click, while D23.4 says it asks first and the list does ask.

**Fix:** Ask, as `main_window/confirm.rs::cancel` does.

**Blocks:** nothing. **Blocked by:** nothing.

**Touches:** crates/desktop/src/download_windows/view.rs, progress_buttons; crates/desktop/src/download_windows/view.rs, the progress state's Cancel.

**Findings:**

- `24-7`: confirmed, P2. Cancel in a progress window discards the partial file at one click, with no question. Full text in `chunks/24.md`, verdict in `verdicts/B17.md`.
- `U-14`: confirmed, P2. Cancel in the download window deletes the partial file without asking. Full text in `ui.md`, verdict in `verdicts/B17.md`.

### 56. A pasted link is cut at an apostrophe, a space or a bracket, so another address is downloaded

P2, small, open.

The `LINK` regex stops at `'` and a space, and `trim_end_matches` removes a closing bracket without looking for its opener. `report_(final)` becomes `report_(final`, and `https://example.com/a b.zip` offers `https://example.com/a`. Both parse, so the Add dialog downloads the shortened address with no warning.

**Fix:** Fix the character class and match brackets. The scheme list is its own item.

**Blocks:** nothing. **Blocked by:** nothing.

**Touches:** crates/desktop/src/links.rs, LINK and extract; crates/shell/src/launch.rs, LINK_SCHEMES and is_launch_link; crates/core/src/manager/downloads.rs, parse_url.

**Findings:**

- `23-2`: confirmed, P2. A pasted link is silently cut at an apostrophe or loses a closing bracket, so the wrong address is downloaded. Full text in `chunks/23.md`, verdict in `verdicts/B18.md`.
- `Xpath-normal-download-4` (case b): confirmed, P2. The Add dialog's link rule drops or cuts links every other layer accepts. Full text in `cross/path-normal-download.md`, verdict in `verdicts/B18.md`.

### 57. Choosing a file or folder without a file chooser portal does nothing and says nothing

P2, small, open.

`picked.await.ok()?.ok()??` and the two `let Ok(Ok(Some(..)))` in the backup dialog treat an error as a cancel. On a Linux desktop with no XDG portal, choosing a folder, Export and Import do nothing and log nothing. Linux was not run.

**Fix:** Show the error, as ENGINEERING section 4 asks.

**Blocks:** nothing. **Blocked by:** nothing.

**Touches:** crates/desktop/src/files.rs, choose_folder; crates/desktop/src/main_window/backup_dialog.rs, BackupForm::export, BackupForm::preview.

**Findings:**

- `23-6`: confirmed, P2. Choosing a folder on Linux without a file chooser portal does nothing and says nothing. Full text in `chunks/23.md`, verdict in `verdicts/B18.md`.
- `B18-1`: confirmed, P2. Export and Import settings do nothing and say nothing when the file chooser fails. Full text in `verdicts/B18.md`, verdict in `verdicts/new-findings.md`.

### 58. A typed file name with a dot loses the words after it

P2, small, open.

`final_path` uses `Path::with_extension`, which replaces whatever follows the last dot. A typed `Dr. Who` becomes `Dr.mp4`. `wirecatch_streams::model::with_extension` holds the right rule and is `pub(crate)`.

**Fix:** Share the streams rule instead of a second one.

**Blocks:** nothing. **Blocked by:** nothing.

**Check to add:** A `disallowed-methods` entry for `Path::with_extension` in crates that handle a person's file names.

**Touches:** crates/core/src/manager/media_run/placement.rs, final_path; manager/media.rs, insert_chosen; against crates/streams/src/model.rs, with_extension.

**Findings:**

- `17-3`: confirmed, P2. A typed file name containing a dot loses the words after its last dot. Full text in `chunks/17.md`, verdict in `verdicts/B06.md`.
- `Xrepeated-logic-2`: confirmed, P2. Replacing a finished file's extension is written twice, and only the streams copy spares a name's own words. Full text in `cross/repeated-logic.md`, verdict in `verdicts/B06.md`.

### 59. Downloaded installers are never removed

P2, small, fixed.

`fetch_app_update` writes into `updates` and nothing removes it. Only a file failing its check is deleted, so every installer stays for good.

**Fix:** Clean the folder at a known moment, such as the next start. `12-S1` suggests keeping a checked installer for Try again.

**Blocks:** nothing. **Blocked by:** nothing.

**Touches:** crates/core/src/manager/app_update.rs, fetch_app_update; crates/desktop/src/app_update.rs, install; crates/desktop/src/app_update.rs, start.

**Findings:**

- `12-2`: confirmed, P2. Installers are never removed from the updates folder. Full text in `chunks/12.md`, verdict in `verdicts/B05.md`.
- `Xpath-app-update-5`: confirmed, P2. Every downloaded installer stays in the data folder for good. Full text in `cross/path-app-update.md`, verdict in `verdicts/B05.md`.

### 60. An update with no usable file is shown as a package manager update or opens the release page

P2, small, open.

`download: None` has several reasons that the app reads as one. So a person on Windows is told to use a package manager and loses What's new, and a release whose asset is missing or of another size falls through to the unchecked release page with no log.

**Fix:** Tell the reasons apart. The refusal path (`refused`) exists and is not used.

**Blocks:** nothing. **Blocked by:** nothing.

**Touches:** crates/desktop/src/main_window/notices_bar.rs, update_available; crates/core/src/manager/app_update.rs, check_app_update; crates/core/src/manager/app_update.rs check_app_update; crates/desktop/src/app_update.rs install, install_label.

**Findings:**

- `Xpath-app-update-6`: confirmed, P3. The banner tells a person on Windows to use a package manager. Full text in `cross/path-app-update.md`, verdict in `verdicts/B05.md`.
- `Xsecurity-update-signatures-3`: confirmed, P2. A release that fails the signed list falls through to the unchecked release page. Full text in `cross/security-update-signatures.md`, verdict in `verdicts/B05.md`.

### 61. A conversion demands audio or video that a format did not describe

P2, small, fixed.

`has_audio` and `has_video` read an unknown codec as present, and `add_maps` writes `{v}:a:0` with no `?`. A silent MP4 stops ffmpeg with "Invalid argument", and an audio format with no `vcodec` is listed as a video and mapped from a file with no video (`B07-4`). Keep the original is offered.

**Fix:** Allow missing tracks when the codec is unknown, and look at the extension in `has_video`.

**Blocks:** nothing. **Blocked by:** nothing.

**Touches:** crates/media/src/output/arguments.rs add_maps, crates/media/src/ytdlp/info.rs Format::has_audio, crates/media/src/output/variants.rs original_audio_action; crates/media/src/ytdlp/info.rs, Format::has_video; crates/media/src/output/arguments.rs, add_maps.

**Findings:**

- `Xpath-media-download-10`: confirmed, P2. A conversion demands audio from a video whose audio yt-dlp did not describe. Full text in `cross/path-media-download.md`, verdict in `verdicts/B07.md`.
- `B07-4`: confirmed, P2. An audio format without a vcodec counts as a video. Full text in `verdicts/B07.md`, verdict in `verdicts/new-findings.md`.

### 62. A torn last line of a stream's log can truncate its joined track

P2, small, fixed.

`Log::read` counts a torn last line, and a torn `A 0 5 12` wins over `A 3 1000000`, so the track is truncated and its segments silently dropped. The parse is proven; the short write itself was not produced.

**Fix:** Ignore a last line that does not end with a newline.

**Blocks:** nothing. **Blocked by:** nothing.

**Touches:** crates/streams/src/workdir.rs, Log::read and WorkDir::open; crates/streams/src/download/join.rs, concat.

**Findings:**

- `Xsecond-review-disk-writes-4`: confirmed, P2. A torn last line of done.log counts, and an A line cut short destroys the joined track. Full text in `cross/second-review-disk-writes.md`, verdict in `verdicts/B06.md`.
- `Xsecond-review-resume-3`: confirmed, P2. A stream log line cut short is read as a smaller track, which truncates the track and silently drops its segments. Full text in `cross/second-review-resume.md`, verdict in `verdicts/B06.md`.

### 63. Blank lines hide a program's failure reason and grow memory without limit

P2, small, open.

`push` adds 0 bytes for a blank line, so the eviction loop never runs and 100,000 pushes keep 100,000 lines. With a blank last line, `error_line` gives an empty reason where the line before said "Conversion failed!". `cut` and the no-reason wording are also written twice.

**Fix:** Fix both in one change to `tail.rs` and `run.rs`. Neither shipped program is known to write endless blanks; the hidden reason is what matters.

**Blocks:** nothing. **Blocked by:** nothing.

**Touches:** crates/streams/src/process/tail.rs, StderrTail::push, StderrTail::error_line; crates/streams/src/process/run.rs, Ran::reason; crates/streams/src/process/run.rs, cut, Ran::reason; crates/streams/src/process/tail.rs, cut_bytes; crates/streams/src/process/mod.rs, NO_REASON.

**Findings:**

- `13-1`: confirmed, P2. Blank lines of standard error count for nothing in the tail: memory is unbounded and a blank last line hides the reason. Full text in `chunks/13.md`, verdict in `verdicts/B08.md`.
- `13-5`: confirmed, P3. Two copies of cutting text at a character boundary, and the no-reason wording written twice. Full text in `chunks/13.md`, verdict in `verdicts/B08.md`.

### 64. No test covers control characters in names or a part file that cannot be created

P2, small, fixed.

A mutant that removes the control-character arm of `clean` survives, so nothing would notice `a%0Ab.txt` reaching the disk. Another mutant that treats every error as a taken name in `create_free_part` also survives, and would hang the download. The code is right today.

**Fix:** Add the two tests: a control character, and a missing folder expecting `NotFound`.

**Blocks:** nothing. **Blocked by:** nothing.

**Touches:** crates/engine/src/naming.rs, clean, and through it sanitize and is_safe_name; crates/engine/src/naming.rs, create_free_part.

**Findings:**

- `M-1`: real gap, P2. No test feeds a control character to clean. Full text in `verdicts/mutation.md`, verdict in `verdicts/mutation.md`.
- `M-3`: real gap, P3. No test makes create_free_part fail. Full text in `verdicts/mutation.md`, verdict in `verdicts/mutation.md`.

### 65. Ctrl+C in the CLI can exit with the wrong code or abort

P2, small, fixed.

`main` applies `?` to `download`'s result, so Ctrl+C before the server answers exits 1 with an error, not 130. `eprintln!` comes before `cancel.cancel()` in the signal task, so a closed stderr panics and, with `panic = "abort"`, aborts instead of pausing.

**Fix:** Map the cancel at the end of `main`, and cancel before printing with a write that cannot panic.

**Blocks:** nothing. **Blocked by:** nothing.

**Touches:** crates/cli/src/main.rs, main (result?), module doc; crates/cli/src/main.rs, stop_on_ctrl_c.

**Findings:**

- `32-2`: confirmed, P2. Ctrl+C before the server answers exits 1, with an error, not 130. Full text in `chunks/32.md`, verdict in `verdicts/B03.md`.
- `32-3`: confirmed, P2. Ctrl+C with a closed stderr pipe panics before it cancels, so the process aborts instead of pausing. Full text in `chunks/32.md`, verdict in `verdicts/B03.md`.

### 66. Keep existing, Replace and captures leave the wrong category on the row

P2, small, open.

`settle_at` and `replace_at` do not carry the category known so far, and `add_entry` does not keep the wanted one. So Keep existing and Replace leave the wrong category on the row, and a direct media file the browser called audio goes to the wrong folder.

**Fix:** Carry what is known about the category through to settling.

**Blocks:** nothing. **Blocked by:** nothing.

**Touches:** crates/core/src/manager/file_exists.rs, keep_existing and replace_at; crates/core/src/manager/captures.rs, direct_category, wanted_by; crates/core/src/manager/settle.rs, settle_path_checking.

**Findings:**

- `05-11`: confirmed, P2. Keep existing and Replace leave the wrong category on the row. Full text in `chunks/05.md`, verdict in `verdicts/B01.md`.
- `07-4`: confirmed, P3. The browser's word that a direct media file is audio is dropped when the server answers. Full text in `chunks/07.md`, verdict in `verdicts/B01.md`.

### 67. A finished media file is not flushed before its sources are deleted

P2, small, fixed.

No `sync_all` appears in the path after `ffmpeg::run`, `merge::join` or yt-dlp, and `finish` and `join` delete the sources at once. A power cut can lose both the output and its sources. The engine and `concat` do flush. Not run, since it needs a power cut.

**Fix:** Flush the output before deleting its sources. `Xsecurity-addon-unpacking-S2` asks the same for add-on files.

**Blocks:** nothing. **Blocked by:** nothing.

**Touches:** crates/core/src/manager/media_run/placement.rs, finish and place; crates/streams/src/download/join.rs, mux and join.

**Findings:**

- `Xsecond-review-disk-writes-3`: confirmed, P2. A media file made by ffmpeg or yt-dlp is put in place with its sources deleted, never flushed. Full text in `cross/second-review-disk-writes.md`, verdict in `verdicts/B06.md`.
- `Xsecond-review-resume-7`: confirmed, P2. A finished video is never flushed to the disk before the parts it came from are deleted. Full text in `cross/second-review-resume.md`, verdict in `verdicts/B06.md`.

### 68. Leftover proxy code from the earlier form has no caller

P3, small, fixed.

`updater_proxy` and `UpdaterProxy` have no caller; updates take their proxy from `update_options` through `app_proxy`, so a reader changes the wrong one. `test`'s handling of several proxies and of no proxy is unreachable, kept alive only by its own test, and its doc still describes D71.5's form.

**Fix:** Remove them, and the test that keeps them.

**Blocks:** item 69. **Blocked by:** nothing.

**Touches:** crates/core/src/proxy.rs test, and its test the_test_goes_through_every_proxy_set_whatever_the_exceptions; crates/settings/src/proxy.rs, updater_proxy and UpdaterProxy; crates/core/src/manager/proxy.rs, Manager::updater_proxy; the re-export in crates/core/src/lib.rs; crates/core/src/manager/proxy.rs, updater_proxy; crates/settings/src/proxy.rs, updater_proxy.

**Findings:**

- `19-8`: confirmed, P3. The Test connection's handling of several proxies and of no proxy is never reached. Full text in `chunks/19.md`, verdict in `verdicts/B15.md`.
- `Xrepeated-logic-7`: confirmed, P3. updater_proxy is a third copy of "which proxy for https", and nothing uses it. Full text in `cross/repeated-logic.md`, verdict in `verdicts/B15.md`.
- `B05-2`: confirmed, P3. Manager::updater_proxy has no caller. Full text in `verdicts/B05.md`, verdict in `verdicts/new-findings.md`.

### 69. yt-dlp ignores proxy exceptions and the page's scheme

P2, small, fixed.

`ytdlp_proxy` reads only the proxies, and the callers pass no page. So yt-dlp goes through the proxy for a host on the exceptions while the row says "No proxy", and takes the HTTP proxy for an HTTPS page the engine reaches directly (`B15-1`).

**Fix:** Ask `ProxyChoice::route` for the page. `Some("")` already gives yt-dlp no proxy, so the limit is not yt-dlp's.

**Blocks:** nothing. **Blocked by:** item 68.

**Touches:** crates/settings/src/proxy.rs ytdlp_proxy, its callers crates/core/src/manager/media.rs and crates/core/src/manager/media_run/page.rs, label from State::proxy_route; crates/settings/src/proxy.rs, ytdlp_proxy.

**Findings:**

- `19-4`: confirmed, P2. yt-dlp goes through the proxy for a host on the exceptions, while the download says it used no proxy. Full text in `chunks/19.md`, verdict in `verdicts/B15.md`.
- `B15-1`: confirmed, P3. yt-dlp takes the HTTP proxy for an HTTPS page that the engine reaches directly. Full text in `verdicts/B15.md`, verdict in `verdicts/new-findings.md`.

### 70. Choosing a path blocks an async thread on the disk, so a dead share stalls every command

P2, medium, fixed.

`Destination::choose` calls the resolver directly and documents that it must not block, but `settle_path` and `found_existing` touch the disk there. An unreachable share would stall the runtime that serves every command. The blocking is certain; the stall was not measured.

**Fix:** Run the disk work off the async thread.

**Blocks:** nothing. **Blocked by:** nothing.

**Touches:** crates/core/src/manager/settle.rs, settle_path and settle_path_checking; crates/core/src/manager/file_exists.rs, found_existing; crates/core/src/manager/settle.rs, settle_path, settle_path_checking; crates/core/src/manager/file_exists.rs, found_existing, saved_at.

**Findings:**

- `05-9`: confirmed, P2. The path resolver blocks a runtime thread on the disk. Full text in `chunks/05.md`, verdict in `verdicts/B01.md`.
- `Xconcurrency-5`: confirmed, P2. Choosing a path from the server's answer blocks an async thread on the disk. Full text in `cross/concurrency.md`, verdict in `verdicts/B01.md`.

### 71. A passing read error or a failed rename deletes a download's data

P2, medium, open.

`saved_state` and `part_len` answer `None` to any read error, so `plan_exact` plans a fresh start and `prepare` deletes the part file. D30.3 says set aside, and on an exact path it deletes. One failed final rename also fails a complete download, and a server without ranges then fetches it all again. `WorkDir::open` clears the folder on any read error of `stream.txt`.

**Fix:** Delete only on proof the data is wrong, and retry the rename. `Xsecond-review-resume-S2` suggests finishing a complete part file without asking the server.

**Blocks:** nothing. **Blocked by:** nothing.

**Touches:** crates/engine/src/target.rs, saved_state, part_len, plan_exact and Destination::prepare; crates/streams/src/workdir.rs, WorkDir::open; crates/engine/src/storage.rs, finalize; crates/engine/src/target.rs, continues and Destination::prepare.

**Findings:**

- `Xsecond-review-resume-4`: confirmed, P2. A passing read error on a resume file or stream header deletes everything already downloaded. Full text in `cross/second-review-resume.md`, verdict in `verdicts/B03.md`.
- `Xsecond-review-disk-writes-5`: confirmed, P2. One failed final rename fails a complete download, and a server without ranges then downloads it all again. Full text in `cross/second-review-disk-writes.md`, verdict in `verdicts/B03.md`.

### 72. The app quits before the Windows setup has asked for approval, so a refusal reaches no one

P2, medium, fixed.

D111.4, the comment on `run_setup` and PACKAGING.md say the shell asks for approval, but the built setup is `asInvoker`, so `ShellExecuteW` starts it and returns success. `install` quits at once and `/SUPPRESSMSGBOXES` hides errors, so a refused prompt or a failed setup leaves the app closed with no message. Nothing is lost, and reopening works. A real prompt was not answered.

**Fix:** Correct the premise: either elevate before quitting or have the setup report back. `12-5` was disproved by the same fact.

**Blocks:** nothing. **Blocked by:** nothing.

**Touches:** crates/shell/src/updates.rs, run_setup; crates/desktop/src/app_update.rs, install; crates/desktop/packaging/windows/installer.iss; crates/shell/src/updates.rs run_setup; crates/desktop/src/app_update.rs install.

**Findings:**

- `Xpath-app-update-2`: downgraded to P2. Declining the administrator prompt, or a setup that fails, leaves the app closed with no message. Full text in `cross/path-app-update.md`, verdict in `verdicts/B05.md`.
- `Xsecurity-update-signatures-5`: confirmed, P2. The app quits before the administrator prompt is answered, so a refusal loses the failure path. Full text in `cross/security-update-signatures.md`, verdict in `verdicts/B05.md`.

### 73. An AppImage restart can be lost while the old copy exits

P2, medium, fixed.

`start_again_later` catches a launch only until `again.take()` in `lifecycle::quit`. After that the listener stays open until exit, so a new AppImage's `ShowWindow` is answered and lost, and an exit during the hand-over shows `say_already_running`. Linux only, reasoned from the code, not run.

**Fix:** Close the window between the take and the exit, using `start_again_later`.

**Blocks:** nothing. **Blocked by:** nothing.

**Touches:** crates/shell/src/updates.rs, replace_appimage; crates/desktop/src/app_update.rs, install; crates/desktop/src/lifecycle.rs, quit; crates/desktop/src/main.rs, main.

**Findings:**

- `12-3`: confirmed, P2. The AppImage restart races the old copy's exit and can end with no app running. Full text in `chunks/12.md`, verdict in `verdicts/B05.md`.
- `Xpath-app-update-3`: confirmed, P2. A replaced AppImage is started while the old copy still holds the endpoint, so the restart can be lost. Full text in `cross/path-app-update.md`, verdict in `verdicts/B05.md`.

### 74. A stored bad part can never be thrown away, so Refresh link cannot help

P2, medium, fixed.

A kept `J` track with the same fingerprint skips every fetch, `track_formats` detects the same bytes and `mux` returns `UnknownFormat`, whose fix is `RefreshLink`. `refresh_link` touches only the row and keeps the folder, so the fix cannot work and the failure loops.

**Fix:** Have `UnknownFormat` discard the bad part or track. A link digest does not fix this.

**Blocks:** nothing. **Blocked by:** nothing.

**Touches:** crates/streams/src/download/join.rs join, mux, crates/streams/src/workdir.rs WorkDir::open, crates/media/src/describe.rs unknown_format; crates/media/src/describe.rs unknown_format, stream, crates/core/src/manager/downloads.rs refresh_link, crates/core/src/manager/media_run/workdir.rs Downloaded::read, crates/core/src/manager/media_run/page.rs fetch_part.

**Findings:**

- `15-3`: confirmed, P2. After a bad track was joined, the fix the error offers cannot work. Full text in `chunks/15.md`, verdict in `verdicts/B06.md`.
- `Xpath-media-download-6` (the loop half): confirmed, P2. The Refresh link fix cannot work on media, whose parts are reused whatever the link is. Full text in `cross/path-media-download.md`, verdict in `verdicts/B06.md`.

### 75. A refused HLS or yt-dlp address fails instead of reading the page again

P2, medium, open.

A file format that is refused reads the page again; an HLS playlist or a yt-dlp download does not. One 403 on the playlist ends the row Failed, with sign-in wording. A yt-dlp 403 says "update the media add-on". `run_page`'s doc promises the refresh for any refused address.

**Fix:** One rule for which refusals read the page again. `SIGN_IN_MARKERS` already recognises `http error 403`.

**Blocks:** nothing. **Blocked by:** nothing.

**Touches:** crates/core/src/manager/media_run/page.rs, fetch_part (the Transport::Hls branch), address_gone, run_page; crates/core/src/manager/media_run/page.rs, page_attempt.

**Findings:**

- `17-5`: confirmed, P2. An HLS format whose address is refused fails at once instead of reading the page again. Full text in `chunks/17.md`, verdict in `verdicts/B07.md`.
- `B07-3`: confirmed, P2. A refused address during yt-dlp's own download does not read the page again either. Full text in `verdicts/B07.md`, verdict in `verdicts/new-findings.md`.

### 76. The import preview hides proxy and local site changes

P2, medium, open.

`ImportPreview` has no proxies or local sites, and `preview_import` drops them, while the import applies them, tests each server and may drop saved logins. "Not included" names only site passwords, though the file holds `proxy_servers`. So a person confirms changes they were not shown.

**Fix:** Add both to the preview.

**Blocks:** nothing. **Blocked by:** nothing.

**Touches:** crates/protocol/src/app.rs ImportPreview and ImportSummary, crates/core/src/manager/preferences.rs preview_import, crates/desktop/src/main_window/backup_dialog.rs; crates/desktop/src/main_window/backup_dialog.rs, INCLUDED, EXCLUDED, BackupForm::preview_body; crates/protocol/src/app.rs, ImportPreview.

**Findings:**

- `19-1`: confirmed, P2. The import preview says nothing of proxies or sites on the network, which a settings file can change. Full text in `chunks/19.md`, verdict in `verdicts/B12.md`.
- `27-2`: confirmed, P2. The import preview hides what it does to proxy servers and their passwords. Full text in `chunks/27.md`, verdict in `verdicts/B12.md`.

### 77. A missing library is not seen as a broken add-on, and can raise a system dialog

P2, medium, open.

`mark_broken` is reached only from spawn errors, but a missing library starts the process and it dies with `C0000135`. With the default error mode a system message box can instead hold the run until `STALL_TIMEOUT`, naming a DLL on the person's screen (`B12-3`). `CREATE_NO_WINDOW` itself was not shown to behave that way.

**Fix:** Treat those exit codes as a program that cannot start, and set the error mode for the children.

**Blocks:** nothing. **Blocked by:** nothing.

**Touches:** crates/media/src/describe.rs, cannot_start and ffmpeg_common; crates/media/src/ffmpeg.rs, run; crates/streams/src/process/run.rs, run; crates/streams/src/process/tree.rs (CREATE_NO_WINDOW | CREATE_SUSPENDED, no error mode).

**Findings:**

- `Xsecurity-addon-unpacking-2`: confirmed, P2. A program that dies because a library is missing does not trigger the closer look at the add-on. Full text in `cross/security-addon-unpacking.md`, verdict in `verdicts/B12.md`.
- `B12-3`: confirmed, P2. A missing library can raise a system dialog that holds the ffmpeg run. Full text in `verdicts/B12.md`, verdict in `verdicts/new-findings.md`.

### 78. A download moved to another queue shows in the wrong place

P2, medium, open.

`DownloadSummary` has no position, and `Library::apply` keeps a row at its old index. So a moved download is drawn in the wrong place until a restart, Alt+Up does nothing for a row the window holds first, and a later reorder sends the wrong line to the manager, making it real.

**Fix:** Carry the position in the summary, or refetch on a move.

**Blocks:** nothing. **Blocked by:** nothing.

**Touches:** crates/desktop/src/library.rs, Library::apply (UiEvent::DownloadsChanged); the order is read by main_window/list.rs (members, queue_line) and main_window/sort.rs; crates/desktop/src/main_window/list.rs, queue_line and lines; crates/desktop/src/library.rs, apply.

**Findings:**

- `23-1`: confirmed, P2. A download moved to another queue shows in the wrong place in that queue's line until the app restarts. Full text in `chunks/23.md`, verdict in `verdicts/B13.md`.
- `25-5`: confirmed, P2. A download moved to another queue shows at the wrong place in that queue's numbered line. Full text in `chunks/25.md`, verdict in `verdicts/B13.md`.

### 79. The window and a restart disagree with what a queue's run really is

P2, medium, open.

Whether a queue is in a run lives in a private unsaved `Gate` flag beside the saved `running_outside_schedule`. The sidebar reads `allows` and the core reads `Gate`, so at 22:30 after a run the core waits while the sidebar says "1 waiting" with no Start now. A restart drops a run unless a download was running at that moment.

**Fix:** One saved flag in `QueueStatus`, which also gives the window what it needs.

**Blocks:** item 138. **Blocked by:** nothing.

**Touches:** crates/queues/src/schedule.rs, allows; used by crates/desktop/src/main_window/sidebar.rs, outside_schedule (and through it queue_header.rs, queue_actions, and sidebar.rs, queue_menu); crates/core/src/manager/restore.rs, restore_entry; crates/queues/src/schedule.rs, Gate.

**Findings:**

- `18-2`: confirmed, P2. For a schedule with no stop time, the sidebar and the Start now button follow a different answer from the one that decides what runs. Full text in `chunks/18.md`, verdict in `verdicts/B16.md`.
- `18-3`: confirmed, P2. A restart keeps a schedule's run only if a download of the queue was running at that moment. Full text in `chunks/18.md`, verdict in `verdicts/B16.md`.

### 80. Answering the file-exists question opens a second window for the same download

P2, medium, open.

`became` and `became_download` overwrite `shows` with no look at a window already showing the download, and `window_showing` then finds only the older one. So answering the question, even with Cancel, leaves two windows on the same download. This is the default flow whenever a question window came first, and also reaches a stream that asks when placed.

**Fix:** Look for the window already showing it, as D23.5 says one window per download.

**Blocks:** nothing. **Blocked by:** nothing.

**Touches:** crates/desktop/src/download_windows/view.rs, DownloadWindow::became; crates/shell/src/download_windows.rs, Registry::became_download; crates/desktop/src/download_windows/mod.rs, wirecatch_shell::download_windows::Registry.

**Findings:**

- `24-6`: confirmed, P2. Answering "already in the folder" opens a second progress window for the same download. Full text in `chunks/24.md`, verdict in `verdicts/B17.md`.
- `U-7`: confirmed, P2. After answering the file-exists question two windows show the same download. Full text in `ui.md`, verdict in `verdicts/B17.md`.

### 81. Download windows offer only Try again, whatever fix the failure needs

P2, medium, open.

`progress_buttons` and `row_buttons` take only a status, so a 404 offers Try again where the list offers Get a new link. `Reveal::details` has no caller. D23.4 promises any error with its fix.

**Fix:** Read `error.fix` and `RETRY_FIXES`. `main_window::fixes::run` cannot simply be called from these windows, since two arms open dialogs through `super::library`.

**Blocks:** nothing. **Blocked by:** nothing.

**Touches:** crates/desktop/src/download_windows/view.rs, the Failed | Cancelled arm of the progress buttons; crates/desktop/src/download_windows/finished.rs, row_buttons; crates/protocol/src/desktop.rs, Reveal::details and Reveal.details; crates/protocol/src/app.rs, RETRY_FIXES; crates/desktop/src/download_windows/view.rs, the progress window's buttons.

**Findings:**

- `30-2`: confirmed, P2. Download windows offer only Try again and ignore a failure's fix. Full text in `chunks/30.md`, verdict in `verdicts/B17.md`.
- `Xfeature-behaviour-5`: confirmed, P2. Download windows offer only Try again, whatever fix the failure needs. Full text in `cross/feature-behaviour.md`, verdict in `verdicts/B17.md`.

### 82. Refreshing the link cannot clear the check against what the browser saw

P2, small, open.

`expect` is never cleared by a refresh, so a fresh link still fails with "The server sent a different file than the browser saw". Today a restart is the only way out, because `expect` is not saved.

**Fix:** Clear it on a refresh. This goes before the next item, which saves `expect`.

**Blocks:** item 83. **Blocked by:** nothing.

**Touches:** crates/core/src/manager/downloads.rs, refresh_link and requeue; crates/core/src/manager/settle.rs, check_expectation; crates/messages/src/lib.rs, TargetRejected.

**Findings:**

- `Xpath-normal-download-3`: confirmed, P2. Refreshing the link cannot clear "a different file than the browser saw". Full text in `cross/path-normal-download.md`, verdict in `verdicts/B02.md`.

### 83. An answered Replace and a capture's size check are lost at a restart

P2, medium, open.

`on_existing`, `expect` and `video_check` are not in the saved row, though `settled` is. So an answered Replace finishes as `report (1).pdf` after a restart with the old file kept, and a captured download finishes at 2048 bytes where the browser saw 1024. The doc on `on_existing` is wrong for a plain download.

**Fix:** Save them, together with the row record from item 4.

**Blocks:** nothing. **Blocked by:** item 4, item 82.

**Touches:** crates/core/src/manager/file_exists.rs, replace_at; crates/core/src/manager/state.rs, Entry::on_existing; crates/core/src/manager/restore.rs, restore_entry; crates/core/src/manager/settle.rs, check_expectation and read_as_video; crates/core/src/manager/state.rs, Entry.

**Findings:**

- `05-6`: confirmed, P2. An answered Replace is forgotten on restart, and the file finishes under another name. Full text in `chunks/05.md`, verdict in `verdicts/B02.md`.
- `Xcrash-and-resume-5`: confirmed, P2. After a restart, a captured download no longer checks that the server sent the file the browser saw. Full text in `cross/crash-and-resume.md`, verdict in `verdicts/B02.md`.

### 84. A write that can never pass raises a notification for every batch

P2, small, open.

`apply_one_by_one` drops what `may_pass_later` refuses, so nothing stays kept and `retried` is 0 next time. Every batch then starts a new spell: three failing batches gave three reports where one is promised. With no main window each is a system notification, and the Windows and Linux paths set no tag, so they pile up.

**Fix:** Define a spell so that a write that can never pass does not start a new one. The recovered signal in item 25 does not fix this.

**Blocks:** nothing. **Blocked by:** nothing.

**Touches:** crates/store/src/lib.rs, write_until_closed, apply_one_by_one, may_pass_later; crates/desktop/src/notices.rs, storage_failed.

**Findings:**

- `03-3`: confirmed, P2. A write that can never pass raises a system notification for every batch. Full text in `chunks/03.md`, verdict in `verdicts/B09.md`.

### 85. Choosing the folder the guess already points at is not remembered

P2, small, open.

An early return in `set_download_folder` runs before `naming.folder` is set. Picking the folder the guess already points at is not remembered, so a later server name picks another folder.

**Fix:** Set the choice before the early return.

**Blocks:** nothing. **Blocked by:** nothing.

**Touches:** crates/core/src/manager/relocate.rs, set_download_folder.

**Findings:**

- `05-8`: confirmed, P2. Choosing the folder the guess already points at is not remembered. Full text in `chunks/05.md`, verdict in `verdicts/B01.md`.

### 86. A capture joining an unfinished download is answered Started too early

P2, small, fixed.

`join_unfinished` maps `Connecting` to `Started`, which `AddAction::Started` documents as the server having answered. So the browser is told to give up its copy before any server has answered.

**Fix:** Answer from what the status really knows.

**Blocks:** nothing. **Blocked by:** nothing.

**Touches:** crates/core/src/manager/captures.rs, join_unfinished; crates/core/src/manager/downloads.rs, capture_action.

**Findings:**

- `07-2`: confirmed, P2. A capture joining an unfinished download is answered Started before any server has answered. Full text in `chunks/07.md`, verdict in `verdicts/B01.md`.

### 87. Skipped bytes are not counted against the speed limit

P2, small, open.

In `stream` and `Fetcher::attempt`, `data.is_empty()` continues before `pace`, and only kept bytes count. A retry that skips a large part runs with no speed limit, against the `Fetcher` doc's "Every byte counts".

**Fix:** Pace before the skip. This has no shared cause with the truncation in item 3.

**Blocks:** nothing. **Blocked by:** nothing.

**Touches:** crates/engine/src/fetch.rs, Fetcher::attempt.

**Findings:**

- `02-3`: confirmed, P2. Fetcher does not count skipped bytes against the speed limit, though its docs say every byte counts. Full text in `chunks/02.md`, verdict in `verdicts/B03.md`.

### 88. A stale resume file that could not be deleted makes zeros count as downloaded

P2, small, open.

`prepare` runs `discard(part)`, an unchecked `discard(state)`, then `create_part`, which gives a full-length file of zeros. If the first save also fails on its rename, the old resume file passes every check of the next run and its done ranges stay zeros. It takes two faults together.

**Fix:** Do not go on when the old resume file could not be deleted.

**Blocks:** nothing. **Blocked by:** nothing.

**Touches:** crates/engine/src/target.rs, Destination::prepare and discard.

**Findings:**

- `Xsecond-review-resume-5`: confirmed, P2. Zeros are trusted as downloaded when a stale resume file could not be deleted. Full text in `cross/second-review-resume.md`, verdict in `verdicts/B03.md`.

### 89. A resume file is rewritten every second even when nothing changed

P2, small, open.

`save` has no comparison with the last saved ranges, so a stalled download rewrites and flushes its resume file every second. D30.1 covers a flush per interval only for a download making progress.

**Fix:** Skip the save when nothing changed.

**Blocks:** nothing. **Blocked by:** nothing.

**Touches:** crates/engine/src/checkpoint.rs, ResumeSaver::save.

**Findings:**

- `01-4`: confirmed, P2. A resume file is rewritten and flushed every second even when nothing changed. Full text in `chunks/01.md`, verdict in `verdicts/B03.md`.

### 90. A valid short partial answer is waited out as a failed connection

P2, small, open.

A server capping each answer sends a valid short `206`, and `stream` compares only with what it asked for, so the clean end is `PrematureEof`. Each one waits out the first backoff: 16 MiB at a 256 KiB cap took 57 waits and 5.17 s against about 55 ms. The file is right.

**Fix:** Read the end from the answer's own `Content-Range`. `proofs/FT-sizes.patch` holds the harness, which the plan turns into a permanent test.

**Blocks:** nothing. **Blocked by:** nothing.

**Touches:** crates/engine/src/worker.rs, stream and run_assignment.

**Findings:**

- `F-2`: confirmed, P2. A valid short partial answer is waited out as a failed connection. Full text in `fault-tests.md`, verdict in `verdicts/B03.md`.

### 91. CLI speed and ETA are wrong for half a minute after a resume

P2, small, fixed.

indicatif's estimator ignores a position of 0, so the first non-zero `set_position` after a resume counts the whole kept size as one sample. Speed and ETA are wrong for about half a minute. The shape is read from indicatif 0.18.6; the figures in the finding were not measured.

**Fix:** Set the starting position when the bar is made, or call `reset_eta`.

**Blocks:** nothing. **Blocked by:** nothing.

**Touches:** crates/cli/src/main.rs, spawn_progress_bar.

**Findings:**

- `32-4`: confirmed, P2. After a resume, the shown speed and ETA are wrong for about half a minute. Full text in `chunks/32.md`, verdict in `verdicts/B03.md`.

### 92. A command-line argument that is not valid Unicode aborts the app

P2, small, fixed.

`std::env::args()` in `main` panics on an argument that is not valid Unicode, and the release profile aborts. The Linux entry's `Exec=... %U` does pass outside arguments. A running copy is not affected.

**Fix:** Use `args_os`.

**Blocks:** nothing. **Blocked by:** nothing.

**Check to add:** A `disallowed-methods` entry for `std::env::args`.

**Touches:** crates/desktop/src/main.rs (main).

**Findings:**

- `22-4`: confirmed, P2. A command-line argument that is not valid Unicode aborts the app at start. Full text in `chunks/22.md`, verdict in `verdicts/B04.md`.

### 93. Uninstalling while the app runs leaves its program behind

P2, small, open.

`installer.iss` has no `AppMutex`, no `uninsrestartdelete` and no `[UninstallRun]`, and `CloseApplications=force` covers Setup only. So uninstalling while the app runs leaves its program in Program Files. Not run.

**Fix:** Make the uninstaller close the app or mark the file for deletion.

**Blocks:** nothing. **Blocked by:** nothing.

**Touches:** crates/desktop/packaging/windows/installer.iss, [Setup], [Files] and CurUninstallStepChanged.

**Findings:**

- `29-1`: confirmed, P2. Uninstalling on Windows while the app runs leaves its program in Program Files. Full text in `chunks/29.md`, verdict in `verdicts/B05.md`.

### 94. The AppImage's library folder leaks into programs the app starts

P2, small, open.

`appRun` exports `LD_LIBRARY_PATH` and nothing removes it, so what `xdg-open` starts inherits the bundle's libraries. A program needing a newer libxkbcommon would fail, which is plausible on a rolling distribution. It is harmless for yt-dlp and ffmpeg. Not run.

**Fix:** Remove it from the environment of programs the app starts.

**Blocks:** nothing. **Blocked by:** nothing.

**Touches:** scripts/package.mjs, appRun.

**Findings:**

- `29-2`: confirmed, P2. The AppImage's library folder leaks into every program the app starts. Full text in `chunks/29.md`, verdict in `verdicts/B05.md`.

### 95. A video's title is cut at its last slash

P2, small, fixed.

The title reaches `sanitize`, which treats it as a server's path and cuts at the last slash, so a title loses what comes before it. `clean` is right for a server's name.

**Fix:** Replace separators in a title before `sanitize`.

**Blocks:** nothing. **Blocked by:** nothing.

**Touches:** crates/media/src/lib.rs, file_name.

**Findings:**

- `16-2`: confirmed, P2. A video's title is cut at its last slash when it becomes the file name. Full text in `chunks/16.md`, verdict in `verdicts/B07.md`.

### 96. yt-dlp progress goes backwards for a video with separate audio

P2, small, open.

yt-dlp's counts are per format, never the sum, and `set_own` replaces the last report. So a video with separate audio shows the bar going backwards when the audio starts.

**Fix:** Add finished formats to the running count.

**Blocks:** nothing. **Blocked by:** nothing.

**Touches:** crates/media/src/lib.rs, MediaProgress::set_own; crates/media/src/ytdlp/download.rs, download_once.

**Findings:**

- `16-3`: confirmed, P2. A video and audio downloaded by yt-dlp show progress going backwards. Full text in `chunks/16.md`, verdict in `verdicts/B07.md`.

### 97. A quality says 60 fps but may download 30 fps

P2, small, open.

A quality row labelled "1080p, 60 fps" takes its frame rate from another format and downloads `137+140`, the 30 fps MP4. It needs codecs of one height to differ in frame rate.

**Fix:** Label from the format that will download, or pick the format the label names.

**Blocks:** nothing. **Blocked by:** nothing.

**Touches:** crates/media/src/choice.rs, video_offer.

**Findings:**

- `16-4`: confirmed, P2. A quality says "60 fps" although the format it downloads may be 30 fps. Full text in `chunks/16.md`, verdict in `verdicts/B07.md`.

### 98. Speed limits do not reach yt-dlp

P2, small, open.

`Context` has no limit and `download_args` no `--limit-rate`, so limits reach only the engine and streams. No decision records an exception. A yt-dlp process takes one fixed rate, so it cannot share the queue's limiter exactly.

**Fix:** Pass a rate at start, and decide what a shared limit becomes.

**Blocks:** nothing. **Blocked by:** nothing.

**Touches:** crates/core/src/manager/media_run/page.rs, page_attempt; crates/media/src/ytdlp/download.rs, download_args; ytdlp::Context.

**Findings:**

- `17-4`: confirmed, P2. Speed limits do not apply when yt-dlp downloads a format itself. Full text in `chunks/17.md`, verdict in `verdicts/B07.md`.

### 99. The best audio is picked by rate alone, so a dubbed track can replace the original

P2, small, open.

`Format` reads no `language` or `language_preference`, and `best_audio` compares rate alone. A dubbed or described track at 130 beats the original at 129, and the person gets the wrong language with no choice. yt-dlp's own sort uses these fields first. Not run against a real multi-language page.

**Fix:** Read the fields and prefer the original.

**Blocks:** nothing. **Blocked by:** nothing.

**Touches:** crates/media/src/output/variants.rs, best_by_rate; crates/media/src/choice.rs, best_audio; crates/media/src/ytdlp/info.rs, Format.

**Findings:**

- `B07-2`: confirmed, P2. The best audio is picked by rate alone, so a dubbed or described track can replace the original. Full text in `verdicts/B07.md`, verdict in `verdicts/new-findings.md`.

### 100. HLS key addresses count against no memory budget

P2, small, open.

`update_keys` joins with no budget, and each line makes a new `Arc<Url>` kept by each piece. 8 MiB of playlist can hold about 1.5 GiB. Read only; the sizes follow from the constants.

**Fix:** Charge key addresses to `AddressBudget`.

**Blocks:** nothing. **Blocked by:** nothing.

**Touches:** crates/streams/src/hls.rs, update_keys, encryption and MediaReader::segment.

**Findings:**

- `14-3`: confirmed, P2. HLS key addresses count against no address budget, so 8 MiB of playlist can hold about 1.5 GiB. Full text in `chunks/14.md`, verdict in `verdicts/B08.md`.

### 101. Automatic audio picks another language when the default is inside the video

P2, small, open.

`paired_audio` filters on `r.url.is_some()` before looking for `default`, so an English `DEFAULT=YES` rendition with no `URI`, which RFC 8216 says is in the variant, is never seen and French is picked. A download without ffmpeg then fails with `NeedsFfmpeg`.

**Fix:** Look for the default first.

**Blocks:** nothing. **Blocked by:** nothing.

**Touches:** crates/streams/src/select.rs, paired_audio.

**Findings:**

- `14-4`: confirmed, P2. Automatic audio picks another language when the default audio is inside the video. Full text in `chunks/14.md`, verdict in `verdicts/B08.md`.

### 102. A subtitle codec turns an MP4-ready video into MKV

P2, small, open.

`Codec::from_name("wvtt")` is `None`, read as Matroska only, so `avc1,mp4a,wvtt` gives `mkv` and `mp4a,wvtt` with no resolution is listed as a video. The file still plays.

**Fix:** Know subtitle codecs in `Codec`.

**Blocks:** nothing. **Blocked by:** nothing.

**Touches:** crates/streams/src/codec.rs, Codec::list and Container::for_copy; crates/streams/src/hls.rs, Variant::audio_only.

**Findings:**

- `14-5`: confirmed, P2. A subtitle codec in a variant's CODECS turns an MP4-ready video into MKV and hides an audio-only variant. Full text in `chunks/14.md`, verdict in `verdicts/B08.md`.

### 103. While the list loads, the window says nothing was ever downloaded

P2, small, open.

`empty` tests only `Loaded::Failed`, and `list` calls it whenever there are no lines, also while loading. So the first-run text shows during the integrity check, the lock wait or a data folder move. Commands pressed meanwhile wait and work.

**Fix:** Add a loading branch.

**Blocks:** nothing. **Blocked by:** nothing.

**Touches:** crates/desktop/src/main_window/empty.rs, empty.

**Findings:**

- `25-9`: confirmed, P2. While the list is loading, the window says nothing was ever downloaded. Full text in `chunks/25.md`, verdict in `verdicts/B09.md`.

### 104. Moving a file to another drive drops its Mark of the Web

P2, small, fixed.

`move_file` across drives copies the bytes only, so the moved file has no `Zone.Identifier`. A same-drive move keeps it, because it is a rename.

**Fix:** Carry the mark over in the staged copy from item 8, which does not fix this by itself.

**Blocks:** nothing. **Blocked by:** nothing.

**Touches:** crates/core/src/manager/relocate.rs, move_file.

**Findings:**

- `05-4`: confirmed, P2. Moving a finished file to another drive drops its Mark of the Web. Full text in `chunks/05.md`, verdict in `verdicts/B10.md`.

### 105. Replacing a file another program has open blames the folder

P2, small, open.

`disk_failure` decides from the error kind alone. A rename over a file held open gives `PermissionDenied`, so the person reads "Wirecatch is not allowed to save files in <folder>. Choose another folder." with `ChooseFolder`. Any held file reaches the same arm.

**Fix:** Use `action` and `path`, which reach only the `what` text today.

**Blocks:** nothing. **Blocked by:** nothing.

**Touches:** crates/messages/src/lib.rs, disk_failure (the PermissionDenied arm), reached from describe for EngineError::Io.

**Findings:**

- `28-2`: confirmed, P2. Replacing a file that another program has open blames the folder and offers the wrong fix. Full text in `chunks/28.md`, verdict in `verdicts/B11.md`.

### 106. A stream manifest over the limit offers a retry that cannot help

P2, small, open.

`Source::load` and `media_playlist` reach `describe`'s catch-all through `map_engine`, so a manifest over the limit offers a retry that cannot help.

**Fix:** Add an arm for `TooLarge`.

**Blocks:** nothing. **Blocked by:** nothing.

**Touches:** crates/messages/src/lib.rs, describe (the other arm, reached with EngineError::TooLarge); caller crates/media/src/describe.rs, E::Request.

**Findings:**

- `28-4`: confirmed, P2. A stream manifest over the size limit falls into the catch-all and offers a retry that cannot help. Full text in `chunks/28.md`, verdict in `verdicts/B11.md`.

### 107. A suffix list that could not be saved blames the internet connection

P2, small, open.

`advice` answers "check your internet connection" for a failed `whole_file::replace`, an HTTP answer from the host and a failed blocking task. The real reason does show in brackets.

**Fix:** Split `Failed`.

**Blocks:** nothing. **Blocked by:** nothing.

**Touches:** crates/core/src/manager/suffix_list.rs, NotTaken::advice and Manager::newer_suffix_list.

**Findings:**

- `10-3`: confirmed, P2. A list that could not be saved tells the person to check their internet connection. Full text in `chunks/10.md`, verdict in `verdicts/B11.md`.

### 108. An add-on repair downloads everything and puts yt-dlp back to the pinned version

P2, small, fixed.

`addon_plan` returns every part when `tools` is `None`, and `wanted_components` gives the pin. `swap_in` replaces with no version check, so a repair goes back to the pinned yt-dlp, against D21.1, and the next check is up to 24 hours off.

**Fix:** Apply `outdated` there too. `Xsecurity-addon-unpacking-S1` suggests repairing only what is damaged.

**Blocks:** item 109. **Blocked by:** nothing.

**Touches:** crates/core/src/manager/addons/mod.rs, Manager::addon_plan and Manager::wanted_components; crates/core/src/manager/addons/install.rs, swap_in.

**Findings:**

- `11-1`: confirmed, P2. A repair downloads every part again and puts yt-dlp back to the pinned version. Full text in `chunks/11.md`, verdict in `verdicts/B12.md`.

### 109. The add-on check acts on what it read before its wait, so a removed add-on comes back

P2, small, fixed.

`record` is cloned before `new_ytdlp_releases().await`, and `save` runs `create_dir_all` afterwards. So a removed add-on comes back through a quiet install, and a finished update's record is overwritten. The window is short.

**Fix:** Reread the record after the wait.

**Blocks:** nothing. **Blocked by:** item 108.

**Touches:** crates/core/src/manager/addons/updates.rs, Manager::check_addon_updates.

**Findings:**

- `11-2`: confirmed, P2. The update check acts on what it read before its network wait: a removed add-on comes back, and a finished update's record is overwritten. Full text in `chunks/11.md`, verdict in `verdicts/B12.md`.

### 110. One bad yt-dlp release stops the check, and a failed check forgets its offers

P2, small, fixed.

`?` leaves the loop at the first candidate that fails, and every `Err` arm gives `Vec::new()`, which replaces `ytdlp_releases`. So one bad newest release holds updates up, and one offline check makes the offer disappear. No downgrade follows.

**Fix:** Skip a bad release, and keep the list on a failed check.

**Blocks:** nothing. **Blocked by:** nothing.

**Touches:** crates/core/src/manager/addons/updates.rs, new_ytdlp_releases and check_addon_updates.

**Findings:**

- `Xsecurity-addon-unpacking-3`: confirmed, P2. One bad newest release stops the check from offering an older good one, and any failed check forgets what was already found. Full text in `cross/security-addon-unpacking.md`, verdict in `verdicts/B12.md`.

### 111. Cancel on a selection stops at a finished download

P2, small, open.

`ending` passes every id, `Manager::cancel` returns `WrongState` for a finished row, and `try_for_each` stops there, so later downloads keep running. Every other selection command goes through `selection::each`.

**Fix:** Use `selection::each`, and filter finished rows.

**Blocks:** nothing. **Blocked by:** nothing.

**Touches:** crates/desktop/src/main_window/confirm.rs, cancel; crates/desktop/src/main_window/row_menu.rs, ending.

**Findings:**

- `25-3`: confirmed, P2. Cancel on a selection holding a finished download stops at it, and later downloads keep running. Full text in `chunks/25.md`, verdict in `verdicts/B13.md`.

### 112. The drawer stays open over a page opened from its foot

P2, small, open.

`open_screen` and `open_settings` leave `drawer` set, and `render` hides it only on Settings. The minimum window (960) is under `NARROW` (1100). F1 and Ctrl+, reach the same state.

**Fix:** Close the drawer when a page opens from it.

**Blocks:** nothing. **Blocked by:** nothing.

**Touches:** crates/desktop/src/main_window/mod.rs, open_screen; crates/desktop/src/main_window/sidebar.rs, foot, foot_icon; crates/desktop/src/main_window/rail.rs, rail_foot_item.

**Findings:**

- `25-7`: confirmed, P2. The drawer stays open over a page opened from its foot, and comes back after Settings. Full text in `chunks/25.md`, verdict in `verdicts/B13.md`.

### 113. Change queue inside the details opens the same details again

P2, small, open.

`failure` offers every fix except `Retry`, `fixes::run` answers `ChangeQueue` with `details::open`, and `open_dialog` adds another dialog. So the fix inside the details opens the same details.

**Fix:** Do not offer it there, or go to the queue field directly.

**Blocks:** nothing. **Blocked by:** nothing.

**Touches:** crates/desktop/src/main_window/fixes.rs, run (FixAction::ChangeQueue); crates/desktop/src/main_window/details.rs, failure.

**Findings:**

- `25-8`: confirmed, P2. The Change queue fix inside the details opens the same details again. Full text in `chunks/25.md`, verdict in `verdicts/B13.md`.

### 114. A page address that is not a web address counts as on the person's network

P2, small, open.

`of_capture` only requires `Url::parse` to succeed. `chrome-extension://abcdef/x` has the opaque host `abcdef`, read as a single label off the list, so the capture is `LocalPage` and a link to a local address gets through. `cookie_scope::site_of` refuses those schemes next door.

**Fix:** Check the scheme in `of_capture`.

**Blocks:** nothing. **Blocked by:** nothing.

**Touches:** crates/sites/src/local_network.rs, ChosenBy::of_capture and is_local_by_itself.

**Findings:**

- `10-1`: confirmed, P2. A page address that is not a web address counts as a page on the person's network. Full text in `chunks/10.md`, verdict in `verdicts/B14.md`.

### 115. A Windows socks= proxy fails on sites whose first address is IPv6

P2, small, fixed.

`adopt` writes `socks4://`, reqwest takes the first resolved address, and an IPv6 one returns `SocksV4Error::IpV6` with no fallback. Getting IPv6 first depends on the machine and was not run.

**Fix:** Filter to IPv4 for SOCKS4.

**Blocks:** nothing. **Blocked by:** nothing.

**Touches:** crates/engine/src/proxy.rs, Proxies::adopt (the socks4:// value), with reqwest connect::socks::connect.

**Findings:**

- `09-2`: confirmed, P2. A Windows socks= proxy fails on sites whose first address is IPv6. Full text in `chunks/09.md`, verdict in `verdicts/B15.md`.

### 116. The proxy form says a saved password is kept, then forgets it

P2, small, open.

The placeholder is fixed in `ProxyForm::new`, while `merge_password` answers `None` when `same_proxy` is false and removes the login with no message. D71.2's rule is deliberate and stays: the form promises the opposite.

**Fix:** Make the form say what will happen.

**Blocks:** nothing. **Blocked by:** nothing.

**Touches:** crates/desktop/src/main_window/proxy_dialog.rs, ProxyForm::new (password placeholder), ProxyForm::draft, ProxyForm::login_entered.

**Findings:**

- `27-1`: confirmed, P2. The proxy form says a saved password is kept, then saving forgets it. Full text in `chunks/27.md`, verdict in `verdicts/B15.md`.

### 117. A download that fails to start leaves its slot empty

P2, small, fixed.

`reconcile` computes `plan.start` once, and `fail_to_start` only saves. So a download that fails to start leaves its slot empty and an unscheduled queue stays stopped until an unrelated command.

**Fix:** Plan again after a failed start.

**Blocks:** nothing. **Blocked by:** nothing.

**Touches:** crates/core/src/manager/run.rs, start_run and fail_to_start; crates/core/src/manager/scheduling.rs, reconcile.

**Findings:**

- `04-2`: confirmed, P2. A download that fails to start leaves its slot empty and the next one waits. Full text in `chunks/04.md`, verdict in `verdicts/B16.md`.

### 118. A closing queue's slots are given away at once, exceeding the limit

P2, small, fixed.

A `Running` download found `over` is pushed to `stop` without counting, so the fill loop sees the room free, against `plan`'s doc. It lasts until the stops finish, at most 5 s. It also holds for a lowered queue limit, which the finding says it does not.

**Fix:** Count a stopping download until it stops.

**Blocks:** nothing. **Blocked by:** nothing.

**Touches:** crates/queues/src/plan.rs, plan.

**Findings:**

- `18-4`: confirmed, P2. When a queue closes, its slots are given away in the same plan, so the app's limit is exceeded until those downloads stop. Full text in `chunks/18.md`, verdict in `verdicts/B16.md`.

### 119. Regular expressions differing only in case count as one rule

P2, small, fixed.

`cleaned` uses `seen.insert(read.to_lowercase())` for every kind of rule, so `\d+\.mkv` and `\D+\.mkv` have one key and the second is dropped with no message.

**Fix:** Compare regular expressions as written.

**Blocks:** nothing. **Blocked by:** nothing.

**Touches:** crates/queues/src/rules.rs, cleaned.

**Findings:**

- `18-5`: confirmed, P2. Regular expressions that differ only in case are treated as the same rule, and the second is dropped without a word. Full text in `chunks/18.md`, verdict in `verdicts/B16.md`.

### 120. Enter in the delete-queue field closes the question and deletes nothing

P2, small, open.

`ask` installs no `on_ok`, the input propagates Enter, and the dialog's default handler closes. So Enter closes the question and deletes nothing; only the typed name is lost.

**Fix:** Give the dialog an `on_ok`.

**Blocks:** nothing. **Blocked by:** nothing.

**Touches:** crates/desktop/src/main_window/queue_delete.rs, ask.

**Findings:**

- `27-3`: confirmed, P2. Enter in the delete queue field closes the question and deletes nothing. Full text in `chunks/27.md`, verdict in `verdicts/B16.md`.

### 121. Saving a queue changes its speed limit

P2, small, open.

`read` always sends the field's text through `speed_limit`, and `in_unit` rounds to a tenth of the unit. 1075 KB/s shows "1" and saves 1,048,576 bytes. The drift is at most 0.05 MB/s and does not grow on later saves.

**Fix:** Write only what the person changed.

**Blocks:** nothing. **Blocked by:** nothing.

**Touches:** crates/desktop/src/main_window/queue_dialog.rs, in_unit, speed_limit, QueueForm::new, QueueForm::set_unit.

**Findings:**

- `27-5`: confirmed, P2. Saving a queue for any reason changes its speed limit. Full text in `chunks/27.md`, verdict in `verdicts/B16.md`.

### 122. Confirming a file unedited stops the server from naming it

P2, small, open.

`confirm` sends `file_name: Some(field)` without comparing it to the proposal, and `settled_name` returns a `Some` choice first. So confirming unedited stops the server naming the file. The claim that a refused proposal fails the download is wrong.

**Fix:** Send `None` when unchanged; `hint_name` already carries the proposal.

**Blocks:** nothing. **Blocked by:** nothing.

**Touches:** crates/desktop/src/download_windows/view.rs, DownloadWindow::confirm.

**Findings:**

- `24-5`: confirmed, P2. Confirming a file without editing its name stops the server from naming it. Full text in `chunks/24.md`, verdict in `verdicts/B17.md`.

### 123. A failure notice in the browser loses its buttons after 32 later notices

P2, small, open.

`register` pops the front of one `kept` queue for every kind, and a run of captures reaches 32. An open failure notice then loses its buttons, and `pressed` drops the click silently. The bound is documented, tested for a single kind only.

**Fix:** Keep kinds apart, as `system_notifications.rs` does for brief toasts.

**Blocks:** nothing. **Blocked by:** nothing.

**Touches:** crates/shell/src/browser_notices.rs, KEPT_NOTICES and BrowserNotices::register, ask, pressed, outdated_by.

**Findings:**

- `26-1`: confirmed, P2. An open failure notice in the browser loses its buttons after 32 later notices of any kind. Full text in `chunks/26.md`, verdict in `verdicts/B17.md`.

### 124. A tray action that fails tells the person nothing

P2, small, open.

Every `Err` in `on_click` and the `None` of `open_folder` only log, though `perform`'s docs promise an explanation. `manager` is also `None` after a failed open, when "still starting" is untrue.

**Fix:** Use `could_not` from `notices.rs`.

**Blocks:** nothing. **Blocked by:** nothing.

**Touches:** crates/desktop/src/tray.rs, on_click, AppShell::open_folder; crates/shell/src/tray.rs, perform.

**Findings:**

- `26-4`: confirmed, P2. A tray action that fails tells the person nothing. Full text in `chunks/26.md`, verdict in `verdicts/B17.md`.

### 125. An unfinished download reads 100%

P2, small, open.

The `row.done >= total` branch of `percent` contradicts its doc, and it is reachable: `figure` says 100% beside "Converting, 40%". A file whose real size passes its expected size also reads 100% while downloading.

**Fix:** Remove the branch, so the cap below it works.

**Blocks:** nothing. **Blocked by:** nothing.

**Touches:** crates/desktop/src/text.rs, percent; shown by main_window/list.rs (figure) and main_window/details.rs.

**Findings:**

- `23-3`: confirmed, P2. An unfinished download reads 100%, against the function's own contract, including while it merges or converts. Full text in `chunks/23.md`, verdict in `verdicts/B18.md`.

### 126. A finished download whose file is gone gets advice it cannot follow

P2, small, open.

`file_to_open` tells a finished row to retry, but `can_retry` is only for failed and cancelled rows and `requeue` refuses. `item_to_reveal` says "Wirecatch creates it when the download starts" for a finished row too.

**Fix:** Word both for a finished row.

**Blocks:** nothing. **Blocked by:** nothing.

**Touches:** crates/shell/src/api.rs, file_to_open, item_to_reveal.

**Findings:**

- `23-8`: confirmed, P2. A finished download whose file or folder is gone gets advice that cannot be followed. Full text in `chunks/23.md`, verdict in `verdicts/B18.md`.

### 127. Settings, Media shows an empty Resolution after a rarer quality

P2, small, open.

`remember_media_choice` saves any height, and `QUALITIES` has six, so a rarer one shows an empty Resolution. The saved value still works.

**Fix:** Show an unlisted value as itself.

**Blocks:** nothing. **Blocked by:** nothing.

**Touches:** crates/desktop/src/main_window/settings/media.rs, QUALITIES, video; crates/core/src/manager/media.rs, remember_media_choice.

**Findings:**

- `27-6`: confirmed, P2. Settings, Media shows an empty Resolution after a less common quality is remembered. Full text in `chunks/27.md`, verdict in `verdicts/B18.md`.

### 128. The Add dialog says By file type for downloads going to a queue's folder

P2, small, open.

`folder_row` reads a folder only from `picked_queue`, so Automatic says "By file type" while `add_download` uses the placed queue's own folder. `Route` keeps only the queue's name.

**Fix:** Carry the folder in the route.

**Blocks:** nothing. **Blocked by:** nothing.

**Touches:** crates/desktop/src/add_dialog.rs, folder_row; crates/settings/src/lib.rs, folder_for.

**Findings:**

- `Xpath-normal-download-7`: confirmed, P2. The Add dialog says "By file type" for downloads going to a queue's own folder. Full text in `cross/path-normal-download.md`, verdict in `verdicts/B18.md`.

### 129. scripts/linux-test.sh can never pass

P2, small, open.

`linux-test.sh` excludes `.git`, and the gate's `cfg-allow` runs `git grep`, which exits 128 outside a repository. So the script stops at that check every time. CI runs the same group on Linux.

**Fix:** Keep what the gate needs, or run it without `git grep`.

**Blocks:** nothing. **Blocked by:** nothing.

**Touches:** scripts/linux-test.sh (the tar --exclude=./.git), against filesMatching in scripts/gate/process.mjs, reached from rustLintAttributes (cfg-allow) and testTiming (test-timing).

**Findings:**

- `33-1`: confirmed, P2. scripts/linux-test.sh can never pass: it removes .git, and the gate's Rust group needs it. Full text in `chunks/33.md`, verdict in `verdicts/B19.md`.

### 130. A second running copy of one CLI download takes over the first one's files

P2, medium, fixed.

`plan_in_folder` has no way to know a part file is in use, `discard` succeeds on an open file, and `finalize` opens by path. So two identical commands at once end with one renaming the other's file. Not run. `Xconcurrency-S1` suggests holding the part file exclusively.

**Fix:** Hold a lock on the part file during a run.

**Blocks:** nothing. **Blocked by:** nothing.

**Touches:** crates/engine/src/target.rs, plan_in_folder.

**Findings:**

- `01-2`: confirmed, P2. A second running download of one link into one folder takes over the first one's files. Full text in `chunks/01.md`, verdict in `verdicts/B03.md`.

### 131. A launch handed over while the list is opening pops a box and loses links

P2, medium, fixed.

`Backend::start` serves only after `core.opened()` while the pipe already exists, so the hello waits out `ANSWER_WAIT`, and `main` shows its box on any error. A background start pops a modal box and links are lost. D103.4 did not consider a launch with nothing to hand over. Not run.

**Fix:** Look at `launch.call` before saying hello, and retry links.

**Blocks:** nothing. **Blocked by:** nothing.

**Touches:** crates/desktop/src/main.rs (the AlreadyInUse arm), crates/shell/src/launch.rs (hand_over_on, ANSWER_WAIT), crates/desktop/src/backend.rs (serve).

**Findings:**

- `22-3`: confirmed, P2. A launch handed over while the running copy's list is still opening fails: a background start pops up a modal box, and links are lost. Full text in `chunks/22.md`, verdict in `verdicts/B04.md`.

### 132. A typed extension picks a container ffmpeg refuses, with no way out

P2, medium, open.

Original with no extras keeps `output: None`, so `joined_container` takes the typed extension. `.webm` with H.264 and AAC makes real ffmpeg exit non-zero, `merge::join` tries one container and `keep_original` needs an output. Retry repeats it.

**Fix:** Choose the container from the tracks, or fall back.

**Blocks:** nothing. **Blocked by:** nothing.

**Touches:** crates/core/src/manager/media_run/save.rs joined_container, join_original, crates/core/src/manager/media.rs insert_chosen, crates/media/src/merge.rs join, crates/streams/src/download/join.rs mux.

**Findings:**

- `Xpath-media-download-7`: confirmed, P2. A typed extension decides the join's container, and a refusal has no way out. Full text in `cross/path-media-download.md`, verdict in `verdicts/B06.md`.

### 133. A stream whose placing fails is downloaded again

P2, medium, open.

`join` removes `.wcstream` before `finish`, and a failed `place` writes no `placing.done`, so the next run fetches everything again. The realistic trigger is Replace with the old file open in a player.

**Fix:** Keep the resume state until the file is placed. A flush does not help this.

**Blocks:** nothing. **Blocked by:** nothing.

**Touches:** crates/core/src/manager/media_run/mod.rs run_stream, crates/core/src/manager/media_run/placement.rs finish, crates/streams/src/download/join.rs join.

**Findings:**

- `Xpath-media-download-11`: confirmed, P2. A stream whose placing fails is downloaded again, though the file is in the work folder. Full text in `cross/path-media-download.md`, verdict in `verdicts/B06.md`.

### 134. A saved format the page no longer lists can never succeed

P2, medium, open.

The stored ids are never re-resolved. Real yt-dlp says "Requested format is not available", which reads as `Refused`, always Retry. So a format the page no longer lists can never succeed.

**Fix:** Read the page again and choose again.

**Blocks:** nothing. **Blocked by:** nothing.

**Touches:** crates/core/src/manager/media_run/page.rs page_attempt, crates/media/src/ytdlp/info.rs selection, crates/media/src/describe.rs ytdlp.

**Findings:**

- `Xpath-media-download-9`: confirmed, P2. A saved format that the page no longer lists can never succeed, and the words point elsewhere. Full text in `cross/path-media-download.md`, verdict in `verdicts/B07.md`.

### 135. Try again can never succeed after the list failed to open

P2, medium, open.

`Core.cell` holds the open error for good, so Try again gets it back, and `unreachable_engine` names the engine for every failure. A test pins the repeat.

**Fix:** Let a retry open again, and word the title for each failure.

**Blocks:** nothing. **Blocked by:** nothing.

**Touches:** crates/desktop/src/main_window/empty.rs, unreachable_engine; crates/shell/src/host.rs, Core::opened.

**Findings:**

- `Xdatabase-migrations-2`: confirmed, P2. "Try again" can never succeed after the list failed to open, and the title names the wrong problem. Full text in `cross/database-migrations.md`, verdict in `verdicts/B09.md`.

### 136. Dragging moves rows the selection no longer shows

P2, medium, open.

`dragged_rows` carries raw `selected`, which is never pruned, so a drag moves rows the view no longer shows, stops running hidden rows, and is refused outright if one was removed. `nudge` has the same fault, and a drop on a queue takes HashSet order.

**Fix:** Read `selected_rows`, and keep line order.

**Blocks:** nothing. **Blocked by:** nothing.

**Touches:** crates/desktop/src/main_window/drag.rs, dragged_rows; crates/desktop/src/main_window/mod.rs (selected, never pruned); crates/desktop/src/main_window/queue_header.rs, nudge.

**Findings:**

- `25-4`: confirmed, P2. Dragging moves rows the selection no longer shows, is refused over removed rows, and shuffles their order. Full text in `chunks/25.md`, verdict in `verdicts/B13.md`.

### 137. Reading a website's page with yt-dlp escapes the local network rule

P2, medium, open.

`capture_arrived` checks only the first host, `hold_media` starts `analyse` at once, and `read_page` runs yt-dlp with no address rule, before the person answers. `page_attempt`'s else branch also downloads outside `network_rule`. No cookies cross hosts and nothing goes back to the site. That yt-dlp follows the redirect was not run.

**Fix:** The engine's guard cannot reach another process, so this needs its own rule, such as asking first or using a filtering proxy.

**Blocks:** nothing. **Blocked by:** nothing.

**Touches:** crates/core/src/manager/media.rs (hold_media, analyse, read_page), crates/media/src/ytdlp/mod.rs (extract_once), crates/core/src/manager/media_run/page.rs (page_attempt).

**Findings:**

- `Xsecurity-bridge-and-pipe-input-2`: confirmed, P2. Reading a website's page with yt-dlp escapes the local network rule, before the person answers. Full text in `cross/security-bridge-and-pipe-input.md`, verdict in `verdicts/B14.md`.

### 138. A queue's run ends while its last download only waits

P2, medium, open.

`has_work` looks only at running or waiting entries. A last download that is only waiting for an answer, or held for a move, lets the run end, and it then waits for the next start. Only pressing Start now recovers, which depends on item 79.

**Fix:** Widen what counts as work, including `Hold::Moving`.

**Blocks:** nothing. **Blocked by:** item 79.

**Touches:** crates/core/src/manager/scheduling.rs, evaluate_gates and view_of; crates/queues/src/schedule.rs, Gate::is_open.

**Findings:**

- `18-1`: confirmed, P2. A queue's run ends while its last download only waits for an answer or a move, and that download then waits for the next start. Full text in `chunks/18.md`, verdict in `verdicts/B16.md`.

### 139. Nothing opens a download's window again after Hide

P2, medium, open.

The only call of `show_download` is the `CaptureRepeated` arm of `follow`, and no text offers it. So Hide cannot be undone, against D23.8 and D48.6. The download keeps running and the main window shows it.

**Fix:** Add the row's entry.

**Blocks:** nothing. **Blocked by:** nothing.

**Touches:** crates/desktop/src/download_windows/mod.rs, show_download (no caller but follow); the missing entry belongs to the main window's row actions.

**Findings:**

- `24-8`: confirmed, P2. Nothing opens a download's progress window: Hide cannot be undone. Full text in `chunks/24.md`, verdict in `verdicts/B17.md`.

### 140. A file too large for the disk is not noticed until it is full

P2, medium, open.

`linux::fallocate(...).or_else(set_len)` discards `ENOSPC`, and on NTFS a sparse file allocates nothing. No crate asks for free space, so a file too large for the disk is not noticed until it is full. Not run. The README's "reserved up front" is the promise that fails. A full disk itself is untested.

**Fix:** Check free space at the start. `Xsecond-review-disk-writes-S1` asks the same.

**Blocks:** nothing. **Blocked by:** nothing.

**Touches:** crates/engine/src/storage.rs, reserve.

**Findings:**

- `01-3`: confirmed, P2. A file too large for the disk is not noticed until the disk is full. Full text in `chunks/01.md`, verdict in `verdicts/B03.md`.

### 141. A crash at the end of a move leaves the list and the disk disagreeing

P2, medium, open.

No record of a move exists on disk, so a crash at the end of a move, between the last file and the row's save, leaves the list and the disk disagreeing. `restore` can then mark a settled unfinished row completed. The window is one commit. Read from the code, not run.

**Fix:** Record the move before it starts, in the row record from item 4.

**Blocks:** nothing. **Blocked by:** item 8.

**Touches:** crates/core/src/manager/relocate.rs, move_file, move_files and finish_move; crates/core/src/manager/file_exists.rs, take_next_free_name.

**Findings:**

- `Xcrash-and-resume-4` (paths c and d): confirmed, P2. A crash during a move to another folder leaves a cut-off file under the final name, or strands the data. Full text in `cross/crash-and-resume.md`, verdict in `verdicts/B10.md`.

### 142. Commands and items left from the removed web interface are dead

P3, medium, open.

`api::pending_*`, `get_download`, `WarningsShown`, `is_download_window`, `Registry::shows` and others have no reader left since the Tauri and Svelte app went. `START_NOTICE_TIMES` and the TypeScript `mediaStart` are reached only by tests. A test's doc still names WebView2.

**Fix:** Remove them. This needs a note on D43.1 and new decisions for D65.2 and D50.4. `30-S1` goes further.

**Blocks:** nothing. **Blocked by:** nothing.

**Check to add:** A gate check listing public items of `crates/shell` with no caller, since the compiler does not report a library's.

**Touches:** crates/shell/src/api.rs, WarningsShown, WarningShowing, pending_confirmations, pending_media_choices, pending_file_exists, pending_addon_requests, get_download; crates/shell/src/download_windows.rs: is_download_window, Registry::shows, Registry::windows, Registry::waiting, and the doc on LABEL_PREFIX; crates/shell/src/finished.rs: FinishedBook::lists, next_key and the FinishedGroup::key it fills, and the other places its findings name.

**Findings:**

- `23-11`: confirmed, P3. Commands and a guard left from the web interface are dead, with docs describing a page that no longer exists. Full text in `chunks/23.md`, verdict in `verdicts/B19.md`.
- `24-11`: confirmed, P3. Items left over from the Tauri app, unused outside tests. Full text in `chunks/24.md`, verdict in `verdicts/B19.md`.
- `30-5`: confirmed, P3. START_NOTICE_TIMES and FINISH_NOTICE_TIMES are used by nothing. Full text in `chunks/30.md`, verdict in `verdicts/B19.md`.
- `30-6`: confirmed, P3. The TypeScript port of MediaDefaults::start has no user. Full text in `chunks/30.md`, verdict in `verdicts/B19.md`.
- `B19-5`: confirmed, P3. A test's doc still names WebView2 as a way for the start to fail. Full text in `verdicts/B19.md`, verdict in `verdicts/new-findings.md`.

### 143. Tests give waits a fixed short limit, and the timing check cannot see them

P3, medium, open.

Tests give a wait a short limit of their own: 10 s in `options.rs`, 5 s in `lifecycle.rs`, and `START_BUDGET`, `LIFETIME` and `LINGER` in the bridge and e2e tests. `test-timing` cannot see `recv_timeout`, a local `Duration` passed to a wait, or a measured time compared through a method. Only the check's own gap (`33-2`) lets them through.

**Fix:** Use `PATIENCE` everywhere, and widen `test-timing`.

**Blocks:** nothing. **Blocked by:** nothing.

**Check to add:** Widen `test-timing` to `recv_timeout`, a local `Duration` variable passed to a wait, and a measured time compared through a method. ENGINEERING section 7 forbids each.

**Touches:** crates/engine/tests/options.rs, within_deadline, DEADLINE; FIXED_WAIT and shortWaits in scripts/gate/test-timing.mjs; a live case in crates/engine/tests/options.rs (finished.recv_timeout(DEADLINE), the helper that runs a future on its own thread); crates/core/tests/manager/lifecycle.rs, a_server_that_never_answers_cannot_hang_the_app, and the other places its findings name.

**Findings:**

- `01-10`: confirmed, P3. The option tests give a wait that has to succeed a fixed 10 seconds. Full text in `chunks/01.md`, verdict in `verdicts/B19.md`.
- `33-2`: downgraded to P3. The test-timing check does not see recv_timeout, so a short wait that has to succeed passes the gate. Full text in `chunks/33.md`, verdict in `verdicts/B19.md`.
- `04-7`: confirmed, P3. A lifecycle test gives waits that must succeed a fixed five seconds. Full text in `chunks/04.md`, verdict in `verdicts/B19.md`.
- `06-3`: confirmed, P3. Test limits shorter than what the machine may take. Full text in `chunks/06.md`, verdict in `verdicts/B19.md`.
- `21-3` (its control_reading part): confirmed, P3. FTP tests race the machine against a throttled transfer or a short stall limit. Full text in `chunks/21.md`, verdict in `verdicts/B19.md`.

### 144. Tests race the machine against a real-clock limit of the code under test

P3, medium, open.

Tests run on the real clock against a limit of the code under test: the store's lock wait, a 3 s lookup, `Idle`'s 300 ms, a 600 ms bridge budget and a 1 s stop limit. A stall fails them on correct code. None was seen failing.

**Fix:** Use a paused clock or hold the slow path. A pattern check cannot see these, so they stay with review.

**Blocks:** nothing. **Blocked by:** nothing.

**Touches:** crates/store/src/lib.rs, tests migrations_run_once, negative_counts_are_repaired_and_reported, unreadable_downloads_are_kept_without_their_sign_in, unreadable_rows_are_skipped; crates/sites/src/local_network.rs, test only_a_websites_link_into_the_local_network_is_refused; crates/streams/tests/process.rs, a_stalled_program_keeps_what_it_wrote, what_a_program_leaves_running_ends_with_it, and the other places its findings name.

**Findings:**

- `03-7`: confirmed, P3. Store tests race the machine against the two-second lock wait. Full text in `chunks/03.md`, verdict in `verdicts/B19.md`.
- `10-5`: confirmed, P3. A test races the machine against the 3-second limit of the code under test. Full text in `chunks/10.md`, verdict in `verdicts/B19.md`.
- `13-4`: confirmed, P3. Two tests race the machine against a real-clock limit of the code under test. Full text in `chunks/13.md`, verdict in `verdicts/B19.md`.
- `B19-1`: confirmed, P3. A bridge test must look the key up twice within a real 600 ms. Full text in `verdicts/B19.md`, verdict in `verdicts/new-findings.md`.
- `B19-2`: confirmed, P3. A removal test races the real 1 second stop limit. Full text in `verdicts/B19.md`, verdict in `verdicts/new-findings.md`.

### 145. A page's HLS part runs outside the run's file work

P3, small, open.

The HLS branch of `fetch_part` never sets `file_work`, so `RunGuard` reports the run ended while blocking work still writes. Both writers put the same bytes at the same offsets, so the end is a retryable failure or a folder left on a slow disk, never wrong bytes.

**Fix:** Set it, one line, as the other branches do.

**Blocks:** nothing. **Blocked by:** nothing.

**Touches:** crates/core/src/manager/media_run/page.rs, fetch_part; crates/core/src/manager/media_run/page.rs fetch_part, crates/core/src/manager/run.rs RunGuard, crates/streams/src/download/join.rs concat.

**Findings:**

- `17-6`: downgraded to P3. An HLS part of a page video runs outside the run's file work. Full text in `chunks/17.md`, verdict in `verdicts/B06.md`.
- `Xpath-media-download-8`: downgraded to P3. A page's HLS part runs outside the run's file work, so a new run can meet it. Full text in `cross/path-media-download.md`, verdict in `verdicts/B06.md`.
- `Xsecond-review-disk-writes-9`: confirmed, P3. An HLS part of a page's video is left out of the run's file work. Full text in `cross/second-review-disk-writes.md`, verdict in `verdicts/B06.md`.
- `Xsecond-review-resume-6`: downgraded to P3. An HLS part of a page's video runs outside the run's file work. Full text in `cross/second-review-resume.md`, verdict in `verdicts/B06.md`.

### 146. Bridge and test values are written separately in several crates

P3, small, open.

The bridge's test overrides, `--background`, `WIRECATCH_TEST_BRIDGE` and the key file's name are literals in several crates, tied by comments. Renaming one leaves tests that pass or rebuild the bridge under other tests. More copies of the key name are in `ipc/tests/channel.rs` and `shell/src/host.rs`.

**Fix:** One constant each, as ENGINEERING section 1 asks. The bridge already depends on `wirecatch-protocol` and `wirecatch-product`.

**Blocks:** nothing. **Blocked by:** nothing.

**Touches:** crates/bridge/src/app.rs, test_overrides; crates/test-support/src/bridge.rs, ENDPOINT_VAR, LAUNCH_VAR, START_BUDGET_VAR; crates/shell/src/launch.rs (BACKGROUND_FLAG), crates/bridge/src/app.rs (BACKGROUND_ARG); crates/test-support/src/bridge.rs, BRIDGE_VARIABLE; scripts/gate/rust.mjs, BRIDGE_VARIABLE; crates/ipc/src/key.rs, KEY_FILE; crates/ipc/tests/keyed.rs, KEY_FILE, and the other places its findings name.

**Findings:**

- `06-4`: confirmed, P3. The bridge's test override names are written in two crates. Full text in `chunks/06.md`, verdict in `verdicts/B04.md`.
- `22-9`: confirmed, P3. The --background flag is written separately in the app and the bridge. Full text in `chunks/22.md`, verdict in `verdicts/B04.md`.
- `B04-3`: confirmed, P3. The variable that names the tests' bridge is written in two languages. Full text in `verdicts/B04.md`, verdict in `verdicts/new-findings.md`.
- `B04-4`: confirmed, P3. Tests write the key file's name again. Full text in `verdicts/B04.md`, verdict in `verdicts/new-findings.md`.

### 147. Test servers repeat their range parsing and behave unlike their docs

P3, small, open.

`Range` is parsed four times in `test-support`, and the HTTP/3 copy answers 200 where the others answer 416. `bytes_sent` has no reader, `drop_first` does not break off after the bytes it names, and `FileServer` records a request when answered, against its docs.

**Fix:** One parser, and correct or remove the rest.

**Blocks:** nothing. **Blocked by:** nothing.

**Touches:** crates/test-support/src/http3.rs (range, asked), crates/test-support/src/http.rs (parse_range, asks_too_much, Seen::range); crates/test-support/src/http.rs (Stats::bytes_sent, TestServer::bytes_sent, the update in body), crates/test-support/src/ftp.rs (FtpStats::bytes_sent, the update in send_file); crates/test-support/src/http.rs (Config::drop_first, body), and the other places its findings name.

**Findings:**

- `31-1`: confirmed, P3. The Range header is parsed four times in the crate, and the HTTP/3 copy answers a range past the end differently. Full text in `chunks/31.md`, verdict in `verdicts/B19.md`.
- `31-2`: confirmed, P3. bytes_sent is kept by the HTTP and FTP servers and read by nothing. Full text in `chunks/31.md`, verdict in `verdicts/B19.md`.
- `31-3`: confirmed, P3. drop_first on the HTTP server does not break off after the bytes it names, and never breaks off a short answer. Full text in `chunks/31.md`, verdict in `verdicts/B19.md`.
- `31-4`: confirmed, P3. FileServer records a request when it is answered, while its docs say when it arrived. Full text in `chunks/31.md`, verdict in `verdicts/B19.md`.

### 148. An import's summary event and the every-video answer have no reader

P3, small, open.

`SettingsImported` is read only in core's tests, and `INCLUDED` promises a question about add-ons that never comes. `every_video` is set only by tests, so its branch runs for no product caller.

**Fix:** Wire them or remove them. `every_video` has no tie to import and may travel separately.

**Blocks:** nothing. **Blocked by:** nothing.

**Touches:** crates/desktop/src/main_window/backup_dialog.rs, INCLUDED; crates/core/src/manager/preferences.rs, import_settings; crates/protocol/src/app.rs, UiEvent::SettingsImported; crates/protocol/src/app.rs, UiEvent::SettingsImported, ImportSummary; crates/desktop/src/download_windows/media_form.rs, answer; crates/core/src/manager/media.rs, remember_media_choice.

**Findings:**

- `27-11`: confirmed, P3. Nothing reads SettingsImported, and the promise to ask about add-ons is not kept. Full text in `chunks/27.md`, verdict in `verdicts/B12.md`.
- `30-4`: confirmed, P3. UiEvent::SettingsImported has no reader in the product. Full text in `chunks/30.md`, verdict in `verdicts/B12.md`.
- `Xfeature-behaviour-7`: confirmed, P3. An import's summary event and the "every video" answer have nothing on the other end. Full text in `cross/feature-behaviour.md`, verdict in `verdicts/B12.md`.

### 149. Tests repeat their fixtures and data generators

P3, medium, open.

Each test file writes a whole `NewDownload`, six 14-field `Job` literals repeat, and two private `test_data` generators sit next to the shared one.

**Fix:** Shared bases in `test-support`, which has a `downloads` feature for it.

**Blocks:** nothing. **Blocked by:** nothing.

**Touches:** crates/engine/src/http3/tests.rs (test_data), crates/engine/examples/bench_http3.rs (run); crates/core/tests/manager/again.rs (click), protocol.rs (capture), windows.rs (file_capture), and outside the chunk ftp.rs, queues.rs, media_support.rs (media_capture), crates/core/src/manager/tests.rs, crates/e2e/src/app.rs, crates/sites/src/local_network.rs; crates/core/src/manager/media_run/tests.rs (Job literal), and the other places its findings name.

**Findings:**

- `02-8`: confirmed, P3. Two private copies of a test data generator next to the shared one. Full text in `chunks/02.md`, verdict in `verdicts/B19.md`.
- `07-7`: confirmed, P3. Each test file writes its own whole NewDownload. Full text in `chunks/07.md`, verdict in `verdicts/B19.md`.
- `17-9`: confirmed, P3. The media tests repeat their fixtures. Full text in `chunks/17.md`, verdict in `verdicts/B19.md`.

### 150. A reload after missed events repeats notices and leaves open windows stale

P3, medium, open.

`load` after `Lagged` refills startup notices, so one notice repeats, and it does not refresh open download windows, which keep the old step. It needs 1,024 unread events. Try again after a failed open shows them for the first time, which is correct. `24-9` was not produced.

**Fix:** Do not refill notices on a reload, and refresh what is open.

**Blocks:** nothing. **Blocked by:** nothing.

**Touches:** crates/desktop/src/library.rs, Library::load; crates/desktop/src/main_window/notices_bar.rs, tell_startup_notices; crates/desktop/src/download_windows/mod.rs, restore; crates/shell/src/download_windows.rs, Registry::restore; called from Library::load.

**Findings:**

- `23-9`: downgraded to P3. The start's warnings show again after the list reloads from missed events. Full text in `chunks/23.md`, verdict in `verdicts/B13.md`.
- `Xdatabase-migrations-3`: confirmed, P3. The notice about a repaired list comes back when the library reloads. Full text in `cross/database-migrations.md`, verdict in `verdicts/B13.md`.
- `24-9`: downgraded to P3. After missed events, open download windows keep showing the old step. Full text in `chunks/24.md`, verdict in `verdicts/B13.md`.

### 151. A capture's check and its record sit in separate lock sections

P3, small, fixed.

The handover's check and its record are separate lock sections, so a withdrawal can land between them with contrary answers, and two captures of one link at once make two downloads (`MOST ROWS 2`). `hold_for_confirmation` is one section, so this needs `ask_before_download` off.

**Fix:** One decision under one lock.

**Blocks:** nothing. **Blocked by:** nothing.

**Touches:** crates/core/src/manager/captures.rs, capture_arrived, the Ok(()) arm after await_answer; crates/core/src/manager/captures.rs, capture_arrived, join_unfinished; crates/core/src/manager/downloads.rs, insert.

**Findings:**

- `07-5`: confirmed, P3. A withdrawal can land between the check and the record of a capture's answer, and both sides get contrary answers. Full text in `chunks/07.md`, verdict in `verdicts/B01.md`.
- `Xconcurrency-6`: confirmed, P3. Two captures of one link at the same moment make two downloads. Full text in `cross/concurrency.md`, verdict in `verdicts/B01.md`.

### 152. A found duplicate overrides a stop the person chose in the same moment

P3, small, fixed.

`run.found` is taken whatever `run.stop` says, so a Cancel or Pause in the moment after a duplicate is found is overridden. The window has no file work in it. The finding's "longest for a video placing a large file" does not hold.

**Fix:** Let the stop win.

**Blocks:** nothing. **Blocked by:** nothing.

**Touches:** crates/core/src/manager/run.rs, finish; crates/core/src/manager/file_exists.rs, on_found; crates/core/src/manager/run.rs, finish (the if let Some(found) after the status match); crates/core/src/manager/state.rs, Entry::stop.

**Findings:**

- `04-3`: downgraded to P3. A run's found duplicate overrides what the person chose while it stopped. Full text in `chunks/04.md`, verdict in `verdicts/B01.md`.
- `05-12`: downgraded to P3. Cancel or Pause in the moment after a duplicate is found is overridden. Full text in `chunks/05.md`, verdict in `verdicts/B01.md`.

### 153. A link or folder in place of the key gets a raw system error

P3, small, open.

`stored` sends everything but `NotFound` to `Io`, so a link on Linux and a folder on Windows get a raw error where `endpoint_in` promises `BadEndpointKey`. The refusal itself is right.

**Fix:** Map both to `BadEndpointKey`. A link is never repaired, by design (D41.4), so only the advice changes.

**Blocks:** nothing. **Blocked by:** nothing.

**Touches:** crates/ipc/src/key.rs, stored and open_for_reading.

**Findings:**

- `08-4`: confirmed, P3. On Linux a link in place of the key is a raw system error, not BadEndpointKey. Full text in `chunks/08.md`, verdict in `verdicts/B04.md`.
- `B04-1`: confirmed, P3. On Windows a folder in place of the key is a raw system error, not BadEndpointKey. Full text in `verdicts/B04.md`, verdict in `verdicts/new-findings.md`.

### 154. Cancelling does not interrupt name lookups

P3, small, open.

`private_guard` and the guard in `send` sit outside any `select!`, and `fetch_parts` awaits `network_rule` the same way. In the app the delay is bounded by `stop_timeout`, and nothing is lost by the abort. The 60 s holds only for a direct user of the engine.

**Fix:** Race the lookups with the cancel token.

**Blocks:** nothing. **Blocked by:** nothing.

**Touches:** crates/engine/src/net_guard.rs, private_guard and NetworkGuard::check, as called from download_with, Fetcher::with_private_network and http::send; crates/core/src/manager/media_run/page.rs, fetch_parts; crates/core/src/manager/media_run/network.rs, network_rule, page_is_local.

**Findings:**

- `09-4`: downgraded to P3. Cancelling does not interrupt the guard's name lookups. Full text in `chunks/09.md`, verdict in `verdicts/B14.md`.
- `B14-2`: confirmed, P3. Core's early lookup of a format's address ignores a stop. Full text in `verdicts/B14.md`, verdict in `verdicts/new-findings.md`.

### 155. The rebinding tests never reach the filtering resolver, and its refusal ends as a network failure

P3, small, open.

Both rebinding tests use `localhost`, refused before any connection, so removing `.dns_resolver` passes them. A refusal by the resolver also ends as `Error::Network`, not `PrivateNetwork`. The connection stays blocked.

**Fix:** A test with the name cached as allowed in `NetworkGuard::names` covers both.

**Blocks:** nothing. **Blocked by:** nothing.

**Touches:** crates/engine/src/net_guard.rs, test a_download_cannot_be_rebound_to_the_local_network; crates/engine/tests/fetch.rs, test a_start_name_that_resolves_to_the_local_network_is_blocked_when_asked; crates/engine/src/net_guard.rs, FilteringResolver::resolve.

**Findings:**

- `09-5`: confirmed, P3. The DNS rebinding tests never reach the filtering resolver. Full text in `chunks/09.md`, verdict in `verdicts/B14.md`.
- `09-6`: confirmed, P3. A refusal by the resolver ends as a plain network failure. Full text in `chunks/09.md`, verdict in `verdicts/B14.md`.

### 156. Tests do not cover the stored forms of enums and old settings

P3, small, open.

`KINDS` has no entry for `status`, `category`, `chosen_by` or `queue_reason`, so a rename hides rows unnoticed. The migration tests seed today's settings, so they do not cover old ones. `Xdatabase-migrations-S2` suggests a real database per release.

**Fix:** Add the kinds and an old settings sample. No behaviour changes.

**Blocks:** nothing. **Blocked by:** nothing.

**Touches:** crates/store/tests/samples.rs, KINDS; crates/store/src/lib.rs, save_download, read_download; crates/store/src/lib.rs, tests database_with_settings, the_window_before_each_download_is_switched_on_once.

**Findings:**

- `03-6`: confirmed, P3. The samples test does not cover the enum columns or the queue reason, so a rename hides rows unnoticed. Full text in `chunks/03.md`, verdict in `verdicts/B09.md`.
- `Xdatabase-migrations-5`: confirmed, P3. The migration tests seed today's settings, so they do not cover old stored settings. Full text in `cross/database-migrations.md`, verdict in `verdicts/B09.md`.

### 157. A held-open output loses a program's collected output, and its test cannot fail

P3, small, open.

`Readers::abort` aborts the task that holds the only copy of the output, so a run past the grace parses nothing. It needs a detached child on Unix or a stall of over 2 s. `a_child_that_left_the_group_does_not_hold_the_run` passes without the grace, and is `cfg(unix)`, not run here.

**Fix:** Keep the output, and add the assertion the test needs.

**Blocks:** nothing. **Blocked by:** nothing.

**Touches:** crates/streams/src/process/run.rs, run (the grace branch), Readers::abort, finish, collect; crates/streams/tests/process.rs, a_child_that_left_the_group_does_not_hold_the_run; crates/streams/examples/fake_program.rs, LINGER.

**Findings:**

- `13-2`: confirmed, P3. A run whose outputs are held open loses all of its collected standard output, even when the program succeeded. Full text in `chunks/13.md`, verdict in `verdicts/B08.md`.
- `13-3`: confirmed, P3. The test that a child outside the group does not hold the run passes without the grace. Full text in `chunks/13.md`, verdict in `verdicts/B08.md`.

### 158. A resume file's temporary file can stay for good

P3, small, open.

The temporary name is a private detail of `save`, removed only there, so a kill can leave one small `.wcstate.tmp` for good. Nothing reads it.

**Fix:** Remove it where partial files are listed.

**Blocks:** nothing. **Blocked by:** nothing.

**Touches:** crates/engine/src/state.rs, ResumeState::save; crates/core/src/naming.rs, partial_files; crates/engine/src/download.rs, finish; crates/core/src/manager/state.rs, partial_paths.

**Findings:**

- `01-8`: downgraded to P3. A resume file's temporary file can be left in the download folder for good. Full text in `chunks/01.md`, verdict in `verdicts/B03.md`.
- `Xsecond-review-disk-writes-8`: confirmed, P3. .wcstate.tmp can stay beside a finished or removed download for good. Full text in `cross/second-review-disk-writes.md`, verdict in `verdicts/B03.md`.

### 159. Add-on release files of skipped versions stay in the work folder

P3, small, fixed.

`swap_in` deletes only the installed version's file, and files are named by version, so skipped ones stay in `.work`.

**Fix:** Clear the folder after a swap.

**Blocks:** nothing. **Blocked by:** nothing.

**Touches:** crates/core/src/manager/addons/install.rs, swap_in and Manager::fetch_part; crates/core/src/manager/addons/install.rs, swap_in and fetch_part; crates/core/src/manager/addons/mod.rs.

**Findings:**

- `11-4`: confirmed, P3. Release files of a version that is never installed stay in the work folder for good. Full text in `chunks/11.md`, verdict in `verdicts/B12.md`.
- `Xsecurity-addon-unpacking-4`: confirmed, P3. Release files of versions no longer wanted stay in .work for good. Full text in `cross/security-addon-unpacking.md`, verdict in `verdicts/B12.md`.

### 160. Tray ticks flip on a repeat click, and & in a name shows wrongly

P3, small, open.

Muda flips a check item before it sends the event, and the menu rebuilds only when the choices differ, so a repeat click clears the tick. `&` in a queue or proxy name marks a mnemonic. The setting stays right. Linux was not traced.

**Fix:** Set the check back and escape labels, both in `tray.rs::menu`.

**Blocks:** nothing. **Blocked by:** nothing.

**Touches:** crates/desktop/src/tray.rs, menu, follow, on_click; crates/shell/src/tray_speed.rs, crates/shell/src/tray_proxy.rs; crates/desktop/src/tray.rs, menu (checks); crates/shell/src/tray_queues.rs, items; crates/shell/src/tray_proxy.rs, items.

**Findings:**

- `26-2`: downgraded to P3. Clicking the speed limit or proxy that is already checked removes its tick. Full text in `chunks/26.md`, verdict in `verdicts/B17.md`.
- `26-3`: downgraded to P3. A queue or proxy name containing & shows wrongly in the tray menu. Full text in `chunks/26.md`, verdict in `verdicts/B17.md`.

### 161. Messages name the product with a literal

P3, small, open.

Messages in `notifications.rs`, `tray.rs`, `api.rs`, `appimage.rs`, `bridge.rs` and `core/naming.rs` say "Wirecatch" in a build whose channel is "Wirecatch Dev", and a change to `config/product.toml` leaves them behind.

**Fix:** Read the product crate.

**Blocks:** nothing. **Blocked by:** nothing.

**Check to add:** A gate check for the product name in string literals, which nothing catches today.

**Touches:** crates/shell/src/notifications.rs, SHOW_IN_APP; crates/shell/src/tray.rs, perform; crates/core/src/naming.rs, checked; crates/shell/src/api.rs, the invalid-text and folder messages; crates/shell/src/appimage.rs, its error messages; crates/shell/src/bridge.rs, its error messages.

**Findings:**

- `26-9`: confirmed, P3. Two messages name the product with a literal. Full text in `chunks/26.md`, verdict in `verdicts/B17.md`.
- `B17-1`: confirmed, P3. Product name written out in more messages than the two of 26-9. Full text in `verdicts/B17.md`, verdict in `verdicts/new-findings.md`.

### 162. A navigation item left behind keeps its hover colour until the mouse moves

P3, small, open.

`SidebarMenuItem` sets `hover_style` only while not active, and GPUI reads the stored flag at layout and the hitbox at paint. So a page left behind keeps its hover text colour until the next mouse move. Posted messages send no further move, which is why every capture showed it.

**Fix:** Always set the hover style. The cause is in gpui-pre and gpui-component, not in `settings/mod.rs`.

**Blocks:** nothing. **Blocked by:** nothing.

**Touches:** settings navigation in crates/desktop/src/main_window/settings/mod.rs, drawn by the gpui-kit settings component; crates/desktop/src/main_window/sidebar.rs, the Settings entry and the navigation tile.

**Findings:**

- `U-1`: confirmed, P3. The settings page left behind keeps the accent colour. Full text in `ui.md`, verdict in `verdicts/B18.md`.
- `B18-2`: confirmed, P3. The main navigation keeps a page's old hover colour after leaving it. Full text in `verdicts/B18.md`, verdict in `verdicts/new-findings.md`.

### 163. Empty pages offer Pause all or Resume all

P3, small, open.

`group_command` does not look at the page's rows, so empty pages offer Pause all beside "Nothing downloading", and Resume all beside "Nothing paused".

**Fix:** Offer it only with rows.

**Blocks:** nothing. **Blocked by:** nothing.

**Touches:** crates/desktop/src/main_window/list.rs, group_command; crates/desktop/src/main_window/command_bar.rs, page_command.

**Findings:**

- `U-3`: confirmed, P3. Empty pages offer Pause all with nothing to pause. Full text in `ui.md`, verdict in `verdicts/B13.md`.
- `B13-2`: confirmed, P3. An empty Paused page offers Resume all. Full text in `verdicts/B13.md`, verdict in `verdicts/new-findings.md`.

### 164. A failed video or conversion shows the program's raw output

P3, small, open.

`refusal` strips little, so `[generic] id:` and `(caused by <HTTPError ...>)` reach the message a person reads. `FfmpegError::Failed` puts ffmpeg's own last line in brackets the same way (`B18-3`). D3 and D83 say otherwise.

**Fix:** Word the common causes, and keep the raw line for the log.

**Blocks:** nothing. **Blocked by:** nothing.

**Roadmap:** Same as the open item "A failed video shows the add-on's raw output", so `docs/ROADMAP.md` lists it once under that name.

**Touches:** crates/media, the reading error; shown by crates/desktop/src/download_windows/view.rs; crates/media/src/ffmpeg.rs, FfmpegError::Failed; crates/media/src/describe.rs, join, conversion.

**Findings:**

- `U-9`: confirmed, P3. Raw yt-dlp output in a failed video message. Full text in `ui.md`, verdict in `verdicts/B18.md`.
- `B18-3`: confirmed, P3. Raw ffmpeg output in a failed join or conversion message. Full text in `verdicts/B18.md`, verdict in `verdicts/new-findings.md`.

### 165. New queue and New proxy do not put the cursor in the Name field

P3, small, open.

`queue_dialog::open` and the proxy dialog never call `focus_after_opening`, which `add_dialog`, `refresh_dialog` and `queue_delete` do. Typing at once puts nothing in the field. `B16-1` is read, not run.

**Fix:** Call it.

**Blocks:** nothing. **Blocked by:** nothing.

**Touches:** crates/desktop/src/main_window/queue_dialog.rs; crates/desktop/src/main_window/proxy_dialog.rs, the crate::ui::dialog::open call.

**Findings:**

- `U-13`: confirmed, P3. New queue does not put the cursor in its Name field. Full text in `ui.md`, verdict in `verdicts/B16.md`.
- `B16-1`: confirmed, P3. New proxy does not put the cursor in its Name field. Full text in `verdicts/B16.md`, verdict in `verdicts/new-findings.md`.

### 166. The row, the menu and the details decide offers and words on their own

P3, small, open.

`cancellable` leaves out Failed while `can_cancel` allows it, `RowAction::offered` matches on status itself, and a cancelled row says Try again in the menu and Start over in the details.

**Fix:** One place decides offers and their words.

**Blocks:** nothing. **Blocked by:** nothing.

**Touches:** crates/desktop/src/main_window/details.rs, footer (cancellable, main); crates/desktop/src/main_window/list.rs, RowAction::offered; crates/desktop/src/main_window/applies.rs; crates/desktop/src/main_window/row_menu.rs, details.rs.

**Findings:**

- `25-11`: confirmed, P3. The details and the row's buttons decide what applies on their own, and have drifted from applies. Full text in `chunks/25.md`, verdict in `verdicts/B13.md`.
- `U-10`: confirmed, P3. The cancelled download's action has two names. Full text in `ui.md`, verdict in `verdicts/B13.md`.

### 167. Two test files depend on an order only timing decides

P3, small, open.

`the_same_server_name_twice_gets_a_free_name` fails if the first download ends before the second's answer, and FTP transfer tests rely on a throttle so other connections can join.

**Fix:** Hold the server so the order is fixed, and count what happened.

**Blocks:** nothing. **Blocked by:** nothing.

**Touches:** crates/core/tests/manager/settle.rs, the_same_server_name_twice_gets_a_free_name; crates/engine/tests/ftp.rs plain_download_is_byte_identical_over_parallel_connections, completes_within_session_limit, the_size_from_the_transfer_reply_is_used_without_size_support; crates/engine/src/ftp/tests.rs explicit_tls_download_is_byte_identical, implicit_tls_download_is_byte_identical, and the other places its findings name.

**Findings:**

- `05-14`: confirmed, P3. the_same_server_name_twice_gets_a_free_name fails when the first download wins the race. Full text in `chunks/05.md`, verdict in `verdicts/B19.md`.
- `21-3` (its transfer tests): confirmed, P3. FTP tests race the machine against a throttled transfer or a short stall limit. Full text in `chunks/21.md`, verdict in `verdicts/B19.md`.

### 168. The same small pieces of logic are written many times in the forms

P3, medium, open.

The same small pieces are written several times across the forms, and the failure match of `Backend::run` is at 15 sites in 12 files, which already differ. A change to how a manager that did not open is reported must be made at each.

**Fix:** One helper per piece. ENGINEERING section 3 allows no third copy. `jscpd` found only the exact clones.

**Blocks:** nothing. **Blocked by:** nothing.

**Touches:** crates/desktop/src/main_window/order.rs, crates/desktop/src/main_window/refresh_dialog.rs, crates/desktop/src/app_update.rs, crates/desktop/src/notices.rs, crates/desktop/src/backend.rs, crates/desktop/src/download_windows/view.rs (two), crates/desktop/src/files.rs (two).

**Findings:**

- `27-12`: confirmed, P3. The same small pieces of logic are written several times in these forms. Full text in `chunks/27.md`, verdict in `verdicts/B19.md`.
- `B19-3`: confirmed, P3. The failure match of Backend::run has nine more copies than 27-12 lists. Full text in `verdicts/B19.md`, verdict in `verdicts/new-findings.md`.

### 169. Dying between placing a video and deleting its work folder gives a second copy

P3, medium, open.

Media placing has no durable step between `place` and `discard::dir`. A crash or kill there gives a second copy or a question about its own file. A normal quit cannot reach it, and no file is lost. Read, not run.

**Fix:** A marker written before placing, in `placement.rs`.

**Blocks:** nothing. **Blocked by:** item 4.

**Touches:** crates/core/src/manager/media_run/placement.rs, finish and place; crates/core/src/manager/restore.rs, restore_entry; crates/core/src/manager/media_run/placement.rs, finish; crates/core/src/naming.rs, finished_size.

**Findings:**

- `Xcrash-and-resume-3`: downgraded to P3. A video put in place before the crash is treated as someone else's file on the next run. Full text in `cross/crash-and-resume.md`, verdict in `verdicts/B02.md`.
- `Xsecond-review-resume-8`: confirmed, P3. Dying between placing a video and deleting its work folder gives a second copy or a question about its own file. Full text in `cross/second-review-resume.md`, verdict in `verdicts/B02.md`.

### 170. Exporting settings over an earlier file is not written whole

P3, small, open.

`export_settings` uses `std::fs::write` over the chosen file, and the save prompt offers an existing one, so a failure leaves it cut.

**Fix:** Call `whole_file::replace`.

**Blocks:** nothing. **Blocked by:** nothing.

**Check to add:** A `disallowed-methods` entry for `std::fs::write`; a search of non-test writers finds no other file a person keeps.

**Touches:** crates/core/src/manager/preferences.rs export_settings; crates/bridge/src/install/host.rs, files::write; crates/core/src/manager/preferences.rs, the settings export; against crates/files/src/whole_file.rs, replace.

**Findings:**

- `19-6`: confirmed, P3. Exporting settings over an earlier file is not written whole. Full text in `chunks/19.md`, verdict in `verdicts/B12.md`.
- `Xrepeated-logic-6` (the export half): confirmed, P3. Two writers go around whole_file::replace and lose what it guarantees. Full text in `cross/repeated-logic.md`, verdict in `verdicts/B12.md`.

### 171. The bridge's manifest write has no flush

P3, small, open.

`files::write` does `fs::write` then `rename` with no `sync_all`, while `Host::write_file` documents an atomic replace. It takes a power cut right after a rewrite, and the next start repairs it.

**Fix:** Flush in its own writer. `replace` has one `.tmp` name, which the bridge's per-process name avoids.

**Blocks:** nothing. **Blocked by:** nothing.

**Touches:** crates/bridge/src/install/host.rs, files::write; crates/core/src/manager/preferences.rs, the settings export; against crates/files/src/whole_file.rs, replace.

**Findings:**

- `Xrepeated-logic-6` (the bridge half): confirmed, P3. Two writers go around whole_file::replace and lose what it guarantees. Full text in `cross/repeated-logic.md`, verdict in `verdicts/B12.md`.

### 172. Pause all while a cancel is stopping keeps the download

P3, small, fixed.

`pause_all` passes `Stop::Pause`, a person's choice, which replaces a stopping Cancel or Restart. The row ends paused with its partial file, and can be cancelled again.

**Fix:** A bulk pause should not count as a choice about each download.

**Blocks:** nothing. **Blocked by:** nothing.

**Touches:** crates/core/src/manager/downloads.rs, pause_all; crates/core/src/manager/state.rs, Entry::stop.

**Findings:**

- `Xconcurrency-7`: confirmed, P3. Pause all while a cancel is stopping keeps the download and its partial file. Full text in `cross/concurrency.md`, verdict in `verdicts/B01.md`.

### 173. The same link with another #fragment opens a second question

P3, small, fixed.

`p.capture.url == url_text` keeps the fragment, so the same link with another `#fragment` opens a second window while a question is open.

**Fix:** Use `same_address`.

**Blocks:** nothing. **Blocked by:** nothing.

**Touches:** crates/core/src/manager/captures.rs, hold_for_confirmation, against same_address.

**Findings:**

- `07-3`: downgraded to P3. The same link with another #fragment opens a second question and a second download when windows ask first. Full text in `chunks/07.md`, verdict in `verdicts/B01.md`.

### 174. A file beside a leftover part or resume file is never asked about

P3, small, open.

`found_existing` uses `finished_size`, which returns `None` when a partial file sits beside the name, so the setting is not applied and the download silently takes `x (1).bin`. `place` avoids this with its own metadata check.

**Fix:** Check the file itself, as `place` does.

**Blocks:** nothing. **Blocked by:** item 48.

**Touches:** crates/core/src/manager/file_exists.rs, found_existing; crates/core/src/naming.rs, finished_size.

**Findings:**

- `B02-1`: confirmed, P3. A file beside a leftover part or resume file is never asked about. Full text in `verdicts/B02.md`, verdict in `verdicts/new-findings.md`.

### 175. A saved download that gives way to its page loses its link if the app stops first

P3, small, fixed.

`read_answered_page` removes the saved row at once, and `hold_media` keeps the link only in memory, so a quit before the question is answered loses it. Only a row added in this session reaches it.

**Fix:** Keep the row until its replacement is saved.

**Blocks:** nothing. **Blocked by:** nothing.

**Touches:** crates/core/src/manager/added_pages.rs, read_answered_page.

**Findings:**

- `Xcrash-and-resume-6`: confirmed, P3. A saved download that gives way to reading its page loses its link if the app stops before the question is answered. Full text in `cross/crash-and-resume.md`, verdict in `verdicts/B02.md`.

### 176. A removal gives up the path before it deletes the files

P3, small, fixed.

`finish_removing` calls `remove_entry` under the lock and `delete_files` after it, with no `cleanup` token, and `unclaimed_path` does not look at the disk. A new video can start in a folder being deleted. A retry recovers.

**Fix:** A `cleanup` token like a cancel's. Owners from item 1 do not fix this.

**Blocks:** nothing. **Blocked by:** nothing.

**Touches:** crates/core/src/manager/downloads.rs, finish_removing; crates/core/src/naming.rs, unclaimed_path.

**Findings:**

- `Xconcurrency-10`: confirmed, P3. A removal gives up the path before it deletes the files, and a new video can start in the folder being deleted. Full text in `cross/concurrency.md`, verdict in `verdicts/B01.md`.

### 177. A failed write takes too much back from the progress count

P3, small, open.

A failed write moves `unwritten` to `dropped`, `stream` breaks before `progress.add`, and `take_back` then takes too much. It shows only when one connection fails and others carry on, and only in the count.

**Fix:** Take back only what was added.

**Blocks:** nothing. **Blocked by:** nothing.

**Touches:** crates/engine/src/worker.rs, stream; crates/engine/src/writer.rs, Writer::push, Writer::wait.

**Findings:**

- `01-5`: downgraded to P3. A failed write takes more back from the progress count than was ever added. Full text in `chunks/01.md`, verdict in `verdicts/B03.md`.

### 178. A range past the end of a shrunken file is reported as a server error

P3, small, open.

`fetch` sends a 416 to `HttpStatus`, and the words depend on which connection failed first. A new run recovers.

**Fix:** Map it to a changed file.

**Blocks:** nothing. **Blocked by:** nothing.

**Touches:** crates/engine/src/http.rs, fetch; crates/engine/src/http/range.rs, check_same_file.

**Findings:**

- `02-4`: confirmed, P3. A range past the end of a file that shrank is reported as a server error, not as a changed file. Full text in `chunks/02.md`, verdict in `verdicts/B11.md`.

### 179. The step-back rule's long-wait test is true only because of jitter

P3, small, fixed.

`pause` is capped at `max` before jitter, so `delay > backoff.max` is true only at the cap with jitter. `retry_after.is_some()` already covers D69.1, and the branch is close to dead at defaults.

**Fix:** Remove the comparison.

**Blocks:** nothing. **Blocked by:** nothing.

**Touches:** crates/engine/src/retry.rs, Attempts::after_failure (long_wait).

**Findings:**

- `02-5`: confirmed, P3. The "delay past the normal backoff" test in the step-back rule is true only because of jitter. Full text in `chunks/02.md`, verdict in `verdicts/B03.md`.

### 180. Fetcher::save promises data survives a drop, but up to 256 KiB is lost

P3, small, open.

`sink.finish()` is reached only after `transfer` returns, and `BufWriter` does not flush on drop, so up to 256 KiB is lost on a drop, against `Fetcher::save`'s doc. The one caller renames only after `Ok`, so nothing is harmed.

**Fix:** Correct the doc.

**Blocks:** nothing. **Blocked by:** nothing.

**Touches:** crates/engine/src/fetch.rs, Fetcher::save.

**Findings:**

- `02-6`: confirmed, P3. Fetcher::save docs promise the data survives dropping the future, but up to 256 KiB is lost. Full text in `chunks/02.md`, verdict in `verdicts/B03.md`.

### 181. bench_local sleeps for no time after each accept

P3, small, fixed.

`sleep(Duration::ZERO)` in `bench_local serve` does nothing, since `accept` is the await point.

**Fix:** Remove it.

**Blocks:** nothing. **Blocked by:** nothing.

**Touches:** crates/engine/examples/bench_local.rs, serve.

**Findings:**

- `02-7`: confirmed, P3. bench_local serve sleeps for no time after each accept. Full text in `chunks/02.md`, verdict in `verdicts/B11.md`.

### 182. A redirect error tells the CLI user to allow something the CLI cannot

P3, small, fixed.

`Error::InsecureRedirect`'s text says "Allow insecure redirects", and the CLI offers no such option. The app's own message does not say it.

**Fix:** Add the option or change the text.

**Blocks:** nothing. **Blocked by:** nothing.

**Touches:** crates/cli/src/main.rs, build_request, Args; crates/engine/src/error.rs, Error::InsecureRedirect.

**Findings:**

- `32-5`: confirmed, P3. A redirect from https to http tells the person to allow something the CLI cannot allow. Full text in `chunks/32.md`, verdict in `verdicts/B03.md`.

### 183. async_and_blocking_reads_agree never does a blocking read

P3, small, open.

The test named for two readers agreeing calls only `read_async`.

**Fix:** Compare the two.

**Blocks:** nothing. **Blocked by:** nothing.

**Touches:** crates/bridge/src/raw_frame.rs, test async_and_blocking_reads_agree.

**Findings:**

- `06-2`: confirmed, P3. async_and_blocking_reads_agree never does a blocking read. Full text in `chunks/06.md`, verdict in `verdicts/B04.md`.

### 184. The AppImage replacement is renamed into place without a flush

P3, small, fixed.

`replace_appimage` runs `copy`, `set_permissions`, `rename` with no `sync_all`, while `write_private` in the same crate syncs, and an error leaves `.wirecatch-update.AppImage` behind.

**Fix:** Flush, and clean up on error.

**Blocks:** nothing. **Blocked by:** nothing.

**Touches:** crates/shell/src/updates.rs, replace_appimage.

**Findings:**

- `12-4`: confirmed, P3. The AppImage replacement is renamed into place without being flushed. Full text in `chunks/12.md`, verdict in `verdicts/B05.md`.

### 185. The unsigned release page address reaches the system shell unchecked

P3, small, open.

`html_url` is a plain string kept unparsed and reaches `ShellExecuteW` through `open_url`, against the letter of D111.3. It takes a trusted certificate, a genuine signed manifest and a click.

**Fix:** Parse it and allow only https.

**Blocks:** nothing. **Blocked by:** nothing.

**Touches:** crates/core/src/manager/app_update.rs check_app_update (page); crates/desktop/src/main_window/notices_bar.rs, settings/general.rs, crates/desktop/src/app_update.rs install (cx.open_url).

**Findings:**

- `Xsecurity-update-signatures-4`: downgraded to P3. The unsigned release page address goes to the system's shell with no check. Full text in `cross/security-update-signatures.md`, verdict in `verdicts/B05.md`.

### 186. Turning off pre-release versions keeps offering the one already found

P3, small, open.

A quiet check returns early on `Available`, and that state has no check button. So turning the setting off keeps offering the pre-release already found.

**Fix:** Check again when the setting changes.

**Blocks:** nothing. **Blocked by:** nothing.

**Touches:** crates/desktop/src/app_update.rs, check; crates/desktop/src/main_window/settings/general.rs, updates.

**Findings:**

- `Xpath-app-update-7`: confirmed, P3. Turning off pre-release versions keeps offering the one already found. Full text in `cross/path-app-update.md`, verdict in `verdicts/B05.md`.

### 187. No test holds a manifest's version and size rules

P3, small, open.

`Harness::publish` derives the manifest's version from the tag and gives the asset and manifest one size. With both checks removed, all 5 `app_update` tests still passed.

**Fix:** Add a test for each rule.

**Blocks:** nothing. **Blocked by:** nothing.

**Touches:** crates/core/src/manager/app_update.rs, check_app_update and its tests.

**Findings:**

- `12-6`: confirmed, P3. No test holds the rule that a manifest must name its release's own version. Full text in `chunks/12.md`, verdict in `verdicts/B05.md`.

### 188. The packaging example for ARM64 on x64 cannot work

P3, small, open.

`arch` from the triple also picks the tool, so x64 runs an arm64 `nfpm`. `release.yml` passes no `--target`, so only the documented example breaks.

**Fix:** Pick the tool by the machine.

**Blocks:** nothing. **Blocked by:** nothing.

**Touches:** scripts/package.mjs, architecture, tool, linux, bundleLibraries; crates/desktop/PACKAGING.md, "Building installers".

**Findings:**

- `29-3`: confirmed, P3. --target aarch64-unknown-linux-gnu on an x64 machine cannot work, though PACKAGING.md gives it as the example. Full text in `chunks/29.md`, verdict in `verdicts/B05.md`.

### 189. ARM64 Linux packages are drafted without the fresh-system check

P3, small, open.

`linux-packages` downloads `desktop-Linux-X64` only, while `draft` merges every artifact. PACKAGING.md says it checks each Linux package.

**Fix:** Check ARM64 too, or say so.

**Blocks:** nothing. **Blocked by:** nothing.

**Touches:** .github/workflows/release.yml, job linux-packages; crates/desktop/PACKAGING.md, "Releases and updates".

**Findings:**

- `29-4`: confirmed, P3. ARM64 Linux packages are drafted without the fresh-system check. Full text in `chunks/29.md`, verdict in `verdicts/B05.md`.

### 190. The .deb and .rpm state no C library version

P3, small, open.

Neither list names a C library or version, and nfpm computes none, so a package installs where the app cannot start. Which symbols the binary needs was not read. `29-S1` suggests a named older base.

**Fix:** State the version.

**Blocks:** nothing. **Blocked by:** nothing.

**Touches:** scripts/package.mjs, LINUX_DEPENDS.

**Findings:**

- `29-5`: confirmed, P3. The .deb and .rpm state no C library version, so they install where the app cannot start. Full text in `chunks/29.md`, verdict in `verdicts/B05.md`.

### 191. The icon holds its own unchecked copy of the mark

P3, small, open.

`icon.svg` repeats the points and widths of `mark.json` by hand. They match today, and only `mark.rs` and `AppMark.svelte` read the data.

**Fix:** Generate it, or test it.

**Blocks:** nothing. **Blocked by:** nothing.

**Touches:** crates/desktop/assets/icons/icon.svg.

**Findings:**

- `29-6`: confirmed, P3. The icon holds its own copy of the mark's strokes, unchecked. Full text in `chunks/29.md`, verdict in `verdicts/B05.md`.

### 192. Two copies of the read-until-full loop have drifted

P3, small, open.

`fill` retries `Interrupted` and `read_up_to` does not. A regular file does not return it in practice, so the cost is the copy.

**Fix:** Share `fill`, which is generic over `Read`.

**Blocks:** nothing. **Blocked by:** nothing.

**Touches:** crates/streams/src/crypto.rs fill, crates/streams/src/ffmpeg.rs read_up_to.

**Findings:**

- `15-4`: confirmed, P3. Two copies of the "read until full" loop have drifted. Full text in `chunks/15.md`, verdict in `verdicts/B06.md`.

### 193. Code keeps work folders of a layout that was never released

P3, small, open.

The header, a branch and a test keep work folders of layout 1, and no release ever shipped: `git tag` is empty.

**Fix:** Remove them.

**Blocks:** nothing. **Blocked by:** nothing.

**Touches:** crates/streams/src/workdir.rs PREVIOUS_VERSION, WorkDir::open, test a_folder_of_the_layout_before_is_kept.

**Findings:**

- `15-5`: confirmed, P3. Code that keeps work folders of a layout that was never released. Full text in `chunks/15.md`, verdict in `verdicts/B06.md`.

### 194. axum is a dev-dependency nothing in streams uses

P3, small, open.

Nothing under `crates/streams` names `axum`, and `cargo check` passes with the line removed.

**Fix:** Remove it.

**Blocks:** nothing. **Blocked by:** nothing.

**Touches:** crates/streams/Cargo.toml.

**Findings:**

- `15-6`: confirmed, P3. axum is a dev-dependency that nothing in the crate uses. Full text in `chunks/15.md`, verdict in `verdicts/B06.md`.

### 195. The path-escape half of a media test cannot fail

P3, small, open.

The test writes the URL where `remember_placing` writes a digest, so `waiting_to_place` returns `None` before `inside` is reached.

**Fix:** Write the digest. The mutation was not run.

**Blocks:** nothing. **Blocked by:** nothing.

**Touches:** crates/core/src/manager/media_run/tests.rs, a_waiting_file_belongs_to_its_link.

**Findings:**

- `17-8`: confirmed, P3. The path-escape half of a_waiting_file_belongs_to_its_link cannot fail. Full text in `chunks/17.md`, verdict in `verdicts/B06.md`.

### 196. Doc comments on media runs sit on the wrong items

P3, small, open.

`disk`'s doc describes another function, `MAX_NAME_ATTEMPTS` carries `place`'s description, and a test's doc opens with a sentence about a deleted queue.

**Fix:** Move or remove them.

**Blocks:** nothing. **Blocked by:** nothing.

**Touches:** media_run/mod.rs, disk; media_run/placement.rs, MAX_NAME_ATTEMPTS and place; crates/core/tests/manager/media.rs, a_video_clicked_again_shows_its_question.

**Findings:**

- `17-10`: confirmed, P3. Doc comments sit on the wrong items. Full text in `chunks/17.md`, verdict in `verdicts/B06.md`.

### 197. Sizes and bit rates a page chooses are added without a check

P3, small, open.

`video_offer` adds sizes a page chooses with no check. A dev build panics; a shipped build shows a wrapped size.

**Fix:** Use saturating addition.

**Blocks:** nothing. **Blocked by:** nothing.

**Touches:** crates/media/src/choice.rs, video_offer and estimate; crates/media/src/output/variants.rs, variant; crates/media/src/lib.rs, MediaProgress::read.

**Findings:**

- `16-5`: confirmed, P3. Sizes and bit rates a page chooses are added without a check. Full text in `chunks/16.md`, verdict in `verdicts/B07.md`.

### 198. The key error prints the whole signed address

P3, small, open.

`Error::Key` holds the whole signed address and `describe::stream` uses it as the message, up to 8 KiB. `Error::Manifest` keeps only the host. Whether it reaches what is saved was not traced.

**Fix:** Keep only the host.

**Blocks:** nothing. **Blocked by:** nothing.

**Touches:** crates/streams/src/error.rs, Error::Key; built in crates/streams/src/download/fetch.rs.

**Findings:**

- `14-6`: confirmed, P3. The key error prints the whole signed address to the person and into what is saved. Full text in `chunks/14.md`, verdict in `verdicts/B08.md`.

### 199. On Linux, media programs may outlive an app ended by a signal

P3, medium, open.

No `crates/` code outside the CLI handles a signal, and `Tree::kill` runs only from a live process. When the app ends on `SIGTERM`, the pipes close, and how yt-dlp and ffmpeg each answer was not checked, so "goes on downloading" is not established. Linux was not run.

**Fix:** The supervisor the roadmap names would cover both this and a crash.

**Blocks:** nothing. **Blocked by:** nothing.

**Roadmap:** The crash half is the open item "On Linux, yt-dlp and ffmpeg keep running after the app crashes", so `docs/ROADMAP.md` lists it once under that name. Only the `SIGTERM` half is new. `Xcrash-and-resume-S3` asks to add file corruption to that item.

**Touches:** crates/streams/src/process/tree.rs, Tree.

**Findings:**

- `Xshutdown-4`: downgraded to P3. On Linux, media programs outlive an app ended by a signal or a crash. Full text in `cross/shutdown.md`, verdict in `verdicts/B08.md`.

### 200. Placing a finished video has its own weaker no-replace move

P3, small, fixed.

`place` falls to `std::fs::rename` on any `hard_link` error other than `AlreadyExists`, and that replaces. It needs a drive without hard links, and a short window.

**Fix:** Use `rename_no_replace`, as `relocate` does.

**Blocks:** nothing. **Blocked by:** nothing.

**Touches:** crates/core/src/manager/media_run/placement.rs, place; against crates/engine/src/naming.rs, rename_no_replace, and crates/core/src/manager/relocate.rs, move_file.

**Findings:**

- `Xrepeated-logic-4`: confirmed, P3. Placing a finished video has its own move that never replaces, weaker than the engine's. Full text in `cross/repeated-logic.md`, verdict in `verdicts/B10.md`.

### 201. Moving a finished video leaves its subtitle files behind

P3, small, open.

For a completed download `move_download_files` moves only the video, so `<name>.<language>.srt` stays behind. "Remove and delete" also leaves them; that was noted but not validated.

**Fix:** List the sidecars in both. `17-S1` asks the same for Replace.

**Blocks:** nothing. **Blocked by:** nothing.

**Touches:** crates/core/src/manager/relocate.rs, move_download_files; crates/core/src/manager/media_run/placement.rs, finish, sidecar_path.

**Findings:**

- `B10-1`: confirmed, P3. Moving a finished video leaves its subtitle files in the old folder. Full text in `verdicts/B10.md`, verdict in `verdicts/new-findings.md`.

### 202. A failed browser registration logs no reason

P3, small, fixed.

`Report::fail` lines reach only stdout, and `run_bridge` reports only stderr, so a failed browser registration logs no reason.

**Fix:** Log them.

**Blocks:** nothing. **Blocked by:** nothing.

**Touches:** crates/shell/src/bridge.rs (run_bridge, BridgeError::Failed), crates/bridge/src/install/mod.rs (run, print_report).

**Findings:**

- `22-5`: confirmed, P3. A failed browser registration logs no reason. Full text in `chunks/22.md`, verdict in `verdicts/B10.md`.

### 203. The autostart Exec line escapes reserved characters once

P3, small, fixed.

`exec_word` writes `\$`, while the Desktop Entry Specification applies the string escape first, so it should be `\\$`. Read, not run.

**Fix:** Escape twice, and change its test.

**Blocks:** nothing. **Blocked by:** nothing.

**Touches:** crates/shell/src/autostart.rs (exec_word, and its test a_program_path_stays_one_word_of_the_exec_line).

**Findings:**

- `22-6`: confirmed, P3. The autostart Exec line escapes reserved characters once, so a path with $, a backtick, " or \ gives an entry desktops reject. Full text in `chunks/22.md`, verdict in `verdicts/B10.md`.

### 204. The autostart Launcher trait has one implementation and no test

P3, small, fixed.

`autostart::Launcher` has one implementation, and `apply` and `follow` run in no test.

**Fix:** Test the path, or remove the trait.

**Blocks:** nothing. **Blocked by:** nothing.

**Touches:** crates/shell/src/autostart.rs (Launcher, apply, Entry), crates/desktop/src/sign_in.rs (follow).

**Findings:**

- `22-8`: confirmed, P3. The autostart::Launcher trait has one implementation, and the path it was made to test has no test. Full text in `chunks/22.md`, verdict in `verdicts/B10.md`.

### 205. Every window has an empty native title

P3, small, fixed.

`title_bar_options` gives `title: None` and nothing calls `set_window_title`. Alt+Tab and screen readers have no name for the window.

**Fix:** Set the title.

**Blocks:** nothing. **Blocked by:** nothing.

**Touches:** crates/desktop/src/main.rs, window_options.

**Findings:**

- `U-2`: confirmed, P3. Every window has an empty native title. Full text in `ui.md`, verdict in `verdicts/B10.md`.

### 206. invalid_link ignores its argument, and its doc and test claim otherwise

P3, small, open.

The `InvalidUrl` arm drops the text callers format, the test compares the function with its own body, and the doc is untrue.

**Fix:** Use the text or remove the argument.

**Blocks:** nothing. **Blocked by:** nothing.

**Touches:** crates/messages/src/lib.rs, invalid_link and the test a_saved_link_that_no_longer_parses_reads_as_one_refused_when_added.

**Findings:**

- `28-5`: confirmed, P3. invalid_link ignores its argument, and its doc and test claim something untrue. Full text in `chunks/28.md`, verdict in `verdicts/B11.md`.

### 207. An import can leave two queues with the same name

P3, small, open.

`merge_localized` checks no queue name against the others, so a renamed default queue can meet a same-named one, and neither can be edited until renamed. A non-default queue named like the default is added beside it too.

**Fix:** Check names on merge.

**Blocks:** nothing. **Blocked by:** nothing.

**Touches:** crates/settings/src/settings_file.rs merge_localized.

**Findings:**

- `19-2`: downgraded to P3. An import can leave two queues with the same name, and then neither can be edited without renaming it. Full text in `chunks/19.md`, verdict in `verdicts/B12.md`.

### 208. An update offer names a yt-dlp older than the one installed

P3, small, fixed.

`latest_version` describes `wanted` without the record, and `status` prints it for `UpdateAvailable`, so an offer can name an older yt-dlp.

**Fix:** Take the text from the plan.

**Blocks:** nothing. **Blocked by:** nothing.

**Touches:** crates/core/src/manager/addons/mod.rs, Manager::addon_info (latest_version); shown by status in crates/desktop/src/main_window/settings/addon.rs.

**Findings:**

- `11-3`: confirmed, P3. An update offer names a yt-dlp older than the one installed. Full text in `chunks/11.md`, verdict in `verdicts/B12.md`.

### 209. swap_in spells out the release file's name

P3, small, fixed.

`swap_in` spells out what `release_file` gives, in the same file.

**Fix:** Call it.

**Blocks:** nothing. **Blocked by:** nothing.

**Touches:** crates/core/src/manager/addons/install.rs, swap_in and release_file.

**Findings:**

- `11-5`: confirmed, P3. swap_in spells out the release file's name instead of using release_file. Full text in `chunks/11.md`, verdict in `verdicts/B12.md`.

### 210. Ticking the first row pushes the list down under the pointer

P3, small, open.

The selection bar has no reserved space, so ticking the first row moves every row about 50 pixels, and a second tick lands in a row body and opens its details.

**Fix:** Reserve the space, or overlay the bar.

**Blocks:** nothing. **Blocked by:** nothing.

**Touches:** crates/desktop/src/main_window/selection.rs, the selection bar.

**Findings:**

- `U-4`: confirmed, P3. Ticking the first row pushes the list down under the pointer. Full text in `ui.md`, verdict in `verdicts/B13.md`.

### 211. The empty queue's Add button opens the dialog past the window's record

P3, small, open.

`empty_queue` drops the form `add_dialog::open` returns, past the window's record of it.

**Fix:** Go through `new_download`.

**Blocks:** nothing. **Blocked by:** nothing.

**Touches:** crates/desktop/src/main_window/empty.rs, empty_queue; crates/desktop/src/main_window/mod.rs, new_download.

**Findings:**

- `25-10`: confirmed, P3. The empty queue's Add button opens the dialog past the window's record of it. Full text in `chunks/25.md`, verdict in `verdicts/B13.md`.

### 212. `let _theme = cx.theme()` does nothing

P3, small, open.

Three unread bindings in `confirm.rs`.

**Fix:** Remove them.

**Blocks:** nothing. **Blocked by:** nothing.

**Touches:** crates/desktop/src/main_window/confirm.rs, RemoveForm::render and cancel; crates/desktop/src/main_window/details.rs, header.

**Findings:**

- `25-12`: confirmed, P3. let _theme = cx.theme(); does nothing. Full text in `chunks/25.md`, verdict in `verdicts/B13.md`.

### 213. The link's hover popover covers the row's menu

P3, small, open.

`link_line`'s tooltip has no check for an open menu, so it covers Get a new link and Try again.

**Fix:** Hide it while a menu is open.

**Blocks:** nothing. **Blocked by:** nothing.

**Touches:** crates/desktop/src/main_window/list.rs, the link line's hover popover; row_menu.rs.

**Findings:**

- `U-8`: confirmed, P3. The link's hover popover covers the row's right-click menu. Full text in `ui.md`, verdict in `verdicts/B13.md`.

### 214. The damaged-list notice squeezes its text and breaks the path

P3, small, open.

`tell_startup_notices` sets no width or shorter text, so the message runs to 13 lines, the path breaks mid-number, and it sits over the toolbar.

**Fix:** A wider, shorter notice.

**Blocks:** nothing. **Blocked by:** nothing.

**Touches:** crates/desktop/src/main_window/notices_bar.rs, tell_startup_notices.

**Findings:**

- `U-15`: confirmed, P3. The damaged-list notice squeezes its text and breaks the path. Full text in `ui.md`, verdict in `verdicts/B13.md`.

### 215. A top-level domain held only through a wildcard counts as a local name

P3, small, open.

`find` takes `node.leaf` for the first label, so `jm`, `ck` and five others read as not public. Only those registries could serve a host at the bare label, so no realistic site gains.

**Fix:** Honour wildcard parents.

**Blocks:** nothing. **Blocked by:** nothing.

**Touches:** crates/sites/src/public_suffix.rs, SuffixList::is_public_suffix; reached from crates/sites/src/local_network.rs, is_local_by_itself.

**Findings:**

- `10-2`: downgraded to P3. A top-level domain the list holds only through a wildcard rule counts as a local name. Full text in `chunks/10.md`, verdict in `verdicts/B14.md`.

### 216. direct_uses_no_proxy passes whatever Direct does

P3, small, open.

The test sets no proxy and checks only the size. The real proof is in `proxy/tests.rs`.

**Fix:** Remove it or set a proxy.

**Blocks:** nothing. **Blocked by:** nothing.

**Touches:** crates/engine/tests/proxy.rs, direct_uses_no_proxy.

**Findings:**

- `09-7`: confirmed, P3. direct_uses_no_proxy passes whatever Direct does. Full text in `chunks/09.md`, verdict in `verdicts/B15.md`.

### 217. Module docs describe proxy behaviour that has changed

P3, small, open.

The `proxy.rs` header says FTP is refused while `ftp_route` carries it, and the `PrivateNetwork` section says the proxy resolves every name, while reqwest resolves locally for `socks4` and `socks5`.

**Fix:** Correct them.

**Blocks:** nothing. **Blocked by:** nothing.

**Touches:** crates/engine/src/proxy.rs, module header; crates/engine/src/net_guard.rs, PrivateNetwork, section "With a proxy".

**Findings:**

- `09-9`: confirmed, P3. The module docs describe proxy behaviour that has changed. Full text in `chunks/09.md`, verdict in `verdicts/B14.md`.

### 218. A proxy test cancelled by quitting is saved as its last result

P3, small, fixed.

`test_one` turns `Cancelled` into `Unreachable`, and `keep_test` saves it with no check of `closed`. It is a race with the store closing.

**Fix:** Do not keep a cancelled result.

**Blocks:** nothing. **Blocked by:** nothing.

**Touches:** crates/core/src/proxy.rs test_one, crates/core/src/manager/proxy.rs test_in_background, keep_test.

**Findings:**

- `19-7`: confirmed, P3. A proxy test cancelled by quitting is saved as that proxy's last result. Full text in `chunks/19.md`, verdict in `verdicts/B15.md`.

### 219. A test named for keeping the last result does not check it

P3, small, fixed.

The last assertion reads only the return of `test_proxy_server`, so removing that `keep_test` call leaves every test passing.

**Fix:** Read `last_test`.

**Blocks:** nothing. **Blocked by:** nothing.

**Touches:** crates/core/tests/manager/proxy.rs a_test_uses_the_draft_and_the_saved_password_and_is_kept_when_saved.

**Findings:**

- `19-9`: confirmed, P3. A test named for keeping the result as the last test does not check it. Full text in `chunks/19.md`, verdict in `verdicts/B15.md`.

### 220. Copy from system turns a SOCKS4 server into a SOCKS5 profile

P3, small, fixed.

`parse_server` maps `socks4` to `ProxyKind::Socks5`, and a test pins it. A server speaking only SOCKS4 then fails every request. `ProxyKind` has no SOCKS4 (D73.1), and D73.9 does not say what a copy becomes.

**Fix:** Refuse the copy with a reason, or record a decision.

**Blocks:** nothing. **Blocked by:** nothing.

**Touches:** crates/settings/src/proxy.rs, parse_server; against crates/engine/src/proxy.rs, Proxies::adopt.

**Findings:**

- `B15-2`: confirmed, P3. Copy from system turns a SOCKS4 server into a SOCKS5 profile. Full text in `verdicts/B15.md`, verdict in `verdicts/new-findings.md`.

### 221. An impossible port is saved as the usual port without a word

P3, small, open.

The port's input sets `min` and `max` but no number mask, so `70000`, `80a` and an empty field all become the usual port. `queue_dialog.rs` `whole` refuses the same input.

**Fix:** Refuse it the same way.

**Blocks:** nothing. **Blocked by:** nothing.

**Touches:** crates/desktop/src/main_window/proxy_dialog.rs, ProxyForm::settings.

**Findings:**

- `27-9`: confirmed, P3. An impossible port is saved as the usual port without a word. Full text in `chunks/27.md`, verdict in `verdicts/B15.md`.

### 222. A file name or host holding a secret word loses the rest of its log line

P3, small, fixed.

`is_name_byte` takes `.` and `-`, so `key` or `session` inside a file name or host followed by a colon hides the rest of the line. D40.10 says the log keeps hosts and paths.

**Fix:** Tighten what counts as a field name.

**Blocks:** nothing. **Blocked by:** nothing.

**Touches:** crates/logging/src/redact/named.rs, kind_of and secret_value; crates/protocol/src/secrets.rs, segments.

**Findings:**

- `20-2`: confirmed, P3. A file name or host holding a secret word, followed by a colon, loses the rest of its line in the log. Full text in `chunks/20.md`, verdict in `verdicts/B15.md`.

### 223. The rules help says the highest queue wins, but for sites the most specific does

P3, small, fixed.

`HELP` says the highest in the sidebar wins, and `best_site` keeps the most specific.

**Fix:** Correct the help.

**Blocks:** nothing. **Blocked by:** nothing.

**Touches:** crates/desktop/src/main_window/queue_rules.rs, HELP.

**Findings:**

- `27-10`: confirmed, P3. The rules help says the highest queue wins, but for sites the most specific one does. Full text in `chunks/27.md`, verdict in `verdicts/B16.md`.

### 224. A dotted file type is accepted in a category rule and never matches

P3, small, fixed.

`clean_extensions` keeps `tar.gz`, and `category_of` looks only after the last dot, so it never matches. `file_type_rule` honours the same entry.

**Fix:** Match by the end of the name.

**Blocks:** nothing. **Blocked by:** nothing.

**Touches:** crates/settings/src/lib.rs, category_of and clean_extensions; against crates/queues/src/rules.rs, file_type_rule.

**Findings:**

- `Xrepeated-logic-5`: confirmed, P3. A file type with a dot is accepted in a category rule and never matches, though queue rules honour it. Full text in `cross/repeated-logic.md`, verdict in `verdicts/B16.md`.

### 225. Queue page numbers start at 2 and number failed rows

P3, small, open.

`queue_line` keeps every row that is not over, so failed, paused and running rows take a place, and a row can read "2" while its reason says "1st in line". D47.4 does not cover failed rows.

**Fix:** Number only waiting rows.

**Blocks:** nothing. **Blocked by:** nothing.

**Touches:** crates/desktop/src/main_window/list.rs, time_cell, queue view.

**Findings:**

- `U-5`: confirmed, P3. On a queue page the row numbers start at 2 and number failed rows. Full text in `ui.md`, verdict in `verdicts/B16.md`.

### 226. A queue using a proxy with no name reads "via" and nothing after it

P3, small, open.

`marks` clones an empty name and `summary` formats `via {proxy}`. `proxy_dialog::shown_name` already falls back to the address.

**Fix:** Use it.

**Blocks:** nothing. **Blocked by:** nothing.

**Touches:** crates/desktop/src/main_window/sidebar.rs, summary, and where its proxy argument is read.

**Findings:**

- `U-11`: confirmed, P3. A queue using a proxy with no name reads "via" and nothing after it. Full text in `ui.md`, verdict in `verdicts/B16.md`.

### 227. The queue page's facts line is cut off with no way to read the end

P3, small, open.

The line truncates with no tooltip, and the current state comes last, so it is the part lost.

**Fix:** Put the state first, or add a tooltip.

**Blocks:** nothing. **Blocked by:** nothing.

**Touches:** crates/desktop/src/main_window/queue_header.rs.

**Findings:**

- `U-12`: confirmed, P3. The queue page's facts line is cut off at the default window size, with no way to read the end. Full text in `ui.md`, verdict in `verdicts/B16.md`.

### 228. The details panel says Merging for every processing step

P3, small, open.

`status_label(Processing)` is fixed, though the same panel's sentence names the real step.

**Fix:** Name the step.

**Blocks:** nothing. **Blocked by:** nothing.

**Touches:** crates/desktop/src/text.rs, status_label; used by main_window/details.rs in the header.

**Findings:**

- `23-4`: downgraded to P3. The details panel says "Merging video and audio" for every processing step. Full text in `chunks/23.md`, verdict in `verdicts/B18.md`.

### 229. The system accent breaks the promised contrast

P3, small, open.

The doc promises 4.5:1, and the system accent gives as little as 2.34 with stock Windows colours. The default accent is Cyan, and `Accent::System` is opt-in.

**Fix:** Adjust a system accent to pass, or correct the doc.

**Blocks:** nothing. **Blocked by:** nothing.

**Touches:** crates/desktop/src/theme.rs, accent_colors.

**Findings:**

- `23-7`: downgraded to P3. The system accent breaks the promised 4.5 contrast with stock Windows colours. Full text in `chunks/23.md`, verdict in `verdicts/B18.md`.

### 230. Check for updates and Copy from system give no answer when there is nothing to do

P3, small, open.

`command` returns silently on `Ok`, and both buttons discard the result, so nothing changes on screen when there is nothing to do.

**Fix:** Say so.

**Blocks:** nothing. **Blocked by:** nothing.

**Touches:** crates/desktop/src/main_window/settings/addon.rs, buttons; crates/desktop/src/main_window/settings/network.rs, servers.

**Findings:**

- `27-7`: confirmed, P3. Check for updates and Copy from system give no answer when there is nothing to do. Full text in `chunks/27.md`, verdict in `verdicts/B18.md`.

### 231. A download waiting on the question reads Paused at Size unknown

P3, small, open.

`Paused` here is a recorded decision (D88.3). What remains is wording: "Paused at Size unknown", a provisional ` (1)` name until the answer, and "The partial file was removed" for every cancelled row.

**Fix:** Word the waiting state in `text.rs`, `list.rs` and `run_panel`.

**Blocks:** nothing. **Blocked by:** nothing.

**Touches:** crates/desktop/src/download_windows/view.rs, progress state; crates/desktop/src/main_window/list.rs, the paused row's text.

**Findings:**

- `U-6`: downgraded to P3. A download waiting on the file-exists question shows as Paused, with a name that was never chosen and "Paused at Size unknown". Full text in `ui.md`, verdict in `verdicts/B17.md`.

### 232. The complete window stays open and blank once its rows are removed

P3, small, open.

`follow` only notifies, and `render` has no empty state, so the window stays blank once its rows are removed.

**Fix:** Close it, or say so.

**Blocks:** nothing. **Blocked by:** nothing.

**Touches:** crates/desktop/src/download_windows/finished.rs, follow, FinishedView::render.

**Findings:**

- `24-10`: confirmed, P3. The complete window stays open and blank once its rows are removed. Full text in `chunks/24.md`, verdict in `verdicts/B17.md`.

### 233. The notice of a failed save ignores Do Not Disturb

P3, small, open.

`storage_failed` passes a literal `true`, against D64.2. It needs Do Not Disturb, a changed switch, no main window and a failing database.

**Fix:** Follow `could_not`.

**Blocks:** nothing. **Blocked by:** nothing.

**Touches:** crates/desktop/src/notices.rs, storage_failed.

**Findings:**

- `26-6`: downgraded to P3. The notice of a failed save ignores the Do Not Disturb setting. Full text in `chunks/26.md`, verdict in `verdicts/B17.md`.

### 234. The language list names Twi "Akan" and gives Ossetic itself as its other name

P3, small, open.

`tw` is named "Akan", and `os` has `also_called: Some("Ossetic")`, so "Twi" and "Ossetian" find nothing, against the module doc.

**Fix:** Correct the two entries. `30-S2` is related.

**Blocks:** nothing. **Blocked by:** nothing.

**Touches:** crates/protocol/src/languages.rs, LANGUAGES, the entries tw and os.

**Findings:**

- `30-7`: confirmed, P3. The language list names Twi "Akan" and gives Ossetic itself as its other name. Full text in `chunks/30.md`, verdict in `verdicts/B19.md`.

### 235. The comment on RecvError::Closed in the server's loop is wrong

P3, small, open.

`serve_connection` owns a `Manager`, so `wire_events` outlives the receiver and `Closed` cannot happen. The arm must stay for the match; its comment misleads about shutdown.

**Fix:** Correct the comment.

**Blocks:** nothing. **Blocked by:** nothing.

**Touches:** crates/server/src/lib.rs, serve_connection.

**Findings:**

- `07-6`: confirmed, P3. RecvError::Closed in the server's loop can never happen. Full text in `chunks/07.md`, verdict in `verdicts/B19.md`.

### 236. Manager::update_settings has no caller outside tests

P3, small, open.

Every call is in a test or `crates/e2e`, and the body repeats `change_settings`.

**Fix:** Remove it from the public surface.

**Blocks:** nothing. **Blocked by:** nothing.

**Touches:** crates/core/src/manager/preferences.rs update_settings.

**Findings:**

- `19-10`: confirmed, P3. Manager::update_settings has no caller outside tests, and repeats change_settings. Full text in `chunks/19.md`, verdict in `verdicts/B19.md`.

### 237. lock_path repeats with_suffix

P3, small, open.

`lock_path` is `with_suffix(database, ".lock")` written out, in a file that imports `with_suffix`.

**Fix:** Call it.

**Blocks:** nothing. **Blocked by:** nothing.

**Touches:** crates/store/src/lib.rs, lock_path; crates/files/src/lib.rs, with_suffix.

**Findings:**

- `03-9`: confirmed, P3. lock_path repeats with_suffix. Full text in `chunks/03.md`, verdict in `verdicts/B19.md`.

### 238. Product tests are pinned to today's config values

P3, small, open.

The tests assert `version = "0.1.0"` and five live placeholder values, so a version bump or filling in the identity fails them.

**Fix:** Use sample input.

**Blocks:** nothing. **Blocked by:** nothing.

**Touches:** scripts/product/read.test.mjs and scripts/gate/product.test.mjs.

**Findings:**

- `33-3`: confirmed, P3. The product tests are pinned to today's values of config/product.toml, so a version bump or filling in the identity fails them. Full text in `chunks/33.md`, verdict in `verdicts/B19.md`.

### 239. Two copies of the pnpm-workspace parser

P3, small, open.

`workspaceParents` and `packageFolders` hold the same list regex, glob regex and `folder/*` rule.

**Fix:** One function.

**Blocks:** nothing. **Blocked by:** nothing.

**Touches:** workspaceParents in scripts/gate/js-policy.mjs, packageFolders in scripts/product/outputs.mjs.

**Findings:**

- `33-4`: confirmed, P3. Two copies of the parser for packages in pnpm-workspace.yaml. Full text in `chunks/33.md`, verdict in `verdicts/B19.md`.

### 240. Dead code in the product generator

P3, small, open.

`withNote` has one product caller, the `.sh`, `.nsh` and `#!` branches run only in tests, and nothing imports `GENERATED_NOTE`.

**Fix:** Remove them.

**Blocks:** nothing. **Blocked by:** nothing.

**Touches:** COMMENT_STYLES and the keepFirstLine branch of withNote in scripts/product/manifests.mjs; export { GENERATED_NOTE } in scripts/product/outputs.mjs.

**Findings:**

- `33-5`: confirmed, P3. Dead code in the product generator. Full text in `chunks/33.md`, verdict in `verdicts/B19.md`.

### 241. Comments in the checked configuration describe things that are gone

P3, small, open.

Comments in `config/product.toml`, `Cargo.toml`, `browser.mjs` and `.githooks/commit-msg` describe things that are gone.

**Fix:** Correct them.

**Blocks:** nothing. **Blocked by:** nothing.

**Touches:** config/product.toml (header), Cargo.toml (members), scripts/gate/browser.mjs (header), .githooks/commit-msg, scripts/linux-test.sh (--exclude='./apps/desktop/build'), scripts/product/read.mjs (IDENTIFIER).

**Findings:**

- `33-6`: confirmed, P3. Comments in the checked configuration describe things that are gone. Full text in `chunks/33.md`, verdict in `verdicts/B19.md`.

### 242. Two test helpers wait for progress with no deadline

P3, small, open.

`pause_midway` and `another_program_saves_meanwhile` loop with no deadline and never look at the spawned task, so a download that fails early hangs the test until the CI job's limit.

**Fix:** Wait under `PATIENCE`, and fail when the task ends.

**Blocks:** nothing. **Blocked by:** nothing.

**Touches:** crates/engine/tests/download.rs, pause_midway, another_program_saves_meanwhile.

**Findings:**

- `01-9`: confirmed, P3. Two test helpers wait for progress with no deadline. Full text in `chunks/01.md`, verdict in `verdicts/B19.md`.

### 243. A roaming data folder that could not be deleted keeps the list with its cookies

P3, small, fixed.

`merge` and `copy_then_delete` only log a failed delete, and `needs_move` is false once the local folder holds a list. So a roaming copy stays for good, cookies included. Only a data folder an earlier version left, on Windows.

**Fix:** Try the delete again at start. A held file was not tested.

**Blocks:** nothing. **Blocked by:** nothing.

**Touches:** crates/shell/src/host.rs, merge, copy_then_delete, needs_move.

**Findings:**

- `Xdatabase-migrations-4`: confirmed, P3. A roaming data folder that could not be deleted keeps the whole list with its cookies for good. Full text in `cross/database-migrations.md`, verdict in `verdicts/B09.md`.

### 244. The `<local>` queue rule is never tested against the real network check

P3, small, open.

All three mutants of `is_on_own_network` survive: `queues` tests `<local>` with a stand-in, and `placement` tests use named sites. The code is right today.

**Fix:** Test `rule_among` with the real check.

**Blocks:** nothing. **Blocked by:** nothing.

**Touches:** crates/sites/src/local_network.rs, is_on_own_network, as used by rule_among in crates/core/src/manager/placement.rs.

**Findings:**

- `M-4`: real gap, P3. The <local> queue rule is never tested against the real network check. Full text in `verdicts/mutation.md`, verdict in `verdicts/mutation.md`.

## Suggestions

Better ways of doing something that already works. Not validated, no severity, no number in the fix order; each waits until the person accepts it, one by one. Each line gives what the person gains.

### Downloads and resume

- **Ask again without a range when the first 206 cannot be read** (`02-S1`, in `chunks/02.md`). A download that otherwise cannot start works, at the cost of no resume on that server.
- **Check a first answer whose range stops short with no size** (`02-S2`, in `chunks/02.md`). A truncated file is not reported as finished.
- **Keep cookies set during a redirect chain** (`02-S3`, in `chunks/02.md`). Links behind a cookie check download in the app without going through the browser.
- **Accept FTP links whose path is not UTF-8** (`21-S1`, in `chunks/21.md`). Files with non-ASCII names on older FTP servers can be downloaded instead of failing with a message the person cannot act on.
- **Hold the part file exclusively during a run** (`Xconcurrency-S1`, in `cross/concurrency.md`). A future bookkeeping bug costs a person one failed download and a message, not a finished file with the wrong content.
- **Say early when the file will not fit** (`Xsecond-review-disk-writes-S1`, in `cross/second-review-disk-writes.md`). A person learns at the start that a large file will not fit, instead of after hours of downloading. Overlaps finding `01-3`.
- **Check kept bytes when a server gives no validators** (`Xsecond-review-resume-S1`, in `cross/second-review-resume.md`). The case most likely to give a silently mixed file gets caught, for one small request.
- **Finish a complete part file without asking the server** (`Xsecond-review-resume-S2`, in `cross/second-review-resume.md`). A fully downloaded file is never lost to an expired link, and the person is not told it failed.
- **Keep every name the app uses for its own files away from downloads** (`Xsecurity-path-handling-S1`, in `cross/security-path-handling.md`). A site cannot, even by accident, make one download look unfinished or fail because of another's name.

### Saved list and crash recovery

- **Save what can be read from a damaged list** (`03-S1`, in `chunks/03.md`). A damaged page costs a few rows instead of the whole list and every setting.
- **Stop a download that crashes the app from crashing it at every start** (`Xcrash-and-resume-S1`, in `cross/crash-and-resume.md`). One bad download cannot make the app unusable until the list is edited by hand.
- **Offer to clean up partial files no row owns** (`Xcrash-and-resume-S2`, in `cross/crash-and-resume.md`). Disk space comes back, and later downloads of that name do not get a ` (1)` for no visible reason.
- **Read the version before the integrity check** (`Xdatabase-migrations-S1`, in `cross/database-migrations.md`). A person who goes back to an older build sees "Update Wirecatch" with their list in place, rather than an empty list and a healthy list set aside.

### Start and quit

- **Stop downloads at sign-out and system shutdown** (`22-S1`, `Xshutdown-S1`, in `chunks/22.md`). A clean stop at the most common way a day ends, instead of relying on crash recovery every time.
- **Let a second launch bring up the window of a copy whose list failed to open** (`22-S2`, in `chunks/22.md`). The message "Restart Wirecatch" can be followed from the Start menu without finding the tray's Quit first.
- **Say that work is still finishing when a quit has to wait** (`Xshutdown-S2`, in `cross/shutdown.md`). A person does not eject a disk or shut down thinking the app has finished.

### Captures and links with passwords

- **Forget a password in a captured link along with the rest of the sign-in** (`07-S1`, in `chunks/07.md`). A password never sits in the list for longer than the sign-in beside it. `Copy link` and `Try again` on a finished download would lose it, so this is a trade-off for the person to accept.
- **Copy link gives a link that works** (`25-S1`, in `chunks/25.md`). A copied link does what the button implies, and the person knows when it will not.

### App updates

- **Keep a finished, checked installer for Try again** (`12-S1`, in `chunks/12.md`). A second click starts the setup at once, with no second download.
- **Let installed copies read manifests from later versions** (`12-S3`, `Xpath-app-update-S2`, `Xsecurity-update-signatures-S5`, in `chunks/12.md`). A later release can add a package format without cutting older copies off from updates.
- **Build the x64 Linux packages on a named, older base** (`29-S1`, in `chunks/29.md`). An AppImage that starts on older distributions, and a release whose requirements change only by a commit.

### Media add-on

- **Try a folder move again briefly before failing it** (`11-S1`, in `chunks/11.md`). Fewer failed installs on first use, the moment a person has just asked for a video.
- **A repair downloads only what is damaged** (`Xsecurity-addon-unpacking-S1`, in `cross/security-addon-unpacking.md`). A repair after an antivirus took one file, or an install that failed in the swap, takes seconds and uses little data. Overlaps finding `11-1`.
- **Flush unpacked files before the record says they are there** (`Xsecurity-addon-unpacking-S2`, in `cross/security-addon-unpacking.md`). An install that survives a power cut is either whole or shown as needing repair, at a small cost, since an install writes few files and rarely runs.

### Video and audio

- **Bound the wait after killing a program** (`13-S1`, in `chunks/13.md`). A download stuck on a broken driver can still be cancelled and frees its place in the queue.
- **Replace a video's subtitle files when the video is replaced** (`17-S1`, in `chunks/17.md`). The subtitles a player opens belong to the video in the folder.
- **Stop the other part as soon as one part fails for good** (`17-S2`, in `chunks/17.md`). The person hears of a failure when it happens, and a refresh of a refused address starts sooner.
- **Check saved subtitle languages against the codes sites really use** (`30-S2`, in `chunks/30.md`). A saved language is ticked for every video that has it.
- **Try yt-dlp once when the engine's fetch of a format keeps being refused** (`Xpath-media-download-S1`, in `cross/path-media-download.md`). Fewer failed videos on sites that treat a plain client differently, whatever the cause.

### Queues

- **A start time passed while the app was closed could count, as one passed during sleep does** (`18-S1`, in `chunks/18.md`). A night queue still runs on a computer that was off or booted late.

### Proxies

- **Save a pasted web address in the proxy exceptions as its host** (`19-S1`, in `chunks/19.md`). Pasting an address from the browser just works.

### Main window

- **The palette offers Cancel** (`25-S2`, in `chunks/25.md`). A keyboard user can cancel a download without the menu key.
- **Do not move rows while the pointer is over the list** (`U-S4`, in `ui.md`). Fewer clicks on the wrong row in a busy list.
- **Details: label the footer icons and drop the second Try again** (`U-S5`, in `ui.md`). The dialog says each thing once, and a healthy paused download does not suggest a problem.
- **Activity should record what happened** (`U-S8`, in `ui.md`). The person can see when and why a download stopped.
- **Hold the toast clear of the toolbar** (`U-S12`, in `ui.md`). The toolbar stays usable for the seconds it shows.

### Download windows

- **The failed download window should offer the same fix as the list** (`U-S6`, in `ui.md`). The person is not sent round a retry that cannot work. The same request as finding `30-2`, so fixing that item settles it.
- **Say the add-on's size in the offer** (`U-S7`, in `ui.md`). No surprise on a metered connection.
- **The already-downloaded warning and the file-exists question ask twice** (`U-S11`, in `ui.md`). One decision in one window.

### Settings and forms

- **Test a saved proxy with a visible state** (`27-S1`, in `chunks/27.md`). The person knows the click took effect and does not press it again.
- **Let a type's folder go back to the download folder** (`27-S2`, in `chunks/27.md`). Moving the download folder later does not leave types behind.
- **Say where a settings folder really is** (`U-S1`, in `ui.md`). A person sees where files go without opening a folder picker. The open roadmap item "The Download folder field may read as a placeholder" is the same point, so it is not listed twice.
- **Reword the subtitle format help** (`U-S2`, in `ui.md`). The sentence no longer has to be read twice.
- **Offer the logs where the app asks for them** (`U-S3`, in `ui.md`). A report arrives with logs, without the person having to find `%LOCALAPPDATA%`.
- **Queue names in Settings sentences should stand out** (`U-S9`, in `ui.md`). A made-up queue name is not misread as words of the sentence.
- **Follow the Time format in the schedule editor** (`U-S10`, in `ui.md`). The time typed is the time the app then says.

### Notifications

- **Tell what ended during a lag** (`26-S1`, in `chunks/26.md`). A failure is told even after a burst of events, such as a bulk action on a long list.

### From the feature map

`FEATURES.md` records how other download managers handle core features. A difference is not a finding, and only two of its lines point at something a person would clearly gain from:

- **A way from Get a new link to the source page.** IDM's "Refresh download address" opens the page and picks up the new link. Here the person has to find a fresh link themselves and paste it.
- **Shut the computer down when a queue ends.** IDM offers it for scheduled queues, which suits a night queue. Nothing here does that; whether it belongs is the person's call.

### Left out

These do not clearly help the person using the app: they are tests, release tooling, repository housekeeping or hardening with no failure a person would notice. Their text stays in the files named by the ID: `06-S1`, `12-S2`, `30-S1`, `33-S1`, `Xcrash-and-resume-S3`, `Xdatabase-migrations-S2`, `Xfeature-behaviour-S1`, `Xpath-app-update-S1`, `Xpath-media-download-S2`, `Xsecurity-update-signatures-S1`, `Xsecurity-update-signatures-S2`, `Xsecurity-update-signatures-S3`, `Xsecurity-update-signatures-S4`. `Xfeature-behaviour-S1` and `Xpath-media-download-S2` are named as checks on items 7 and 23, and `Xcrash-and-resume-S3` on the Linux item.

## Coverage

### Chunks found clean

None. Each of the 33 chunks and each of the 15 flows has at least one confirmed finding. Most chunk findings are small, so this says little about how sound a chunk is overall.

### Flows and what each left out

These are reported as not reviewed, not as passing.

| Flow                                     | File                                                                  | Left out or only searched                                                                                                                                                                                                                  |
| ---------------------------------------- | --------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| Concurrency                              | `cross/concurrency.md`                                                | Races inside media questions and add-on installs: `core/manager/media.rs`, `core/manager/addons/`, `media_run/page.rs`, and the desktop's window code.                                                                                     |
| Crash and resume                         | `cross/crash-and-resume.md`                                           | `store/src/lib.rs`, `records`, `streams/download/fetch.rs`, `core/batches.rs`, `core/manager/media.rs`, `media_run/extras.rs`, so a batch's state across a crash is not covered. The store's own repair is covered by the migrations flow. |
| Database migrations                      | `cross/database-migrations.md`                                        | No build or test was run.                                                                                                                                                                                                                  |
| Feature behaviour                        | `cross/feature-behaviour.md`                                          | Connections were followed, not files read whole, and nothing ran in the app. `crates/server`, `apps/extension`, `release.yml` and the packaging scripts were searched only.                                                                |
| App update path                          | `cross/path-app-update.md`                                            | `manifest.test.mjs`, `updates/samples/update.json`, the engine's download loop. No real update ran.                                                                                                                                        |
| Media download path                      | `cross/path-media-download.md`                                        | `streams` `hls`, `dash`, `plan`, `select`, `workdir`, `download/fetch`; `media` `ffmpeg`, `encoders`, `metadata`, `cookies`; most of `media_form`, `media_output`, `added_pages`, `captures`.                                              |
| Normal download path                     | `cross/path-normal-download.md`                                       | The bridge's relay, `cli/src/main.rs`, the engine's `worker`, `transport`, `http` and `segment`, all taken from contracts.                                                                                                                 |
| Repeated logic                           | `cross/repeated-logic.md`                                             | Only suspected copies were read; the rest of each crate was searched.                                                                                                                                                                      |
| Second reviews of disk writes and resume | `cross/second-review-disk-writes.md`, `cross/second-review-resume.md` | Parts of `engine/naming.rs`, `fetch.rs`, `http`, `transport`, `core/manager/run.rs`, `downloads.rs`, `state.rs`, `relocate.rs`. Nothing was built or run against a full disk.                                                              |
| Add-on security                          | `cross/security-addon-unpacking.md`                                   | Add-on tests searched only; `process/tree.rs` from its contract; Linux read, not run. SHA-256 values and the Linux tar's contents were not checked (chunk 11).                                                                             |
| Bridge and pipe security                 | `cross/security-bridge-and-pipe-input.md`                             | `bridge/relay.rs`, `raw_frame.rs`, `caller.rs`, `shell_parent.rs` not read; how `streams` uses headers on segments not read; nothing was run against a real app, FTP server or yt-dlp.                                                     |
| Path handling security                   | `cross/security-path-handling.md`                                     | `relocate.rs`, `file_exists.rs`, `restore.rs` from contracts; `addons/archive.rs` left to the add-on flow; `streams/workdir.rs` and `download/join.rs` searched for path joins only.                                                       |
| Update signatures                        | `cross/security-update-signatures.md`                                 | `scripts/package.mjs` and the engine from contracts; nothing on Linux, and no update ran.                                                                                                                                                  |
| Shutdown                                 | `cross/shutdown.md`                                                   | The engine's `FileWork` and writer, `core/manager/addons/`, `media.rs`, `crates/server` and `crates/ipc`'s endpoint release from contracts; GPUI's Linux platform not read.                                                                |

### Untested

Stated as untested, not as passing.

- **Full disk through a real download.** No dedicated volume was provided. `a_failed_write_is_never_covered_by_the_next_one` proves the writer only, and part files are sparse on Windows, so a full disk arrives as a failed write mid-download.
- **Power loss,** and anything else that loses the system's write cache. A kill keeps it, so the kill tests say nothing here. `Xsecond-review-disk-writes-1`, `Xsecond-review-disk-writes-3`, `Xsecond-review-resume-7` and `Xrepeated-logic-6` rest on this by reading.
- **Linux.** Every run was on Windows, and a kill there is `TerminateProcess`. Findings on Linux behaviour (`12-3`, `22-6`, `29-2`, `Xshutdown-4`, `B18-1`, the Linux halves of `08-3` and `08-4`) are read, not run. Mutants in `cfg(unix)` code were never built.
- **HTTP/2, HTTP/3 and FTP faults.** The fault server speaks only HTTP/1.1, so kills and size faults were not run over them. Nothing ran against a real FTP server or a real app for the bridge flow.
- **Files over 4 GiB through `Manager`.** Only through the CLI (`FT-large`, 4 of 4 pass).
- **A real update.** No download, install and restart ran; the Windows approval prompt was not answered, since the audit cannot drive it. The roadmap's "Not tested yet" lists this too.
- **Real installers.** No installer was built or run in this audit (chunk 29), and an uninstall was not attempted. The roadmap lists this too.
- **UI states not reached:** the storage failure banner (M19), system notifications for failed and finished downloads (N1, N2), the backup dialog (Q6), the refresh link dialog (Q7), the update banner (O2), tray submenus, and any shortcut with Ctrl or Alt. Several states were seen in dark theme only, as `ui.md` marks them: M3, M15, M18, A2 to A4, N3, parts of D, M13, M14, M16 and Q1 to Q5. Clicks were posted messages, not a physical mouse.
- **Fault tests that passed, kept as proofs only:** `FT-ranges` (4 of 4), `FT-change` (5 of 5), `FT-large` (4 of 4), `FT-manager-kill` (7 of 7). The plan turns them into permanent regression tests during the fixing work; no item carries them except `F-1` and `F-2`, whose patches hold the harness.
- **Parts of proven findings the reruns do not cover** (`verdicts/proof-check.md`): the 977 GiB form of `14-1` and the wall times of `14-2`; real yt-dlp giving `cookies` in `Xpath-media-download-2`; a later flush passing with the bytes gone in `Xsecond-review-disk-writes-1`; a task stuck inside a run's `FileWork` in `Xshutdown-3`; the real-app keystrokes of `25-1`; the junk `USP10.dll` load of `Xsecurity-path-handling-2`; the Unix half of `12-1`. `05-1` was not run as a whole release build.
- **Batches judged from the code with nothing run:** B14, B15 (one cost measured outside the repo), B16, B17, B18 and B19 (small samples only). No full gate ran.
- **Tool limits.** Line coverage is 72.4% overall and 11% for `crates/desktop`; `crates/shell/src/teller.rs` has 0%. `crates/streams` is understated because 19 tests could not find a fake program in the coverage build. Mutation covered only the 15 files named in `CHUNK-PLAN.md`, under a 2-hour cap, with each run using only the mutated crate's own tests; 7 of its 77 missed mutants are real gaps (`M-1` to `M-4`). `jscpd` finds copied text only.
- **Leads from the fault tests that were not tested:** a same-size change with no validators can join two versions (D34.2), and a `Last-Modified` with no `Date` header can exit 0 with mixed bytes (D34.2).
- **Outside this scope:** `apps/extension` and `packages/`, which are the `extension` audit. Left out everywhere: generated code, data snapshots, binary files and standard licence texts, listed in `CHUNK-PLAN.md`, `## Left out`.

### Files reviewed through the chunks

631 files, as each chunk's `## Files read` records them.

- **01 engine-disk-resume** (31): `crates/engine/Cargo.toml`, `crates/engine/README.md`, `crates/engine/src/abandon.rs`, `crates/engine/src/checkpoint.rs`, `crates/engine/src/download.rs`, `crates/engine/src/file_work.rs`, `crates/engine/src/lib.rs`, `crates/engine/src/limiter.rs`, `crates/engine/src/naming.rs`, `crates/engine/src/numeric.rs`, `crates/engine/src/options.rs`, `crates/engine/src/progress.rs`, `crates/engine/src/receive.rs`, `crates/engine/src/segment.rs`, `crates/engine/src/segment/proptests.rs`, `crates/engine/src/state.rs`, `crates/engine/src/storage.rs`, `crates/engine/src/target.rs`, `crates/engine/src/units.rs`, `crates/engine/src/worker.rs`, `crates/engine/src/writer.rs`, `crates/engine/tests/common/mod.rs`, `crates/engine/tests/download.rs`, `crates/engine/tests/options.rs`, `crates/engine/tests/request_limit.rs`, `crates/engine/src/retry.rs`, `crates/engine/src/error.rs`, `crates/cli/src/main.rs`, `crates/core/src/naming.rs`, `crates/core/src/manager/run.rs`, `docs/DECISIONS.md`
- **02 engine-http** (23): `crates/engine/examples/bench_http3.rs`, `crates/engine/examples/bench_local.rs`, `crates/engine/src/error.rs`, `crates/engine/src/fetch.rs`, `crates/engine/src/http.rs`, `crates/engine/src/http/client.rs`, `crates/engine/src/http/range.rs`, `crates/engine/src/http/status.rs`, `crates/engine/src/http/tests.rs`, `crates/engine/src/http3.rs`, `crates/engine/src/http3/tests.rs`, `crates/engine/src/request.rs`, `crates/engine/src/retry.rs`, `crates/engine/src/transport.rs`, `crates/engine/tests/fetch.rs`, `crates/engine/tests/hardening.rs`, `crates/engine/src/download.rs`, `crates/engine/src/worker.rs`, `crates/engine/src/naming.rs`, `crates/messages/src/lib.rs`, `crates/core/src/headers.rs`, `crates/streams/src/download/fetch.rs`, `crates/logging/src/redact.rs`
- **03 saved-state** (42): `crates/files/Cargo.toml`, `crates/files/src/discard.rs`, `crates/files/src/error.rs`, `crates/files/src/lib.rs`, `crates/files/src/owner_only.rs`, `crates/files/src/whole_file.rs`, `crates/files/src/zone.rs`, `crates/records/Cargo.toml`, `crates/records/src/batch.rs`, `crates/records/src/lib.rs`, `crates/records/src/media/mod.rs`, `crates/records/src/media/output.rs`, `crates/records/src/origin.rs`, `crates/records/src/proxy.rs`, `crates/store/Cargo.toml`, `crates/store/samples/batches/example.json`, `crates/store/samples/download_error/example.json`, `crates/store/samples/last_ids/example.json`, `crates/store/samples/media/page_before_extras.json`, `crates/store/samples/media/page_before_subtitle_files.json`, `crates/store/samples/media/page_with_format_field.json`, `crates/store/samples/media/page_with_output.json`, `crates/store/samples/media/stream.json`, `crates/store/samples/media/unreadable.json`, `crates/store/samples/origin/forgot_sign_in.json`, `crates/store/samples/origin/with_headers.json`, `crates/store/samples/proxy_logins/example.json`, `crates/store/samples/proxy_servers/example.json`, `crates/store/samples/queue_settings/main.json`, `crates/store/samples/queue_settings/own_values.json`, `crates/store/samples/settings/defaults.json`, `crates/store/src/error.rs`, `crates/store/src/lib.rs`, `crates/store/tests/samples.rs`, `crates/core/src/manager/mod.rs`, `crates/core/src/manager/restore.rs`, `crates/core/src/manager/startup.rs`, `crates/desktop/src/library.rs`, `crates/desktop/src/notices.rs`, `crates/desktop/src/main_window/notices_bar.rs`, `src/row.rs`, `src/types/value_ref.rs`
- **04 core-run-lifecycle** (26): `crates/core/Cargo.toml`, `crates/core/examples/headless.rs`, `crates/core/src/clock.rs`, `crates/core/src/lib.rs`, `crates/core/src/manager/arrival.rs`, `crates/core/src/manager/downloads.rs`, `crates/core/src/manager/fake.rs`, `crates/core/src/manager/mod.rs`, `crates/core/src/manager/restore.rs`, `crates/core/src/manager/run.rs`, `crates/core/src/manager/startup.rs`, `crates/core/src/manager/state.rs`, `crates/core/src/manager/tests.rs`, `crates/core/src/manager/ticker.rs`, `crates/core/src/speed.rs`, `crates/core/tests/manager/common.rs`, `crates/core/tests/manager/lifecycle.rs`, `crates/core/tests/manager/main.rs`, `crates/core/tests/manager/restart.rs`, `crates/core/src/manager/file_exists.rs`, `media_run/mod.rs`, `media_run/placement.rs`, `crates/core/src/naming.rs`, `crates/queues/src/plan.rs`, `crates/store/src/lib.rs`, `crates/shell/src/quit.rs`
- **05 core-final-path** (12): `crates/core/src/manager/file_exists.rs`, `crates/core/src/manager/relocate.rs`, `crates/core/src/manager/settle.rs`, `crates/core/src/naming.rs`, `crates/core/tests/manager/file_exists.rs`, `crates/core/tests/manager/settle.rs`, `manager/run.rs`, `manager/state.rs`, `manager/downloads.rs`, `manager/captures.rs`, `engine/src/target.rs`, `settings/src/folders.rs`
- **06 bridge-and-frames** (28): `crates/bridge/Cargo.toml`, `crates/bridge/src/app.rs`, `crates/bridge/src/caller.rs`, `crates/bridge/src/config.rs`, `crates/bridge/src/main.rs`, `crates/bridge/src/raw_frame.rs`, `crates/bridge/src/relay.rs`, `crates/bridge/src/shell_parent.rs`, `crates/bridge/tests/bridge.rs`, `crates/bridge/tests/needs_overrides.rs`, `crates/e2e/Cargo.toml`, `crates/e2e/README.md`, `crates/e2e/src/app.rs`, `crates/e2e/src/bin/wirecatch-e2e-app.rs`, `crates/e2e/src/bridge.rs`, `crates/e2e/src/extension.rs`, `crates/e2e/src/lib.rs`, `crates/e2e/src/server.rs`, `crates/e2e/tests/e2e.rs`, `crates/protocol/Cargo.toml`, `crates/protocol/src/frame.rs`, `crates/protocol/src/lib.rs`, `crates/protocol/src/version.rs`, `crates/protocol/src/wire.rs`, `crates/server/src/lib.rs`, `crates/ipc/src/key.rs`, `apps/extension/src/lib/native/client.ts`, `crates/shell/src/appimage.rs`
- **07 browser-handover** (17): `crates/core/src/headers.rs`, `crates/core/src/manager/captures.rs`, `crates/core/src/manager/handovers.rs`, `crates/core/src/manager/questions.rs`, `crates/core/src/manager/sign_in.rs`, `crates/core/tests/manager/again.rs`, `crates/core/tests/manager/protocol.rs`, `crates/core/tests/manager/windows.rs`, `crates/protocol/src/extension.rs`, `crates/server/Cargo.toml`, `crates/server/src/lib.rs`, `crates/core/src/manager/settle.rs`, `crates/records/src/origin.rs`, `crates/sites/src/cookie_scope.rs`, `crates/engine/src/http.rs`, `http-1.4.0/src/header/value.rs`, `docs/DECISIONS.md`
- **08 local-endpoint** (13): `crates/ipc/Cargo.toml`, `crates/ipc/src/key.rs`, `crates/ipc/src/lib.rs`, `crates/ipc/src/paths.rs`, `crates/ipc/src/unix.rs`, `crates/ipc/src/windows.rs`, `crates/ipc/tests/channel.rs`, `crates/ipc/tests/frames.rs`, `crates/ipc/tests/keyed.rs`, `crates/bridge/src/app.rs`, `crates/desktop/src/main.rs`, `docs/DECISIONS.md`, `/wirecatch-audit/2026-11-desktop/chunk08-pipe-probe.ps1`
- **09 request-routing** (11): `crates/engine/src/net_guard.rs`, `crates/engine/src/proxy.rs`, `crates/engine/src/proxy/tests.rs`, `crates/engine/src/url.rs`, `crates/engine/tests/http_proxy_guard.rs`, `crates/engine/tests/proxy.rs`, `crates/engine/src/download.rs`, `http/client.rs`, `ftp/mod.rs`, `crates/sites/src/local_network.rs`, `crates/core/src/manager/captures.rs`
- **10 site-rules** (18): `crates/core/src/manager/suffix_list.rs`, `crates/sites/Cargo.toml`, `crates/sites/src/cookie_scope.rs`, `crates/sites/src/lib.rs`, `crates/sites/src/local_network.rs`, `crates/sites/src/public_suffix.rs`, `crates/sites/src/video_sites.rs`, `packages/protocol/src/cookie-scope.cases.json`, `crates/protocol/src/extension.rs`, `crates/core/src/manager/captures.rs`, `crates/settings/src/lib.rs`, `crates/core/src/manager/preferences.rs`, `crates/queues/src/sites.rs`, `crates/sites/data/public_suffix_list.dat`, `apps/extension/src/lib/hub/hub.ts`, `hub/menu.ts`, `capture/should-capture.ts`, `media/center.ts`
- **11 media-addon** (17): `crates/addons/Cargo.toml`, `crates/addons/src/archive.rs`, `crates/addons/src/lib.rs`, `crates/addons/src/manifest.rs`, `crates/addons/src/signature.rs`, `crates/core/examples/ffmpeg_release.rs`, `crates/core/src/manager/addons/install.rs`, `crates/core/src/manager/addons/mod.rs`, `crates/core/src/manager/addons/tests.rs`, `crates/core/src/manager/addons/updates.rs`, `crates/core/tests/manager/addons.rs`, `crates/files/src/whole_file.rs`, `crates/files/src/discard.rs`, `crates/engine/src/target.rs`, `crates/core/src/manager/media.rs`, `crates/desktop/src/main_window/settings/addon.rs`, `crates/protocol/src/lib.rs`
- **12 app-updates** (20): `crates/core/src/manager/app_update.rs`, `crates/desktop/src/app_update.rs`, `crates/shell/src/updates.rs`, `crates/updates/Cargo.toml`, `crates/updates/samples/update.json`, `crates/updates/src/lib.rs`, `crates/updates/src/manifest.rs`, `crates/updates/src/release.rs`, `scripts/release/manifest.mjs`, `scripts/release/manifest.test.mjs`, `scripts/release/updater-key.mjs`, `crates/engine/src/target.rs`, `crates/engine/src/download.rs`, `crates/desktop/src/lifecycle.rs`, `crates/desktop/src/backend.rs`, `crates/desktop/src/main.rs`, `crates/shell/src/appimage.rs`, `crates/desktop/packaging/windows/installer.iss`, `.github/workflows/release.yml`, `scripts/package.mjs`
- **13 program-running** (11): `crates/streams/examples/fake_program.rs`, `crates/streams/src/process/mod.rs`, `crates/streams/src/process/run.rs`, `crates/streams/src/process/tail.rs`, `crates/streams/src/process/tree.rs`, `crates/streams/tests/process.rs`, `crates/media/src/ytdlp/mod.rs`, `crates/media/src/ffmpeg.rs`, `crates/logging/src/redact.rs`, `crates/test-support/src/lib.rs`, `docs/ROADMAP.md`
- **14 stream-manifests** (21): `crates/streams/src/codec.rs`, `crates/streams/src/dash/list.rs`, `crates/streams/src/dash/mod.rs`, `crates/streams/src/dash/sidx.rs`, `crates/streams/src/dash/template.rs`, `crates/streams/src/dash/tests.rs`, `crates/streams/src/error.rs`, `crates/streams/src/extension.rs`, `crates/streams/src/hls.rs`, `crates/streams/src/model.rs`, `crates/streams/src/plan.rs`, `crates/streams/src/select.rs`, `crates/streams/src/source.rs`, `crates/streams/src/track_id.rs`, `crates/streams/tests/manifest_memory.rs`, `crates/streams/src/ffmpeg.rs`, `crates/streams/src/download/fetch.rs`, `download/join.rs`, `crates/media/src/describe.rs`, `crates/engine/src/fetch.rs`, `crates/streams/src/lib.rs`
- **15 stream-download** (19): `crates/streams/Cargo.toml`, `crates/streams/examples/fake_ffmpeg.rs`, `crates/streams/src/crypto.rs`, `crates/streams/src/download/fetch.rs`, `crates/streams/src/download/join.rs`, `crates/streams/src/download/mod.rs`, `crates/streams/src/ffmpeg.rs`, `crates/streams/src/lib.rs`, `crates/streams/src/workdir.rs`, `crates/streams/tests/common/mod.rs`, `crates/streams/tests/dash.rs`, `crates/streams/tests/ffmpeg.rs`, `crates/streams/tests/hls.rs`, `crates/engine/src/fetch.rs`, `crates/streams/src/plan.rs`, `process/run.rs`, `crates/core/src/manager/media_run/mod.rs`, `crates/core/src/manager/downloads.rs`, `crates/media/src/describe.rs`
- **16 media-layer** (23): `crates/media/Cargo.toml`, `crates/media/src/lib.rs`, `crates/media/src/choice.rs`, `crates/media/src/tools.rs`, `crates/media/src/ffmpeg.rs`, `crates/media/src/merge.rs`, `crates/media/src/describe.rs`, `crates/media/src/convert.rs`, `crates/media/src/output/mod.rs`, `crates/media/src/output/codec.rs`, `crates/media/src/output/variants.rs`, `crates/media/src/output/encoders.rs`, `crates/media/src/output/arguments.rs`, `crates/media/src/output/metadata.rs`, `crates/media/src/output/subtitles.rs`, `crates/media/src/ytdlp/cookies.rs`, `crates/media/src/ytdlp/download.rs`, `crates/media/src/ytdlp/info.rs`, `crates/media/src/ytdlp/mod.rs`, `crates/engine/src/naming.rs`, `crates/core/src/manager/media_run/page.rs`, `crates/streams/src/process/run.rs`, `crates/streams/src/codec.rs`
- **17 core-media-runs** (24): `crates/core/src/bin/fake_media_tool.rs`, `crates/core/src/manager/added_pages.rs`, `crates/core/src/manager/media.rs`, `crates/core/src/manager/media_run/extras.rs`, `crates/core/src/manager/media_run/mod.rs`, `crates/core/src/manager/media_run/network.rs`, `crates/core/src/manager/media_run/page.rs`, `crates/core/src/manager/media_run/placement.rs`, `crates/core/src/manager/media_run/save.rs`, `crates/core/src/manager/media_run/tests.rs`, `crates/core/src/manager/media_run/workdir.rs`, `crates/core/tests/manager/media.rs`, `crates/core/tests/manager/media_output.rs`, `crates/core/tests/manager/media_support.rs`, `manager/run.rs`, `manager/file_exists.rs`, `manager/downloads.rs`, `manager/state.rs`, `crates/media/src/ytdlp/download.rs`, `crates/media/src/ytdlp/mod.rs`, `crates/media/src/ytdlp/info.rs`, `crates/streams/src/error.rs`, `crates/desktop/src/download_windows/media_form.rs`, `crates/desktop/src/add_dialog.rs`
- **18 queues-and-scheduling** (18): `crates/core/src/batches.rs`, `crates/core/src/manager/placement.rs`, `crates/core/src/manager/policy.rs`, `crates/core/src/manager/queues.rs`, `crates/core/src/manager/scheduling.rs`, `crates/core/tests/manager/limits.rs`, `crates/core/tests/manager/queues.rs`, `crates/core/tests/manager/rules.rs`, `crates/core/tests/manager/schedule.rs`, `crates/protocol/src/rules.rs`, `crates/queues/Cargo.toml`, `crates/queues/src/lib.rs`, `crates/queues/src/plan.rs`, `crates/queues/src/rules.rs`, `crates/queues/src/schedule.rs`, `crates/queues/src/sites.rs`, `crates/core/src/manager/mod.rs`, `crates/desktop/src/main_window/sidebar.rs`
- **19 settings-and-proxies** (14): `crates/core/src/manager/preferences.rs`, `crates/core/src/manager/proxy.rs`, `crates/core/src/proxy.rs`, `crates/core/tests/manager/proxy.rs`, `crates/core/tests/manager/settings.rs`, `crates/core/tests/manager/settings_file.rs`, `crates/protocol/src/proxy.rs`, `crates/settings/Cargo.toml`, `crates/settings/src/error.rs`, `crates/settings/src/folders.rs`, `crates/settings/src/lib.rs`, `crates/settings/src/proxy.rs`, `crates/settings/src/proxy/tests.rs`, `crates/settings/src/settings_file.rs`
- **20 logging-and-secrets** (14): `crates/logging/Cargo.toml`, `crates/logging/src/fields.rs`, `crates/logging/src/file.rs`, `crates/logging/src/lib.rs`, `crates/logging/src/redact.rs`, `crates/logging/src/redact/named.rs`, `crates/logging/src/redact/scan.rs`, `crates/logging/src/redact/tests.rs`, `crates/logging/src/sink.rs`, `crates/logging/tests/panics.rs`, `crates/protocol/src/secret.rs`, `crates/protocol/src/secrets.rs`, `crates/test-support/data/redaction.json`, `crates/test-support/src/redaction.rs`
- **21 engine-ftp** (14): `crates/engine/src/ftp/control.rs`, `crates/engine/src/ftp/mod.rs`, `crates/engine/src/ftp/reply.rs`, `crates/engine/src/ftp/tests.rs`, `crates/engine/src/socks.rs`, `crates/engine/src/socks/tests.rs`, `crates/engine/tests/ftp.rs`, `crates/engine/tests/ftp_logging.rs`, `crates/engine/tests/ftp_proxy.rs`, `crates/engine/src/transport.rs`, `crates/core/src/naming.rs`, `crates/core/src/manager/run.rs`, `crates/sites/src/local_network.rs`, `crates/test-support/src/ftp.rs`
- **22 app-start-and-quit** (28): `crates/bridge/src/install/browsers.rs`, `crates/bridge/src/install/host.rs`, `crates/bridge/src/install/mod.rs`, `crates/bridge/src/install/plan.rs`, `crates/desktop/Cargo.toml`, `crates/desktop/build.rs`, `crates/desktop/src/backend.rs`, `crates/desktop/src/closing.rs`, `crates/desktop/src/lifecycle.rs`, `crates/desktop/src/main.rs`, `crates/desktop/src/sign_in.rs`, `crates/shell/Cargo.toml`, `crates/shell/src/appimage.rs`, `crates/shell/src/autostart.rs`, `crates/shell/src/bridge.rs`, `crates/shell/src/elevation.rs`, `crates/shell/src/host.rs`, `crates/shell/src/launch.rs`, `crates/shell/src/lib.rs`, `crates/shell/src/message_box.rs`, `crates/shell/src/quit.rs`, `crates/desktop/src/links.rs`, `crates/desktop/src/add_dialog.rs`, `crates/desktop/src/main_window/mod.rs`, `crates/desktop/src/library.rs`, `crates/server/src/lib.rs`, `crates/bridge/src/app.rs`, `crates/core/src/manager/mod.rs`
- **23 interface-commands-and-ui-kit** (24): `crates/desktop/assets/theme.json`, `crates/desktop/src/files.rs`, `crates/desktop/src/library.rs`, `crates/desktop/src/links.rs`, `crates/desktop/src/text.rs`, `crates/desktop/src/theme.rs`, `crates/desktop/src/ui/callout.rs`, `crates/desktop/src/ui/choice.rs`, `crates/desktop/src/ui/dialog.rs`, `crates/desktop/src/ui/field.rs`, `crates/desktop/src/ui/file_tile.rs`, `crates/desktop/src/ui/folder_row.rs`, `crates/desktop/src/ui/icon.rs`, `crates/desktop/src/ui/layout.rs`, `crates/desktop/src/ui/mark.rs`, `crates/desktop/src/ui/mod.rs`, `crates/desktop/src/ui/queue_tile.rs`, `crates/desktop/src/ui/select_circle.rs`, `crates/desktop/src/ui/small_title_bar.rs`, `crates/desktop/src/ui/speed_graph.rs`, `crates/desktop/src/ui/switch_row.rs`, `crates/desktop/src/ui/tokens.rs`, `crates/shell/src/api.rs`, `crates/shell/src/appearance.rs`
- **24 download-windows** (14): `crates/desktop/src/download_windows/finished.rs`, `crates/desktop/src/download_windows/media_form.rs`, `crates/desktop/src/download_windows/media_output.rs`, `crates/desktop/src/download_windows/mod.rs`, `crates/desktop/src/download_windows/run_record.rs`, `crates/desktop/src/download_windows/view.rs`, `crates/shell/src/download_windows.rs`, `crates/shell/src/finished.rs`, `crates/desktop/src/library.rs`, `main_window/applies.rs`, `crates/core/src/manager/file_exists.rs`, `addons/mod.rs`, `crates/core/src/naming.rs`, `crates/shell/src/api.rs`
- **25 main-window** (27): `crates/desktop/src/add_dialog.rs`, `crates/desktop/src/main_window/applies.rs`, `crates/desktop/src/main_window/command_bar.rs`, `crates/desktop/src/main_window/confirm.rs`, `crates/desktop/src/main_window/details.rs`, `crates/desktop/src/main_window/drag.rs`, `crates/desktop/src/main_window/empty.rs`, `crates/desktop/src/main_window/fixes.rs`, `crates/desktop/src/main_window/keymap.rs`, `crates/desktop/src/main_window/list.rs`, `crates/desktop/src/main_window/mod.rs`, `crates/desktop/src/main_window/notices_bar.rs`, `crates/desktop/src/main_window/order.rs`, `crates/desktop/src/main_window/palette.rs`, `crates/desktop/src/main_window/rail.rs`, `crates/desktop/src/main_window/refresh_dialog.rs`, `crates/desktop/src/main_window/row_menu.rs`, `crates/desktop/src/main_window/selection.rs`, `crates/desktop/src/main_window/sidebar.rs`, `crates/desktop/src/main_window/sort.rs`, `crates/desktop/src/main_window/title_bar.rs`, `crates/core/src/manager/downloads.rs`, `crates/core/src/manager/preferences.rs`, `crates/desktop/src/library.rs`, `crates/desktop/src/main_window/queue_header.rs`, `crates/desktop/src/ui/dialog.rs`, `crates/desktop/src/text.rs`
- **26 notifications-and-tray** (11): `crates/desktop/src/notices.rs`, `crates/desktop/src/tray.rs`, `crates/shell/src/activity.rs`, `crates/shell/src/browser_notices.rs`, `crates/shell/src/notifications.rs`, `crates/shell/src/system_notifications.rs`, `crates/shell/src/teller.rs`, `crates/shell/src/tray.rs`, `crates/shell/src/tray_proxy.rs`, `crates/shell/src/tray_queues.rs`, `crates/shell/src/tray_speed.rs`
- **27 settings-and-queue-forms** (18): `crates/desktop/src/main_window/backup_dialog.rs`, `crates/desktop/src/main_window/link_trial.rs`, `crates/desktop/src/main_window/proxy_dialog.rs`, `crates/desktop/src/main_window/queue_delete.rs`, `crates/desktop/src/main_window/queue_dialog.rs`, `crates/desktop/src/main_window/queue_header.rs`, `crates/desktop/src/main_window/queue_proxy.rs`, `crates/desktop/src/main_window/queue_rules.rs`, `crates/desktop/src/main_window/settings/about.rs`, `crates/desktop/src/main_window/settings/addon.rs`, `crates/desktop/src/main_window/settings/browser.rs`, `crates/desktop/src/main_window/settings/files.rs`, `crates/desktop/src/main_window/settings/general.rs`, `crates/desktop/src/main_window/settings/media.rs`, `crates/desktop/src/main_window/settings/mod.rs`, `crates/desktop/src/main_window/settings/network.rs`, `crates/desktop/src/main_window/settings/notifications.rs`, `crates/desktop/src/main_window/settings/speed.rs`
- **28 failure-messages** (4): `crates/core/src/error.rs`, `crates/core/tests/manager/ftp.rs`, `crates/messages/Cargo.toml`, `crates/messages/src/lib.rs`
- **29 packaging-and-release** (21): `.github/workflows/release.yml`, `config/templates/SECURITY.md`, `config/templates/prerm.sh`, `crates/desktop/PACKAGING.md`, `crates/desktop/assets/icons/icon.svg`, `crates/desktop/packaging/windows/installer.iss`, `scripts/ci/linux-package-check.sh`, `scripts/ci/linux-packages.sh`, `scripts/ci/linux-packages.txt`, `scripts/gate/notices.mjs`, `scripts/notices/cargo.mjs`, `scripts/notices/licences.mjs`, `scripts/notices/notices.test.mjs`, `scripts/notices/npm.mjs`, `scripts/notices/render.mjs`, `scripts/package.mjs`, `scripts/release/manifest.mjs`, `scripts/gate/process.mjs`, `crates/shell/src/appimage.rs`, `crates/shell/src/host.rs`, `gpui-pre-linux-0.3.10/src/linux/platform.rs`
- **30 protocol-types** (6): `crates/protocol/src/app.rs`, `crates/protocol/src/desktop.rs`, `crates/protocol/src/exported.rs`, `crates/protocol/src/languages.rs`, `crates/protocol/src/media_start.rs`, `crates/media/src/choice.rs`
- **31 test-servers** (17): `crates/test-support/Cargo.toml`, `crates/test-support/src/bridge.rs`, `crates/test-support/src/downloads.rs`, `crates/test-support/src/files.rs`, `crates/test-support/src/ftp.rs`, `crates/test-support/src/http.rs`, `crates/test-support/src/http3.rs`, `crates/test-support/src/lib.rs`, `crates/test-support/src/logs.rs`, `crates/test-support/src/proxy.rs`, `crates/test-support/src/resume.rs`, `crates/test-support/src/silent.rs`, `crates/test-support/src/tls.rs`, `crates/test-support/tests/logs.rs`, `crates/engine/src/segment.rs`, `crates/engine/src/http3.rs`, `crates/engine/src/ftp/mod.rs`
- **32 cli** (2): `crates/cli/Cargo.toml`, `crates/cli/src/main.rs`
- **33 repo-checks** (43): `.cargo/config.toml`, `.githooks/commit-msg`, `.github/workflows/audit.yml`, `.github/workflows/ci.yml`, `.github/workflows/outdated.yml`, `Cargo.toml`, `clippy.toml`, `config/product.toml`, `crates/product/Cargo.toml`, `deny.toml`, `rust-toolchain.toml`, `scripts/gate.mjs`, `scripts/gate/browser.mjs`, `scripts/gate/comments.mjs`, `scripts/gate/commits.mjs`, `scripts/gate/exceptions.mjs`, `scripts/gate/imports.test.mjs`, `scripts/gate/javascript.mjs`, `scripts/gate/javascript.test.mjs`, `scripts/gate/js-policy.mjs`, `scripts/gate/linux.mjs`, `scripts/gate/process.mjs`, `scripts/gate/product.mjs`, `scripts/gate/product.test.mjs`, `scripts/gate/rust-attributes.mjs`, `scripts/gate/rust.mjs`, `scripts/gate/rust.test.mjs`, `scripts/gate/suffix-list.mjs`, `scripts/gate/suffix-list.test.mjs`, `scripts/gate/test-timing.mjs`, `scripts/gate/test-timing.test.mjs`, `scripts/gate/text.mjs`, `scripts/gate/text.test.mjs`, `scripts/install-hooks.mjs`, `scripts/linux-test.sh`, `scripts/outdated.mjs`, `scripts/product/generate.mjs`, `scripts/product/manifests.mjs`, `scripts/product/manifests.test.mjs`, `scripts/product/outputs.mjs`, `scripts/product/read.mjs`, `scripts/product/read.test.mjs`, `scripts/product/sources.mjs`

## Disproved findings

These have no item. Each is listed once.

- `12-5` ("Declining the administrator prompt reads as could not be started (error 5)", `chunks/12.md`; verdict in `verdicts/B05.md`). It assumes the shell shows the prompt and returns error 5. The built setup is `asInvoker`, so `ShellExecuteW` starts it with no prompt and returns above 32, and a refusal never reaches `InstallError::Start`. The real failure is item 72.
- `B01-2` ("Moving a download before the server answers hides the duplicate check the same way", `verdicts/B01.md`; verdict in `verdicts/new-findings.md`). The ` (1)` is the documented contract of `set_download_folder`, a settled or finished download moved into the same folder gets the same name, and D96 governs a file already at the path when the server answers, not a move the person asked for. If item 11 wants moves to follow the setting too, that is a new decision.
