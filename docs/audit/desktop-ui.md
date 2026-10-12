# Desktop UI audit (apps/desktop/src, scripts, configs, tests)

> This audit is a snapshot: its paths and line numbers name the code as it was when the audit was made (it was recorded in commit 0bd101a). Some files have since moved or been split, so a path that no longer exists, or a line that no longer matches, refers to the code of that time; `git log -- <path>` shows the commit that moved or removed it.

Scope note: the working tree has an uncommitted, stricter shared ESLint config (`apps/desktop/eslint.config.js` now uses `@wirecatch/eslint-config`). Under it `eslint .` reports 674 errors in this package (329 `no-magic-numbers`, 79 `button-has-type`, 56 `require-await`, 17 `no-restricted-imports`, 13 `complexity`, 8 `max-lines`, ...). Those mechanical hits are not repeated below; findings here are the structural problems behind them plus issues no linter catches.

## DUI-1 Download store silently drops `processing` and `media` updates
- severity: high
- category: state
- location: apps/desktop/src/lib/state/downloads.svelte.ts:91-122, apps/desktop/src/lib/components/DownloadRow.svelte:31, apps/desktop/src/lib/components/DetailPanel.svelte:85, apps/desktop/src/lib/components/DetailPanel.svelte:106
- problem: `DownloadStore.apply` copies fields one by one from a hand-written list (url, file_name, path, queue, category, status, total, done, speed, connections, eta_secs, error, waiting, added_at, completed_at). `DownloadSummary` also has `processing` and `media`, which are never copied after a row is first inserted. A download moves to `processing` through an update, so its row keeps `processing: null`: the list and detail panel show the generic "Processing" label and an indeterminate bar instead of "Converting, 45%", and `media` (quality, source) stays whatever it was at insert. The per-download windows replace the row whole, so they are correct, which makes the main window disagree with them. No test covers it (`downloads.svelte.test.ts` only sets `processing: null`). Any field added to the protocol later will be dropped the same way.
- fix: drive the copy from the type, not a hand list: a `const FIELDS = [...] as const satisfies readonly (keyof DownloadSummary)[]` plus a compile-time exhaustiveness check (`Exclude<keyof DownloadSummary, (typeof FIELDS)[number] | 'id'>` must be `never`), with structural-equality comparison for the object fields (`error`, `waiting`, `processing`, `media`). Add a store test that applies an update changing every field and asserts the row equals the update.
- cross-package: no

## DUI-2 Settings are written as a whole document, last writer wins, with a racy optimistic rollback
- severity: high
- category: state
- location: apps/desktop/src/lib/state/app.svelte.ts:831-842, apps/desktop/src/lib/state/app.svelte.ts:916-924, apps/desktop/src/lib/window/download-window.svelte.ts:451-472, apps/desktop/src-tauri/src/commands.rs:135, crates/core/src/manager/preferences.rs:27
- problem: every settings change sends the full `AppSettings` snapshot held by that webview, and the core replaces its settings with it. The main window, every download window (D23, each its own webview with its own copy) and the extension (D5, via patches) all write settings. A change made elsewhere that has not yet reached this webview's `settings_changed` is reverted by the next write from here. Within one window, two quick changes A then B race: if A fails, the rollback restores the snapshot taken before A, discarding B locally although B succeeded; and since Tauri async commands are not ordered, the core can apply B's document before A's and end with A's older document, losing B.
- fix: add a settings patch command in the core and desktop shell (a typed partial like `ExtensionSettingsPatch`, or one command per setting group) and have `updateSettings` send only the changed fields. Until then, serialize writes in a single promise chain in `AppState` and roll back only the fields the failed change touched. Share the one implementation with `DownloadWindowState` (see DUI-14).
- cross-package: yes (crates/protocol, crates/core, apps/desktop/src-tauri)

