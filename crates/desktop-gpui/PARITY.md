# What the GPUI app still needs

What the Tauri app can do that the GPUI app must also do before Tauri is removed (D95, D99). Ticked items are done. Each item is a capability, not a design: how a screen looks and is laid out is decided anew for the GPUI app (D99, point 4). This file goes when the last item is ticked.

## Main window

- [x] Opens the manager in the background; the window never waits on the database.
- [x] Follows the manager's events; reloads after missing some.
- [x] Light and dark colours with the Theme and Accent settings, following the system's scheme.
- [x] The system accent colour on Windows (`appearance.rs`).
- [x] The manager could not open: "{app} cannot reach its download engine", with Try again.
- [x] Another copy already runs: say so and close (`lib.rs` `say_already_running`).
- [x] Search, focused with Ctrl+K: names and links, a search unfolds groups, "N downloads matching …", the view in the placeholder.
- [x] Search: Escape clears, Down moves into the list; the settings screen searches settings in its own navigation.
- [x] The waiting questions, with their count (`show_question_windows`).
- [x] Navigation toggle; collapsed rail and narrow-window drawer (D91).
- [x] Navigation: All downloads, the six status views with counts, the queues with count or speed.
- [x] Queues: the first three, "N more queues" / "Fewer queues"; the Rules badge, status text, dimmed when Rules are off.
- [x] Queues: "+ New queue".
- [x] Queues: the proxy icon while a saved profile is in use (D73.13).
- [x] Queue "…" menu: Start now (outside its schedule), Pause or Resume the queue, Move up, Move down.
- [x] Queue menu: Edit queue.
- [x] Drag queues to reorder them, the default queue first (D72.9).
- [x] Settings (Ctrl+,), at the foot of the navigation, where the settings screen offers Downloads.
- [x] Find any action (Ctrl+Shift+P) and Keyboard shortcuts (F1), reachable from the main window.
- [x] Each group heading with its count; Pause all on what runs (or waits), Resume all on what is paused.
- [x] A link pasted into the search field offers to download it (Enter), and finds it if it already is a download.
- [x] List header in a queue view: description, schedule and limits, Start now, Pause or Resume queue, Edit queue; "about X left"; "matching '…'".
- [x] New download button (Ctrl+N).
- [x] The New download button hidden on first run, where the empty state offers it.
- [x] Storage failure banner with Try again and Dismiss, and a notification when no main window shows it; startup notices; toasts.
- [x] Groups in order, Finished and Cancelled folded at first, headings fold; no headings in a status view.
- [x] A queue view lists its line ungrouped in run order, Finished and Cancelled grouped below; the # column.
- [x] Sorting by queue order, name, progress, size, speed or time left, either way. The GPUI list has no columns to resize or fold: a row shows what fits its state (D99.4).
- [x] Row: name, the line under it, progress with its caption, size, speed, time left.
- [x] Row: Open (a program asks first, D43.1) and Show in folder, on hover and on the selected row.
- [x] Row: category icon, the "…" menu, the fix button in the progress cell (`row-actions.ts`).
- [x] Hover Pause, Resume, Try again, Start over.
- [x] Selection: Ctrl+click, Shift+click ranges; selection bar with Pause, Resume, Try again, Move to queue, Remove, Clear; the list keys that move and extend it, Ctrl+A, Space, Enter, Alt+Enter, Delete.
- [x] Empty states with their buttons: first run, no search match, empty queue, an empty view.
- [x] First run: "See everything {app} can do" (opens the palette).
- [x] Details dialog (D47), opened by clicking a row: status, the problem with its fix and Try again, speed and progress, facts, Saved to with Change, the link with Copy, activity, Open, Pause, Resume, Start over, Show in folder, More (Copy link, Cancel download), Remove; it follows the download and closes once it is removed.
- [x] Details: the speed graph, the queue as a menu, "Stuck? The link may have expired." with Get a new link, and the fixes that open other screens (Get a new link, Proxy settings, Change queue).
- [x] Open in a separate window, from the row menu and the palette.
- [x] Context menu on rows; Menu key and Shift+F10.
- [x] Keyboard: every shortcut of `lib/keymap.ts`, list keys; F6 moves between the search and the list (the navigation takes no keyboard focus yet).
- [x] Drag rows to reorder in a queue and onto a queue to move them; drop an Internet shortcut file to add its link. GPUI takes only files from other programs, so a link dragged straight from a browser cannot be dropped.
- [x] Paste a link anywhere (Ctrl+V) to open the Add dialog.
- [x] Command palette: the selection's commands, general commands, every view and queue, and the settings pages.
- [x] The total speed readout, with "Starting" and "Finishing videos", and its tooltip (D23).

## Dialogs

- [x] Add: links, file name, Save to, queue, Add paused, Download (Ctrl+Enter), video pages read for their qualities (D51), failed links kept with why.
- [x] Add: the Automatic hint naming the queue each link goes to (`try_link`), focus on Download when opened with links, links from a second launch joining an open dialog.
- [x] Remove, with "Also delete the files".
- [x] Cancel.
- [x] Queue: name, description, own folder (D94), limits, schedule, notifications, Delete queue.
- [x] Queue: rules with Test a link.
- [x] Queue: proxy, with New proxy, and the hosts that skip it.
- [x] Get a new link.
- [x] Export and import settings, with the preview.
- [x] Keyboard shortcuts.
- [x] Remove the add-on.
- [x] Proxy server, with Test connection.
- [x] Delete a proxy server, with what to switch to.

## Settings

- [x] General: theme, accent, start at sign-in, close to tray, updates, suffix list, log folder.
- [x] General: backup (with the Export and import dialog).
- [x] Notifications.
- [x] Saving files: folder, if the file exists, folders by file type.
- [x] Browser.
- [x] Media: asking, resolution, formats, audio quality, subtitles, chapters, video sites.
- [x] Media: the add-on card.
- [x] Media: subtitle languages.
- [x] Speed and queues, with Test a link.
- [x] Network: the global proxy, proxy servers, hosts that skip the proxy, FTP past proxies, local network sites.
- [x] Settings search.
- [x] Jump to a settings page from the palette.

## Download windows (D23, D43)

- [x] One window per question or download, at most five at once, cascaded, height fitted to the content. GPUI has no always-on-top that can be turned off, so a window comes to the front when it opens and is a normal window after.
- [x] File question (D72.8, D94.6), reading a video, add-on needed, media quality (D50), file already exists (D96), progress, finished.
- [x] Reopened at start for open questions; `capture_repeated` brings one back (D89).
- [x] Finished window ("Open a summary window", D52).

## Shell

- [x] Tray: Open, Pause all or Resume all, Proxy submenu, Quit; tooltip; no tray means closing quits (D104).
- [x] Close to tray; quit stops downloads within a time limit.
- [x] A launch while quitting starts the app again once it has exited (D105).
- [x] Single instance: a second launch focuses the running app and passes its links (D103).
- [x] `--background`; links on the command line.
- [x] Browser bridge: serve it, register it on start (not in debug builds), the AppImage copy.
- [x] System notifications with buttons (D52), browser notices with the system fallback (D65), a notice opening its download (D98).
- [x] Open a file, with the warning for programs; show in folder; folder and file pickers.
- [x] Start at sign-in.
- [x] Startup failure dialogs.
- [x] Installers for Windows and Linux (D106); the updater comes with release item 7.
