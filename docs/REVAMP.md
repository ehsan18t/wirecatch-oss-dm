# Interface revamp

The whole visible interface is being redesigned on one design system, for the desktop app and the browser extension together. It is a product workstream beside the audit, not a set of audit findings, so it has its own page; [AUDIT](AUDIT.md) W12 and W13 say which of their findings it absorbs, and [ROADMAP](ROADMAP.md) lists it as a phase.

The work is on branch `ws/ui-revamp` and its child branches. None of it is on main yet: the decision record D36 in [DECISIONS](DECISIONS.md) arrives with the branch, as its first commit.

## What was decided

Settled with the maintainer on 2026-10-13, and recorded as D36:

- Every surface a person can see gets the new look by the end: every desktop screen, dialog and window, the extension's popup, options page and in-page media button, notices and toasts, and as far as the platforms allow the tray menu and the installer pages. The maintainer's MIT-licensed toolbox app is a source of parts and ideas, not a copy to match.
- Stack: Tailwind 4 with tailwind-variants, icons from Iconify through unplugin-icons with the Lucide set, and plain Svelte 5 with Vite. SvelteKit is removed from the desktop app. Tokens, themes, accents, motion and the primitives live in the shared package `packages/ui`, used by both apps.
- Windows: a frameless shell on every window, Mica on Windows 11, Acrylic on Windows 10, a solid themed background on Linux, and CSS backdrop blur for in-app layers (dialogs, menus, toasts, popovers, the slide-over) everywhere.
- Theme (system, light, dark) and accent are app settings in `crates/protocol`, saved by the core, so every open window changes at once. The default follows the system theme with Wirecatch's current cyan accent; the toolbox's seven accents are offered, plus a few more and the Windows system accent.
- The main window has a collapsible sidebar (rail and drawer) for categories, queues and status filters, a full-width list with inline progress and hover actions, and a right slide-over for details that Escape closes. Settings and add-ons are full pages reached from the sidebar, not dialogs.
- Per-download windows stay for browser-caught downloads, redesigned; a setting turns them off for a toast and the main window.
- What feels natural to the person decides the flow, not a click count. The earlier rule that everything is within one or two clicks is retired.

## Done on the branch

- Foundation: SvelteKit removed (each window is a Vite page), the new stack wired into both apps, `packages/ui` created, the lint rules that named the old primitives updated rather than switched off.
- Theme and window material: the theme and accent settings, the frameless shell with working caption buttons and snap layouts on Windows 11, Mica and Acrylic, and the setting for per-download windows.
- Design system and gallery: 24 primitives and the shell layers, a development-only gallery, a `tokens-only` lint rule that keeps raw values out of the interface, and gallery browser checks in the gate.
- Main window: sidebar, list and slide-over built on the design system, with the start-up notice (CORE-1) carried through the protocol, core and shell. Under review.

Started: the extension's popup and options pages, on `ws/ui-extension`.

## Left to build

The maintainer split the work on 2026-10-13. Phase 1 is this round; phase 2 waits until it is on main and the maintainer has reviewed it.

### Phase 1, this round

- The main window, finishing its review.
- The add-download flow.
- The download window: where to save, progress, and the file and media quality questions, with an announcement policy for progress (DUI-10).
- The extension's popup and options pages, started on `ws/ui-extension`.

### Phase 2, later

- Settings pages, including a "Sites on your network" list (D39) and an "Open log folder" action (D40). Offering to add a site adds the exact page host, never its registrable domain: multi-tenant domains such as sharepoint.com are not on the Public Suffix List.
- The add-ons page.
- Every dialog still styled by the old stylesheet, among them the queue form and the file type editor (which calls `normalizeFileType`).
- The command palette.
- The in-page media button.
- The tray menu and the installer pages, as far as the platforms allow.
- The "already running" experience: a second copy of the app that finds the list open (`AlreadyOpen`, D41) says Wirecatch is already running instead of showing an "unavailable" notice.
- "Copy diagnostics" in the extension popup, reading `readDiagnostics(browser.storage.session)` (EXT-7).
- Notices and toasts, and the open low findings of the theme review: from the tray in a development build the page reloads on wake and loses the toast (to check in a release build), and the material has no automated check in the real app (WebView2 shows no backdrop blur over Mica).

## Before it lands on main

The commits are regrouped (the decision record first), the full gate passes, no `*-HANDOFF.md` file is in the tree, and a download window has been opened in a development run and in a packaged build. Findings of W12 that survive a redesign are listed in [AUDIT](AUDIT.md).