## DUI-3 IPC contract with the Rust shell is hand-mirrored and untyped
- severity: high
- category: type-safety
- location: apps/desktop/src/lib/backend/types.ts:22-52, apps/desktop/src/lib/backend/types.ts:170-226, apps/desktop/src/lib/backend/tauri.ts:55-66, apps/desktop/src-tauri/src/questions.rs:58, crates/core (AddonRequestItem, MediaReading)
- problem: `Question`, `AddonRequest`, `AddonRequestItem` and `MediaReading` are redefined by hand in TypeScript ("Mirrors `questions::Question` in `src-tauri`") instead of being generated, because their Rust types live in `wirecatch_core` and `src-tauri`, not `crates/protocol` (contradicts D15 and the CLAUDE.md rule that protocol is the single source of truth). Every command result is typed by a caller-supplied generic (`#call<T>(command, args)`), and argument names are untyped `Record<string, unknown>` relying on Tauri's camelCase mapping. The only guard is a test that command names match; a renamed argument, a changed return type or a new `Question` variant compiles and fails at runtime.
- fix: move `Question`, `AddonRequestItem` and `MediaReading` into `crates/protocol` (or derive `TS` on them and export into `packages/protocol`), and generate typed command bindings (for example tauri-specta, or a hand-written `CommandMap` interface `{ [name]: { args; result } }` generated from the Rust command signatures) so `#call` is `call<K extends keyof CommandMap>(k, args: CommandMap[K]['args']): Promise<CommandMap[K]['result']>`.
- cross-package: yes (crates/protocol, crates/core, apps/desktop/src-tauri, packages/protocol)

## DUI-4 Control flow depends on the wording of a Rust error message
- severity: medium
- category: design
- location: apps/desktop/src/lib/addons.ts:13-15, apps/desktop/src/lib/window/download-window.svelte.ts:299, crates/core/src/error.rs:35, apps/desktop/src-tauri/src/api.rs:516
- problem: `isAddonRequired` decides whether a failed `choose_media` should wait for the add-on by checking `message.startsWith('This needs the media add-on')`. Commands reject with plain strings (`BackendError` carries only `message`), so the UI cannot branch on what went wrong without parsing copy. A Rust test pins the prefix, but any copy edit, translation or second wording (crates/core/src/manager/media.rs:874 already has a different sentence) breaks behavior.
- fix: return a structured command error `{ code, message, fix }` from the shell (the protocol already has `ErrorCode` and `UserError`), add `addon_required` as a code, carry `code` on `BackendError`, and test `err.code === 'addon_required'`.
- cross-package: yes (crates/core, apps/desktop/src-tauri, crates/protocol)

## DUI-5 Download action eligibility is defined in five places and the rules disagree
- severity: medium
- category: duplication
- location: apps/desktop/src/lib/state/app.svelte.ts:611-617, apps/desktop/src/lib/components/DetailPanel.svelte:40-51, apps/desktop/src/lib/components/ContextMenu.svelte:21-77, apps/desktop/src/lib/commands.ts:48-120, apps/desktop/src/lib/window/ProgressView.svelte:41-46, apps/desktop/src/lib/components/DownloadRow.svelte:85
- problem: the "is running" status set (`downloading || connecting || queued || processing`) is copied five times, and three separate action lists decide which actions to show. They differ: the multi-select panel offers Resume for failed rows (`anyStopped` includes `failed`), the context menu offers Resume only for paused rows, and the palette offers Pause, Resume and Try again unconditionally. The palette's header comment says "one behavior per action", which the context menu and detail panel do not honor. Copy link is also implemented twice with different error text (DetailPanel.svelte:54, ContextMenu.svelte:48).
- fix: add `lib/actions.ts` with status predicates (`isRunning`, `canPause`, `canResume`, `canRetry`) built on `groupOf` from `state/view.ts`, and one `selectionActions(app, rows)` that returns `{ id, label, run, shortcut }`. Build the context menu, the detail panel buttons and the `sel.*` palette commands from it.
- cross-package: no

## DUI-6 `AppState` is a 1,000-line god object
- severity: medium
- category: design
- location: apps/desktop/src/lib/state/app.svelte.ts:79-997
- problem: one class owns loading and event routing, list view and selection, toasts, dialogs and navigation, queues, settings and import/export, question counts, storage problems, add-ons and the clock. `handle` has complexity 23 and the file is 783 non-blank lines. Every component gets the whole object via `getApp()`, so any component can mutate any of it (for example `app.screen = 'downloads'` in Settings.svelte:105 and Shell.svelte:88 bypasses `show`), and unit-testing one concern needs the full mock backend.
- fix: keep `AppState` as a thin composition root and split into focused stores in `lib/state/`: `SelectionState` (selection, active, anchor, range logic), `UiState` (screen, dialog, palette, wide, detailsOpen, toasts), `QueueStore`, `SettingsStore` (with the patch logic from DUI-2), `AddonStore`, `QuestionCounts`. Route events through a `Record<UiEvent['type'], handler>` map. Expose navigation only through methods.
- cross-package: no

