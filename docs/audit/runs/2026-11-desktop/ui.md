# UI review: desktop run

Reviewed on the running dev app (`target/ui/wirecatch-app.exe`) on Windows, with a scratch data folder. Screenshots are in `%TEMP%\wirecatch-audit\2026-11-desktop\screens\`, named `<state>-<theme>.png`.

Marks: `[ ]` not yet analysed, `[x]` captured and analysed, `[-]` not reached (with why).

## Windows and states

### Main window

Both themes unless a line says otherwise. Keyboard shortcuts with Ctrl or Alt cannot be posted, so those paths were reached by their buttons.

- [x] M1 Empty list, first start, default size
- [x] M2 Full list, All downloads, day timeline
- [x] M3 Running row (with speed, progress): dark only; light shows the same row layout in its paused and waiting states
- [x] M4 Paused row
- [x] M5 Failed row (needs attention, with fix)
- [x] M6 Completed row
- [x] M7 Waiting row (schedule wait)
- [x] M8 Status pages in the sidebar, empty and full
- [x] M9 Queue page with its header, full and empty
- [x] M10 Selection bar with several rows picked
- [x] M11 Row right-click menu (running, failed, finished, cancelled)
- [x] M12 Details dialog (running, paused, failed, finished, cancelled, waiting)
- [x] M13 Remove confirmation; Cancel confirmation dark only
- [x] M14 Search with matches (dark) and with none (both)
- [x] M15 Sort and View menus: dark only
- [x] M16 Narrow window: rail both; drawer dark only
- [x] M17 Command palette, opened by Browse all commands
- [x] M18 Damaged list notice: dark only, because repairing a damaged list resets the theme to the system's, which is dark here
- [-] M19 Storage failure banner: not reached, it needs the database to fail writing while the app runs

### Add dialog

- [x] A1 Empty
- [x] A2 With links, for one queue: dark only
- [x] A3 Bad link error: dark only
- [x] A4 Several links: dark only

### Download windows (browser handover)

- [x] D1 Ask step (confirm), and its queue menu (dark)
- [x] D2 Progress step, running
- [x] D3 Progress step, paused
- [x] D4 Progress step, failed; cancelled state in light
- [x] D5 Finished step
- [x] D6 Already-downloaded warning and file-exists question; the state after Keep both dark only
- [x] D7 Media: reading the page
- [x] D8 Media: quality choice
- [x] D9 Media: add-on offer (both), installing and failed reading (dark)

### Failure notifications

- [-] N1 System notification for a failed download: not reached. The system draws it outside the app; a scan for the shell's notification window during failures found none, so neither its look nor that it appeared is confirmed.
- [-] N2 System notification for a finished download: not reached, same reason.
- [x] N3 In-app messages ("Link copied.", "2 downloads added."): dark only

### Settings pages

- [x] S1 General
- [x] S2 Saving files
- [x] S3 Notifications
- [x] S4 Browser
- [x] S5 Media, add-on not installed (dark) and installed (light)
- [x] S6 Speed and queues
- [x] S7 Network, without a proxy and with one
- [x] S8 Keyboard shortcuts
- [x] S9 About

### Queue and proxy dialogs

- [x] Q1 New queue dialog: all sections dark; top and empty form light
- [x] Q2 Queue editor with rules, schedule, proxy: expanded sections dark; top light
- [x] Q3 Test a link
- [x] Q4 Delete queue dialog, with and without downloads
- [x] Q5 Proxy dialog, errors and test (dark), empty form (both), list (dark)
- [-] Q6 Backup dialog: not reached. Export and Import start at the system's file picker, which posted messages cannot drive safely, and it would open on the person's screen.
- [-] Q7 Refresh link dialog: not reached. The click on Get a new link landed outside the button twice during this run, and it was not retried before the app was stopped.

### Other

- [x] O1 Tray menu, opened with the tray's own message; submenus not opened
- [-] O2 Update banner: untested. It needs a seed of `Stage::Available` in a temporary worktree and a rebuild, which is not cheap here.
- [x] O3 Closing the window: no prompt exists; the window closes to the tray and a second launch brings it back (dark only)

## Findings

### U-1 [P3] The settings page left behind keeps the accent colour

- Where: settings navigation in `crates/desktop/src/main_window/settings/mod.rs`, drawn by the `gpui-kit` settings component
- Category: design
- Failure: go from Saving files to Notifications to Browser in Settings. The previous item stays in the accent colour (teal label and icon, without the selected background), next to the really selected one, until another page is picked. Two items read as selected; `S3-dark.png` (Saving files teal), `S4-dark.png` (Notifications teal), `S5-dark.png` (Browser teal), `S6-dark.png` (Media teal).
- Evidence: the mark stays after a new capture and after scrolling the page, and with the pointer elsewhere, so it is neither a running transition nor hover.
- Confidence: likely (driven by posted mouse messages, not a physical mouse)

### U-2 [P3] Every window has an empty native title

- Where: `crates/desktop/src/main.rs`, `window_options`
- Category: design
- Failure: `GetWindowText` on the main window returns an empty string. Alt+Tab, the taskbar preview and screen readers have no name to give for it.
- Evidence: `w.ps1 list` prints `[Zed::Window] ''` for the window.
- Confidence: certain

### U-3 [P3] Empty pages offer Pause all with nothing to pause

- Where: `crates/desktop/src/main_window/list.rs`, `group_command`; `crates/desktop/src/main_window/command_bar.rs`, `page_command`
- Category: design
- Failure: open Downloading or Up next with no downloads in that state. The bar still offers "Pause all" beside "Nothing downloading" and "Nothing up next" (`M8-downloading-empty-dark.png`, `M8-upnext-empty-dark.png`).
- Evidence: `group_command` returns the button without checking that the page has rows.
- Confidence: certain

### U-4 [P3] Ticking the first row pushes the list down under the pointer

- Where: `crates/desktop/src/main_window/selection.rs`, the selection bar
- Category: design
- Failure: tick one row's circle. The "1 selected" bar is inserted above the list and every row moves about 50 pixels down. A second tick aimed at the circle of the next row, as seen before the first click, lands in the row body, so the row's details open instead of a second tick. Reproduced here: ticking at (348, 209) then (348, 294) opened the details dialog of the first row.
- Evidence: `M10-selection-dark.png` against `M2-dark.png` shows the rows shifted; the bar has no space reserved.
- Confidence: certain

### U-5 [P3] On a queue page the row numbers start at 2 and number failed rows

- Where: `crates/desktop/src/main_window/list.rs`, `time_cell`, queue view
- Category: design
- Failure: the Main queue page with one paused and three failed downloads shows "paused", 2, 3, 4 in the time column (`M9-queue-dark.png`). No row shows 1, and failed downloads, which wait for nothing, read as second, third and fourth in line.
- Evidence: the same rows show their times on All downloads.
- Confidence: certain

### U-6 [P2] A download waiting on the file-exists question shows as Paused, with a name that was never chosen and "Paused at Size unknown"

- Where: `crates/desktop/src/download_windows/view.rs`, progress state; `crates/desktop/src/main_window/list.rs`, the paused row's text
- Category: wrong implementation
- Failure: hand over a link whose file is already in the folder, and press Download. A question window asks Replace or Keep both. Meanwhile the first window turns into a progress window saying Paused, Downloaded 0 B, with a Resume button, for `wcaudit-hfin (1).zip`; the row in the list reads "Paused at Size unknown". The person paused nothing, the name "(1)" is not yet decided, and the question window still names `wcaudit-hfin.zip` (`D6-dark.png`, `D6q-dark.png`).
- Evidence: seen in the running app with the local test server; the row and window both changed only after Keep both was pressed. Pressing Cancel in the question instead leaves a Cancelled row and window for `wcaudit-hfin (2).zip`, saying "The partial file was removed", though nothing was downloaded (`D6q-light.png`). A video saved with Download later also reads "Paused at Size unknown" (`M4-light.png`).
- Confidence: certain

### U-7 [P3] After answering the file-exists question two windows show the same download

- Where: `crates/desktop/src/download_windows/mod.rs`, `wirecatch_shell::download_windows::Registry`
- Category: bug
- Failure: in the flow of U-6 press Keep both. The question window turns into the finished view of `wcaudit-hfin (1).zip`, and the earlier progress window shows the same finished download too (`D6-after-dark.png`). Two identical windows must be closed.
- Evidence: `w.ps1 list` shows both windows after the answer, with the same name and finish time.
- Confidence: certain

### U-8 [P3] The link's hover popover covers the row's right-click menu

- Where: `crates/desktop/src/main_window/list.rs`, the link line's hover popover; `row_menu.rs`
- Category: design
- Failure: right-click a row on its link text. About a second later the popover with the whole link appears on top of the open menu and hides its first items, here Get a new link and Try again (`M11-link-light.png`). It also covers the row's own action buttons when the link is long (`M3-dark.png`).
- Evidence: the menu's top rows are hidden behind the popover in the capture; reproduced in both themes.
- Confidence: certain

### U-9 [P3] Raw yt-dlp output in a failed video message

- Where: `crates/media`, the reading error; shown by `crates/desktop/src/download_windows/view.rs`
- Category: wrong implementation
- Failure: a video page that answers 404 shows "The media add-on could not get the video from this page ([generic] watch?v=audit: Unable to download webpage: HTTP Error 404: Not Found (caused by <HTTPError 404: Not Found>))." (`D9-later-dark.png`). D3 says a person is told plainly what failed.
- Evidence: the window's text in the capture.
- Confidence: certain

### U-10 [P3] The cancelled download's action has two names

- Where: `crates/desktop/src/main_window/row_menu.rs`, `details.rs`
- Category: design
- Failure: the row menu of a cancelled download says Try again, the details dialog says Start over for the same action (`M11-cancelled-dark.png`, `M12-cancelled-dark.png`).
- Evidence: captures.
- Confidence: certain

### U-11 [P3] A queue using a proxy with no name reads "via" and nothing after it

- Where: `crates/desktop/src/main_window/sidebar.rs`, `summary`, and where its `proxy` argument is read
- Category: bug
- Failure: save a proxy without a name (the dialog says "Optional. Without one, the proxy's address is shown") and choose it for a queue. The queue's sidebar line reads "Starts 1:00 AM · via" (`M7-waiting-dark.png`, `M-twoqueues-dark.png`). Its dropdown and the Network page show `127.0.0.1:8080` for the same proxy.
- Evidence: `summary` formats `via {proxy}` from a name that is empty here.
- Confidence: certain

### U-12 [P3] The queue page's facts line is cut off at the default window size, with no way to read the end

- Where: `crates/desktop/src/main_window/queue_header.rs`
- Category: design
- Failure: at the default 1600 pixel width a queue with a schedule and limits shows "1:00 AM to 7:00 AM · 4 at once · Up to 2.00 MB/s · Other limits are global · Nothi..." (`M7-waiting-dark.png`). The cut text is what the queue is doing now.
- Evidence: capture. The buttons keep their width and the line takes what is left; the current state comes last, so it is the part lost.
- Confidence: certain

### U-13 [P3] New queue does not put the cursor in its Name field

- Where: `crates/desktop/src/main_window/queue_dialog.rs`
- Category: design
- Failure: open New queue and type at once: nothing appears (`Q1a-dark.png`), while New download focuses its Links field (`A1-dark.png`).
- Evidence: typed characters were posted to the window right after it opened, and the field stayed empty.
- Confidence: certain

### U-14 [P3] Cancel in the download window deletes the partial file without asking

- Where: `crates/desktop/src/download_windows/view.rs`, the progress state's Cancel
- Category: design
- Failure: press Cancel in a progress window, next to Hide. The download stops and the partial file is deleted at once (`D3-light.png`, then the cancelled state). The same action from the list first asks "Cancel this download?" (`M-cancel-dark.png`). A slip between Hide and Cancel loses the progress of a long download.
- Evidence: both paths driven in the running app.
- Confidence: certain

### U-15 [P3] The damaged-list notice squeezes its text and breaks the path

- Where: `crates/desktop/src/main_window/notices_bar.rs`, `tell_startup_notices`
- Category: design
- Failure: start on a damaged list. The notice has a narrow text column because its button takes the right third, so the message runs to 13 lines and the path breaks mid-name (`wirecatch.db.damaged-179` / `3765453376`). It stays until closed, over the sort and view buttons and the New download button, and its close button shows only on hover (`M18-damaged-dark.png`).
- Evidence: capture; the notice was unchanged after 10 seconds.
- Confidence: certain

## Suggestions

### U-S11 The already-downloaded warning and the file-exists question ask twice

- Today: the ask step warns "Already downloaded today at 9:58 AM", and after pressing Download a second window asks Replace or Keep both (`D6a-dark.png`, `D6q-dark.png`).
- Better: offer Replace and Keep both on the ask step when the file is in the folder.
- Value: one decision in one window.

### U-S12 Hold the toast clear of the toolbar

- Today: "Link copied." and "2 downloads added." appear over the search field's end and the sort, view and New download buttons (`M7-waiting-dark.png`).
- Better: place them at the bottom right, away from controls.
- Value: the toolbar stays usable for the seconds it shows.

### U-S9 Queue names in Settings sentences should stand out

- Today: "1 queue uses its own value: Audit Queue." and "Queue: Main. No queue's rules match it." show names as plain text (`Q3-dark.png`). A standing design request asks for a badge or colour.
- Better: draw these names with the same queue badge the download windows use.
- Value: a made-up queue name is not misread as words of the sentence.

### U-S10 Follow the Time format in the schedule editor

- Today: with Time format at 12-hour, the schedule fields read 01:00 and 07:00 while the same queue's header reads 1:00 AM to 7:00 AM (`Q2e-dark.png`).
- Better: a 12-hour field with AM and PM when that is the chosen format.
- Value: the time typed is the time the app then says.

### U-S6 The failed download window should offer the same fix as the list

- Today: for an error 404 the list row offers Get a new link, and the download window offers only Try again, though its text says to refresh the link (`D4-dark.png`).
- Better: the window shows the fix that `fixes::offered` picks, as its primary button.
- Value: the person is not sent round a retry that cannot work.

### U-S7 Say the add-on's size in the offer

- Today: Settings says Install (131 MB); the question window says only "Install and download" (`D9-dark.png`).
- Better: "Install and download (131 MB)".
- Value: no surprise on a metered connection.

### U-S8 Activity should record what happened

- Today: details Activity lists Added and Finished only. A failed, cancelled or paused download's activity stops at Added (`M12-failed-dark.png`, `M12-cancelled-dark.png`).
- Better: add failure, cancel, pause and retry lines with their times, as Free Download Manager's log does.
- Value: the person can see when and why a download stopped.

### U-S4 Do not move rows while the pointer is over the list

- Today: a failing download moves to Needs attention at the top, and the rows below shift by a row's height at once. During this review a click meant for an open menu's Pause landed on a row's link after the list had moved, and copied the link.
- Better: hold the order while the pointer is over the list, as `sort::Frozen` does for sorting, and apply the move when it leaves.
- Value: fewer clicks on the wrong row in a busy list.

### U-S5 Details: label the footer icons and drop the second Try again

- Today: the failed download's details offer Try again in the red callout and again in the footer; the footer's other actions are two unlabelled icons, and the "Stuck? The link may have expired." line shows on a download the person paused on purpose (`M12-failed-dark.png`, `M-rowclick-paused-dark.png`).
- Better: one Try again; text labels for Show in folder and Remove; the Stuck hint only for a download that is running without progress.
- Value: the dialog says each thing once, and a healthy paused download does not suggest a problem.

### U-S1 Say where a settings folder really is

- Today: Saving files shows the download folder as `Downloads` in a field that reads like a placeholder.
- Better: show the full path (`C:\Users\...\Downloads`), as Internet Download Manager and qBittorrent do.
- Value: a person sees where files go without opening a folder picker.

### U-S2 Reword the subtitle format help

- Today: "Original keeps the site's file as it is, which may not, and audio files hold none." (Settings, Media, If the format can't hold subtitles)
- Better: "MP4, MKV and WebM hold subtitles. Original may not, because it keeps the site's file as it is. Audio files hold none."
- Value: the sentence no longer has to be read twice.

### U-S3 Offer the logs where the app asks for them

- Today: About, Report a problem says "Describe what happened, with the version above and your logs", and nothing on the page says where the logs are.
- Better: an Open log folder button in that row.
- Value: a report arrives with logs, without the person having to find `%LOCALAPPDATA%`.
