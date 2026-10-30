# What the GPUI app still needs

Every screen, state and behaviour of the Tauri app that the GPUI app must have before Tauri is removed (D95, D99). Ticked items are done. The Svelte sources named are the behaviour to match; the words come from them unless a decision changes them. This file goes when the last item is ticked.

## Main window

- [x] Opens the manager in the background; the window never waits on the database.
- [x] Follows the manager's events; reloads after missing some.
- [x] Light and dark colours with the Theme and Accent settings, following the system's scheme.
- [ ] The system accent colour on Windows (`appearance.rs`).
- [x] The manager could not open: "{app} cannot reach its download engine", with Try again (`ui/DownloadsView.svelte`).
- [ ] Another copy already runs: say so and close (`lib.rs` `say_already_running`).
- [x] Title bar search (Ctrl+K): names and links, a search unfolds groups, "N downloads matching …", the view in the placeholder.
- [ ] Search: Escape clears, Down moves into the list; "Search settings" on the settings screen.
- [ ] Title bar: the waiting-questions bell with its count (`show_question_windows`).
- [ ] Navigation toggle; collapsed rail and narrow-window drawer (D91).
- [x] Navigation: All downloads, the six status views with counts, the queues with count or speed.
- [ ] Queues: the first three, "N more queues" / "Fewer queues"; Rules badge, proxy icon, status text, dimmed when Rules are off.
- [ ] Queue "…" menu: Start now (outside its schedule), Pause or Resume the queue, Edit queue, Move up, Move down.
- [ ] Drag queues to reorder them, the default queue first (D72.9).
- [ ] Navigation footer: Settings (Ctrl+,), Find any action (Ctrl+Shift+P), Keyboard shortcuts (F1).
- [x] List header: the view's title and summary; Pause all or Resume all.
- [ ] List header in a queue view: description, schedule and limits, Start now, Pause or Resume queue, Edit queue; "about X left"; "matching '…'".
- [x] New download button (Ctrl+N).
- [ ] The New download button hidden on first run, where the empty state offers it.
- [ ] Storage failure banner with Try again and Dismiss; startup notices; toasts.
- [x] Groups in order, Finished and Cancelled folded at first, headings fold; no headings in a status view.
- [ ] A queue view lists its line ungrouped in run order, Finished and Cancelled grouped below; the # column.
- [ ] Column sorting (name, progress, size, speed, left; queue order), resizing with saved widths, narrow-window folding.
- [x] Row: name, the line under it, progress with its caption, size, speed, time left.
- [x] Row: Open (a program asks first, D43.1) and Show in folder, on hover and on the selected row.
- [ ] Row: category icon, the "…" menu, the fix button in the progress cell (`row-actions.ts`).
- [x] Hover Pause, Resume, Try again, Start over.
- [ ] Selection: Ctrl+click, Shift+click ranges; selection bar with Pause, Resume, Try again, Move to queue, Remove, Clear.
- [x] Empty states with their buttons (`ListEmpty.svelte`): first run, no search match, empty queue, an empty view.
- [ ] First run: "See everything {app} can do" (opens the palette).
- [x] Details dialog (`DetailsDialog.svelte`, D47), opened by clicking a row: status, the problem with its fix and Try again, speed and progress, facts, Saved to with Change, the link with Copy, activity, Open, Pause, Resume, Start over, Show in folder, More (Copy link, Cancel download), Remove; it follows the download and closes once it is removed.
- [ ] Details: the speed graph, the queue as a select, "Stuck? The link may have expired." with Get a new link, Open in a separate window, and the fixes that open other screens (Get a new link, Proxy settings, Change queue).
- [ ] Context menu on rows; Menu key and Shift+F10.
- [ ] Keyboard: every shortcut of `lib/keymap.ts`, F6 regions, list keys.
- [ ] Drag a link in to add it; drag rows to reorder in a queue and onto a queue to move them.
- [x] Paste a link anywhere (Ctrl+V) to open the Add dialog.
- [ ] Command palette with every command of `lib/commands`.
- [x] The title bar's speed readout, with "Starting" and "Finishing videos", and its tooltip (D23).

## Dialogs

- [x] Add: links, file name, Save to, queue, Add paused, Download (Ctrl+Enter), video pages read for their qualities (D51), failed links kept with why.
- [ ] Add: the Automatic hint naming the queue each link goes to (`try_link`), focus on Download when opened with links, links from a second launch joining an open dialog.
- [x] Remove, with "Also delete the files".
- [x] Cancel.
- [ ] Queue: name, description, own folder (D94), limits, rules with Test a link, proxy, schedule, notifications, Delete queue.
- [ ] Get a new link.
- [ ] Export and import settings, with the preview.
- [ ] Keyboard shortcuts.
- [ ] Remove the add-on.
- [ ] Proxy server, with Test connection.
- [ ] Delete a proxy server, with what to switch to.

## Settings

- [ ] General: theme, accent, start at sign-in, close to tray, updates, suffix list, backup, log folder.
- [ ] Notifications.
- [ ] Saving files: folder, if the file exists, folders by file type.
- [ ] Browser.
- [ ] Media, with the add-on card.
- [ ] Speed and queues, with Test a link.
- [ ] Network: proxies.
- [ ] Settings search; jump to a setting from the palette.

## Download windows (D23, D43)

- [ ] One window per question or download, at most five at once, cascaded, on top until they lose focus, height fitted to the content.
- [ ] File question (D72.8, D94.6), reading a video, add-on needed, media quality (D50), file already exists (D96), progress, finished.
- [ ] Reopened at start for open questions; `capture_repeated` brings one back (D89).
- [ ] Finished window ("Open a summary window", D52).

## Shell

- [ ] Tray: Open, Pause all or Resume all, Proxy submenu, Quit; tooltip; no tray means closing quits.
- [ ] Close to tray; quit stops downloads within a time limit.
- [ ] Single instance: a second launch focuses the running app and passes its links.
- [ ] `--background`; links on the command line.
- [ ] Browser bridge: serve it, register it on start (not in debug builds), the AppImage copy.
- [ ] System notifications with buttons (D52), browser notices with the system fallback (D65), a notice opening its download (D98).
- [ ] Open a file, with the warning for programs; show in folder; folder and file pickers.
- [ ] Start at sign-in.
- [ ] Startup failure dialogs.
- [ ] Installers for Windows and Linux; the updater comes with release item 7.