## DUI-7 Settings screen is a 30-branch if/else on setting ids
- severity: medium
- category: design
- location: apps/desktop/src/lib/components/Settings.svelte:150-529, apps/desktop/src/lib/settings-schema.ts
- problem: `settings-schema.ts` already lists every setting with label, help and section, but rendering ignores it and switches on `def.id` string by string in one snippet (695 lines). Eleven branches are the same `Toggle` with a different property path; the KB/MB unit conversion is implemented here and again in QueueDialog.svelte:12-20; the theme radiogroup's roving-focus keyboard logic is inline in markup. Adding a setting means editing the schema and a new markup branch, and a schema id with no branch renders an empty control with no error.
- fix: give `SettingDef` a typed `control` field (`{ kind: 'toggle'; get(s): boolean; set(d, v): void } | { kind: 'select'; ... } | { kind: 'custom'; component }`) and render with a small `SettingControl` component per kind. Move the radiogroup into a `Segmented` component and the size-with-unit input into a `SizeInput` component shared with QueueDialog. Add a test that every schema id has a control.
- cross-package: no

## DUI-8 Add dialog loses what the person typed when launch links arrive
- severity: medium
- category: state
- location: apps/desktop/src/lib/state/app.svelte.ts:341-352, apps/desktop/src/lib/components/Dialogs.svelte:18, apps/desktop/src/lib/components/AddDialog.svelte:12-20
- problem: when links come from the command line (`links` shell signal) while the Add dialog is open, `#takeLaunchLinks` builds a new dialog object from `open.urls` (the dialog's initial links, not the edited text) and `Dialogs.svelte` wraps dialogs in `{#key d}`, so `AddDialog` is destroyed and remounted. The edited links, file name, chosen folder and queue are discarded, and focus jumps.
- fix: give the open Add dialog a way to receive more links (for example an `AddDraft` object in state that `AddDialog` binds to, with `appendLinks(urls)`), and key `Dialogs` on `d.kind` rather than object identity. Add a test: open Add, type a file name, fire a `links` signal, assert the name and the new links are both present.
- cross-package: no

## DUI-9 Download grid: active row can point at a node that is not rendered, and cells have no column headers
- severity: medium
- category: accessibility
- location: apps/desktop/src/lib/components/DownloadList.svelte:262-265, apps/desktop/src/lib/components/DownloadList.svelte:417-425, apps/desktop/src/lib/components/DownloadList.svelte:440-442, apps/desktop/src/lib/components/DownloadRow.svelte:36-48
- problem: the list is virtualized (only rows in view plus 6 are rendered), but `aria-activedescendant` is set to `row-{active}` whenever the active id is in the list. After scrolling with the wheel or scrollbar, the active row is no longer in the DOM and the grid's active descendant is a dangling id, so screen readers lose their position. `reveal` only runs when `active` changes. Separately, the visible header row is `aria-hidden="true"` and there is no `role="columnheader"`, so each `gridcell` (Size, Done, Speed, Time left, Status) is announced without its column name.
- fix: always render the active row (append it to `windowIds` when outside the window, positioned absolutely), or clear `aria-activedescendant` and move focus semantics when it scrolls out. Render the header inside the grid as `role="row"` with `role="columnheader"` cells (`aria-rowindex=1`, data rows from 2) and keep it visually where it is.
- cross-package: no

## DUI-10 Progress window live region announces every percent
- severity: medium
- category: accessibility
- location: apps/desktop/src/lib/window/ProgressView.svelte:127, apps/desktop/src/lib/format.ts:363-375
- problem: the status line is `role="status"` (implicitly `aria-live="polite"`) and renders `statusShort(row)`, which during processing is "Converting, 45%" and while queued is "3rd in line". Every per-mille step that crosses a percent re-announces, up to about 100 announcements per conversion, drowning the screen reader. The add-on card already solved this with `addonAnnouncement` (quarters only).
- fix: render the visual status without a live role, and add a visually hidden `role="status"` element fed by a throttled `statusAnnouncement(row)` that changes only on status changes and at quarter marks, mirroring `addonAnnouncement` in addons.ts.
- cross-package: no

## DUI-11 Validation limits and defaults are copied from the core
- severity: medium
- category: magic-value
- location: apps/desktop/src/lib/components/Settings.svelte:463-525 (min/max 1..32, fallbacks 8 and 4, `5 * MB`), apps/desktop/src/lib/components/QueueDialog.svelte:16-24, apps/desktop/src/lib/components/QueueDialog.svelte:46-47, apps/desktop/src/lib/components/QueueDialog.svelte:149, apps/desktop/src/lib/components/DetailPanel.svelte:190-192, apps/desktop/src/lib/components/AddDialog.svelte:17, crates/core/src/settings.rs:21, crates/core/src/settings.rs:95
- problem: the UI hard-codes the core's `MAX_CONNECTIONS = 32` (and the same 32 for queue concurrency), the default 8 connections (as a `?? 8` display fallback and a clamp fallback), 4 concurrent downloads, a 5 MB/s and 2 MB/s default limit, a 01:00 to 07:00 schedule, and queue id `1` as a fallback default queue. The error text "between 1 and 32" is duplicated in Rust and TypeScript. Changing a limit in the core leaves the UI accepting or rejecting the wrong range.
- fix: export the limits and defaults from `crates/protocol` (constants generated to TS, or a `limits` object returned by `get_settings`), import them in the UI, and drop the `?? 1` / `?? 8` fallbacks in favor of rendering nothing until data is loaded.
- cross-package: yes (crates/protocol, crates/core, packages/protocol)

## DUI-12 Layout numbers are duplicated between TypeScript and CSS
- severity: medium
- category: magic-value
- location: apps/desktop/src/lib/components/DownloadList.svelte:221-223, apps/desktop/src/app.css:42, apps/desktop/src/lib/components/Shell.svelte:23, apps/desktop/src/lib/components/Shell.svelte:170-175, apps/desktop/src/lib/components/DetailPanel.svelte:335, apps/desktop/src/lib/components/ContextMenu.svelte:79-80, apps/desktop/src/lib/components/ContextMenu.svelte:136-150, apps/desktop/src/lib/window/MockWindow.svelte:53-54
- problem: `ROW = 44` must match `--row-h: 44px` ("Must match" comment, nothing enforces it); `DOCK_WIDTH = 236 + 316 + 600` repeats the sidebar width written twice in Shell's grid CSS and the panel width in DetailPanel's CSS; the context menu clamps with 220 px and 32 px per item while its CSS says `min-width: 200px` and `height: 30px` plus separators, so the clamp is wrong for long menus. Changing one side silently breaks virtualization offsets, docking or menu placement.
- fix: put the numbers in one TS module (`lib/layout.ts`: `ROW_HEIGHT`, `SIDEBAR_WIDTH`, `DETAIL_WIDTH`) and push them into CSS via `style:--row-h` / `style:--sidebar-w` on the shell root; for the menu, measure `el.getBoundingClientRect()` after mount instead of estimating.
- cross-package: no

## DUI-13 Keyboard shortcuts are declared in four places
- severity: medium
- category: duplication
- location: apps/desktop/src/lib/commands.ts:22-37, apps/desktop/src/lib/commands.ts:82-375 (`shortcut:` fields), apps/desktop/src/lib/components/Shell.svelte:65-91, apps/desktop/src/lib/components/DownloadList.svelte:313-369, apps/desktop/src/lib/components/Sidebar.svelte:104, apps/desktop/src/lib/components/Sidebar.svelte:248-253, apps/desktop/src/lib/components/Toolbar.svelte (aria-keyshortcuts)
- problem: the help table (`SHORTCUTS`), the palette's `shortcut` labels, the actual handlers (two `onkeydown` switch statements with complexity 20 and 31) and literal `<kbd>Ctrl K</kbd>` labels are maintained separately. They already drift: the handlers support PageUp, PageDown, Home, End, ContextMenu and Shift+F10, none of which the shortcuts dialog lists, and the sidebar writes "Ctrl K" while the table writes "Ctrl+K".
- fix: one `lib/keymap.ts` table `{ id, keys, scope: 'global' | 'list', description, run }` that the handlers dispatch from, the shortcuts dialog renders, the palette reads by command id, and `aria-keyshortcuts` and `<kbd>` labels are derived from.
- cross-package: no

## DUI-14 `AppState` and `DownloadWindowState` duplicate their action plumbing
- severity: medium
- category: duplication
- location: apps/desktop/src/lib/state/app.svelte.ts:62-66, apps/desktop/src/lib/window/download-window.svelte.ts:41-45, apps/desktop/src/lib/state/app.svelte.ts:695-713, apps/desktop/src/lib/window/download-window.svelte.ts:422-447, apps/desktop/src/lib/state/app.svelte.ts:831-842, apps/desktop/src/lib/window/download-window.svelte.ts:451-462
- problem: `message(err)` is copied verbatim, the `FixAction` switch is implemented twice, settings save with optimistic rollback is implemented twice (with the DUI-2 race in both), and the `#run(async () => { await x; return true; })` idiom appears 20 times (13 in AppState, 7 in the window) to turn a void call into a success flag.
- fix: add `lib/state/actions.ts` with `errorText(err)`, `attempt(action): Promise<boolean>` and `runFix(action, ports)`, and a shared `SettingsWriter` used by both states.
- cross-package: no

## DUI-15 Repeated UI patterns that should be shared components
- severity: medium
- category: duplication
- location: apps/desktop/src/lib/components/DownloadRow.svelte:82-93, apps/desktop/src/lib/components/DetailPanel.svelte:125-130, apps/desktop/src/lib/components/ToastItem.svelte:47-53, apps/desktop/src/lib/window/ProgressView.svelte:143-162, apps/desktop/src/lib/window/AddonView.svelte:71, apps/desktop/src/lib/components/AddonCard.svelte:78, apps/desktop/src/lib/components/AddDialog.svelte:123, apps/desktop/src/lib/components/DetailPanel.svelte:179-183, apps/desktop/src/lib/components/DetailPanel.svelte:278-286, apps/desktop/src/lib/components/Settings.svelte:359-372, apps/desktop/src/lib/window/FileQuestion.svelte:62-82, apps/desktop/src/lib/window/MediaQuestion.svelte:271, apps/desktop/src/lib/components/Shell.svelte:34-39, apps/desktop/src/routes/window/download/+page.svelte:54-59, apps/desktop/src/lib/components/Dialog.svelte:25-78, apps/desktop/src/lib/components/PaletteBody.svelte:35-45
- problem: the "wire" progress bar (track, fill, `scaleX(completed ? 1 : fraction ?? 0)`, indeterminate animation, status colors) is reimplemented with its own markup and CSS in 6 places, and only ProgressView gives it `role="progressbar"`. The queue `<select>` is repeated 7 times, the "Save to ... Change" folder field 3 times, the theme-to-`data-theme` effect twice, and modal open/focus-restore logic in Dialog and PaletteBody separately (the palette has no Tab wrap).
- fix: extract `Wire.svelte` (props: fraction, status, indeterminate, label, optional progressbar semantics), `QueueSelect.svelte`, `FolderField.svelte`, a `applyTheme(theme)` helper or `ThemeRoot.svelte`, and a `modal` attachment used by Dialog and PaletteBody.
- cross-package: no

## DUI-16 CSS has color tokens but no spacing, layer, motion or weight tokens, and the dark palette is written twice
- severity: medium
- category: magic-value
- location: apps/desktop/src/app.css:46-98, apps/desktop/src/lib/components/ContextMenu.svelte:135, apps/desktop/src/lib/components/DetailPanel.svelte:305, apps/desktop/src/lib/components/Toasts.svelte:19, apps/desktop/src/lib/window/MockWindow.svelte:71, apps/desktop/src/lib/window/MockWindow.svelte:84
- problem: the 22 dark-theme values are copied verbatim into both the `prefers-color-scheme` block and `[data-theme='dark']`, so a palette change must be made twice. Components use 217 raw px padding, margin and gap declarations, 57 raw widths and heights, 5 ad hoc z-index values (10, 20, 30, 40, 41) with no scale, 12 hand-picked durations (120, 160, 180, 200, 240, 400 ms, 0.9 to 1.6 s), and 7 distinct font weights (400, 450, 500, 520, 600, 620, 650). For a product heading to commercial polish this blocks consistent theming and density changes.
- fix: define the dark palette once (for example in a `@layer` with `:root[data-theme='dark'], :root:not([data-theme='light'])` inside the media query via a shared custom-property block, or generate both blocks from one source), and add `--space-1..6`, `--z-panel/toast/menu/window`, `--duration-fast/base/slow` and `--weight-regular/medium/strong` tokens, then migrate components.
- cross-package: no

## DUI-17 All UI tests run against a 1,450-line mock that re-implements the core
- severity: medium
- category: testing
- location: apps/desktop/src/lib/backend/mock.ts:476-640, apps/desktop/src/lib/window/registry.ts:1-13, apps/desktop/src/lib/testing/setup.ts:10-19, apps/desktop/e2e
- problem: `MockBackend` re-implements queue admission, schedules, wait reasons, processing steps, media choices and add-on installs (its `tick` has complexity 57), and `WindowRegistry` re-implements the window cascade rules of `src-tauri/src/download_windows.rs` (five question windows, cascade step). Every Vitest and Playwright test drives the UI through this mock, so they prove the UI agrees with the mock, not with the core. Nothing checks that the mock emits the same event sequences as the core (DUI-1 shipped because the store was only exercised with mock data that never updates `processing`).
- fix: record event and command fixtures from the real core (a Rust test in `apps/desktop/src-tauri` that drives the manager through representative scenarios and writes JSON), and add a Vitest contract suite that replays them through `AppState.handle` and asserts on the store; reduce the mock to a scripted replayer for demos.
- cross-package: yes (apps/desktop/src-tauri, crates/core)

## DUI-18 Playwright accessibility, layout and performance suites are not in CI
- severity: medium
- category: testing
- location: .github/workflows/ci.yml:213-222, apps/desktop/e2e/a11y.spec.ts, apps/desktop/e2e/layout.spec.ts, apps/desktop/e2e/performance.spec.ts, apps/desktop/playwright.config.ts:24
- problem: CI runs `pnpm check`, `lint`, `test` and `build` only. The axe WCAG AA audit in both themes, the keyboard walk, the layout checks and the 10,000-row performance budget exist but run only when someone remembers to run `pnpm e2e` locally, and the default config requires the `msedge` channel, which Linux runners do not have.
- fix: add a CI job (Windows runner for msedge, or `PW_CHANNEL=chromium` with `playwright install chromium` on Linux) that runs `pnpm e2e` and `pnpm e2e:perf` for desktop changes, uploading the report on failure.
- cross-package: yes (.github/workflows)

## DUI-19 Logic branches on display strings
- severity: low
- category: design
- location: apps/desktop/src/lib/components/Sidebar.svelte:176, apps/desktop/src/lib/components/Sidebar.svelte:66, apps/desktop/src/lib/components/QueueDialog.svelte:40, apps/desktop/src/lib/format.ts:399-404
- problem: Sidebar appends ", up to N at once" only when `app.queueStatus(q) === 'Idle'`; it shrinks the readout font when `headline.length > 8`; QueueDialog compares `formatDays` output to `'Weekdays'`/`'Weekends'`; `whenSaid` re-parses `formatDate` output by `startsWith('Today, ')` and `slice(7)`. Any copy edit silently changes behavior.
- fix: return structured values (`queueState(q): { kind: 'idle' | 'paused' | ...; text }`, `dayPattern(days): 'every' | 'weekdays' | 'weekends' | 'custom'`, a `relativeDay(ms, now)` used by both formatters) and use a CSS class from the kind instead of string length.
- cross-package: no

## DUI-20 Async teardown gaps: window subscription can leak, settings flash timer is never cleared
- severity: low
- category: state
- location: apps/desktop/src/lib/window/download-window.svelte.ts:104-131, apps/desktop/src/lib/window/download-window.svelte.ts:158-161, apps/desktop/src/lib/window/MockWindow.svelte:38-41, apps/desktop/src/lib/components/Settings.svelte:40-54
- problem: `DownloadWindowState.init` assigns `this.#unsubscribe = await backend.subscribe(...)`; if `dispose` runs during that await (a sample window closed right after opening), `dispose` sees `null` and the listener assigned afterwards is never removed, so the disposed state keeps handling every event. `AppState` guards this with a generation counter; the window state does not. In Settings, `setTimeout(() => (flash = null), 1600)` inside an effect has no cleanup, so a second jump within 1.6 s has its highlight cleared early by the first timer.
- fix: add the same generation or `#disposed` check after each await in `DownloadWindowState.init` and call the unsubscribe immediately if disposed; return a cleanup from the Settings effect that clears the timer.
- cross-package: no

## DUI-21 Ids are plain numbers and DOM values are cast to domain types
- severity: low
- category: type-safety
- location: packages/protocol/src/generated/DownloadId.ts, apps/desktop/src/lib/backend/types.ts:111-144, apps/desktop/src/lib/state/app.svelte.ts:196-203, apps/desktop/src/lib/components/DetailPanel.svelte:64, apps/desktop/src/lib/components/Settings.svelte:380, apps/desktop/src/lib/components/Settings.svelte:434, apps/desktop/src/lib/state/hints.svelte.ts:24, apps/desktop/src/lib/state/app.svelte.ts:761-762
- problem: `DownloadId`, `QueueId` and question ids are all `number`, so a question id passes where a download id is expected (both flow through `waitingCount`, window targets and `chooseMedia(id)`); addon ids are bare `string` next to `MEDIA_ADDON = 'media'`. `<select>` values are cast with `as OutputFormat` / `as QueueId` without checking membership, and stored hint ids are cast `as HintId[]` after only a `typeof x === 'string'` filter.
- fix: generate branded id types (or at least `QuestionId` and `AddonId` aliases) in the protocol; add tiny parsers (`parseOutputFormat(v): OutputFormat | null`, `isHintId`) and use them instead of `as`.
- cross-package: yes (crates/protocol, packages/protocol)

## DUI-22 The download-window route bypasses the backend abstraction
- severity: low
- category: design
- location: apps/desktop/src/routes/window/download/+page.svelte:6-7, apps/desktop/src/routes/window/download/+page.svelte:27-46, apps/desktop/src/lib/backend/tauri.ts:164-174, apps/desktop/src/lib/backend/index.ts:1-5
- problem: the route constructs `TauriBackend` directly and calls `fitDownloadWindow` and `closeDownloadWindow`, which are not on the `Backend` interface, so the page cannot run on the mock and the lint layering rule in the new ESLint config flags it. `lib/backend/index.ts` does not re-export `Question`, `AddonRequestItem`, `WindowHost` types, which is why window and state modules reach into `backend/types` (17 `no-restricted-imports` hits).
- fix: add an optional `windowShell?: { fit(height): Promise<void>; close(): Promise<void> }` to `Backend` (mock implements it as no-ops), build the window page from `createBackend()`, and re-export the public types from `$lib/backend`.
- cross-package: no

## DUI-23 Toast accessibility: nested live regions and actions in auto-dismissing notes
- severity: low
- category: accessibility
- location: apps/desktop/src/lib/components/Toasts.svelte:8, apps/desktop/src/lib/components/ToastItem.svelte:10, apps/desktop/src/lib/components/ToastItem.svelte:30, apps/desktop/src/lib/components/ToastItem.svelte:36-46
- problem: error toasts are `role="alert"` inside a container that is already `aria-live="polite"`, which double-announces in some screen readers. Capture toasts hold three action buttons (Change, Open folder, Start without asking) and vanish after 12 s; the timer pauses on hover or focus, but there is no keyboard route to the toast region before it disappears.
- fix: drop the container's `aria-live` and give info toasts their own `role="status"`; add a shortcut (for example F6 cycles landmarks, or a palette command "Go to notifications") and do not auto-dismiss toasts that have actions while the window is focused, or keep them for at least 20 s per WCAG 2.2.1 guidance.
- cross-package: no

## DUI-24 Product name and other brand strings are literals across the UI
- severity: low
- category: magic-value
- location: apps/desktop/src/lib/components/EmptyState.svelte:26-34, apps/desktop/src/lib/components/Shell.svelte:136, apps/desktop/src/lib/components/Sidebar.svelte:64, apps/desktop/src/lib/components/StorageBanner.svelte:19, apps/desktop/src/lib/backend/tauri.ts:243, apps/desktop/src/lib/backend/tauri.ts:250, apps/desktop/src/lib/state/app.svelte.ts:845, apps/desktop/src/lib/state/hints.svelte.ts:17
- problem: "Wirecatch" appears in 28 lines of user-facing copy plus file names (`wirecatch-settings.json`), dialog filter names (twice, identical) and storage keys. For a commercial edition or rename, every string must be found by hand.
- fix: add `lib/brand.ts` (`PRODUCT_NAME`, `SETTINGS_FILE_NAME`, `SETTINGS_FILTER`, `STORAGE_PREFIX`) and use it in copy via template strings; reuse one filter constant in `pickSaveFile` and `pickOpenFile`.
- cross-package: no

## DUI-25 Hot-path allocation and serial IPC in the store and add flow
- severity: low
- category: performance
- location: apps/desktop/src/lib/state/downloads.svelte.ts:120, apps/desktop/src/lib/state/app.svelte.ts:588-598
- problem: every progress batch runs `JSON.stringify(row.waiting) !== JSON.stringify(s.waiting)` for every changed row, two string allocations per row per tick, against the project rule of no per-update allocations in hot paths. Separately, `add()` awaits each `addDownload` in sequence, so pasting 50 links costs 50 serial IPC round trips and raises one error toast per failure, unlike `#bulk`, which reports once.
- fix: compare `waiting` structurally with a small `sameWait(a, b)` (type plus the few data fields), as `sameError` already does. Send adds with `Promise.allSettled` (or a batch `add_downloads` command) and report failures through `#bulk`'s single message.
- cross-package: no

## DUI-26 Dead hint, mixed sample code and an unguarded bundle check
- severity: low
- category: convention
- location: apps/desktop/src/lib/state/hints.svelte.ts:4-13, apps/desktop/src/lib/backend/mock.ts, apps/desktop/src/lib/backend/mock-data.ts, apps/desktop/src/lib/window/registry.ts, apps/desktop/src/lib/window/MockWindow.svelte, apps/desktop/src/lib/window/MockWindows.svelte, apps/desktop/scripts/check-bundle.mjs:6-11
- problem: the `'paste'` hint has text but is never offered by `DownloadList` (dead). Sample-only code (mock backend, sample data, window registry, mock windows) sits beside production modules in `lib/backend` and `lib/window`, so the boundary is enforced only by a dynamic import and by `check-bundle.mjs`, which greps the build for four hand-picked marker strings; if those strings are edited in the mock, the check passes vacuously.
- fix: remove the `'paste'` hint or wire it to the empty state; move sample code to `lib/sample/`; have `check-bundle.mjs` export its markers and add a Vitest test asserting each marker still occurs in `lib/sample` sources (or emit a sentinel constant from the sample module and grep for that).
- cross-package: no

## DUI-27 Component behavior is only tested through whole-app integration files named after review rounds
- severity: low
- category: testing
- location: apps/desktop/src/lib/review-fixes.test.ts, apps/desktop/src/lib/features.test.ts, apps/desktop/src/lib/keyboard.test.ts, apps/desktop/src/lib/testing/setup.ts:10-19
- problem: apart from `DownloadWindow`, no component is rendered in isolation; Settings, ContextMenu, DetailPanel, QueueDialog, Sidebar and Toolbar are covered only through `renderApp()` in files organized by when bugs were found (`review-fixes.test.ts`) rather than by unit. Failures point at a review round, not a component, and edge cases like DUI-1, DUI-8 and DUI-9 have no natural home.
- fix: colocate tests per module (`ContextMenu.test.ts`, `Settings.test.ts`, `downloads.svelte.test.ts` additions), move the scenarios in `review-fixes.test.ts` and `features.test.ts` next to the code they cover, and keep `renderApp()` for a small set of end-to-end flows.
- cross-package: no

Summary: 27 findings: 3 high, 15 medium, 9 low; no critical.
The most urgent are a real data bug (the store drops `processing`/`media` updates), last-writer-wins settings writes across webviews, and an IPC contract mirrored by hand instead of generated.
The structural theme is duplication of rules (action eligibility, shortcuts, limits, settings plumbing, UI widgets) across components instead of one shared module; layout tokens and accessibility of the virtualized grid follow.
