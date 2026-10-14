# Interface revamp

The whole visible interface is being redesigned on one design system, for the desktop app and the browser extension together. It is a product workstream beside the audit, not a set of audit findings, so it has its own page; [AUDIT](AUDIT.md) W12 and W13 say which of their findings it absorbs, and [ROADMAP](ROADMAP.md) lists it as a phase.

What was decided, and how it was built, is D36 in [DECISIONS](DECISIONS.md), with its amendments: D36.2 (no backdrop blur over the system material, since WebView2 draws none over the transparent page the material needs) and D36.10 (theme, accent and window shell as built). D36.2's second amendment (2026-10-15) gives a window over a material no drawing surface of its own (Tauri's `no_redirection_bitmap`), which keeps a white square out of its corner. This page does not restate D36; it says what is done, what is left and what has not been seen working.

## Phase 1: done (2026-10-14)

Built on `ws/ui-revamp`, reviewed screen by screen and as a whole, and joined with main's W4 and W5, whose rules hold in the rebuilt windows (D43.6).

- Foundation: SvelteKit removed, each window a Vite page (`index.html`, `download.html`); Tailwind 4, tailwind-variants and Lucide icons through unplugin-icons in both apps; `packages/ui` (`@wirecatch/ui`) with the tokens, light and dark schemes, ten accents plus the Windows system accent, motion, 24 primitives and the shell layers (title bar, sidebar, slide-over, dialogs, menus, toasts); a development-only gallery the gate checks in a browser; and the `tokens-only` lint rule, which keeps raw values out of the interface.
- Theme and window: the theme and accent are app settings saved by the core, and every open window follows a change at once. Every window is frameless, with caption buttons and Windows 11's snap layouts, Mica on Windows 11, Acrylic on Windows 10 and a solid background on Linux. The "ask before each download" setting turns download windows off for a note in the main window.
- Main window: a sidebar that collapses to a rail or opens as a drawer, a full-width list with inline progress and hover actions, details in a slide-over, and each start-up notice shown in the window until dismissed (CORE-1).
- The add-download flow.
- The download window: where to save, progress, and the file and media quality questions, with the shell's own warning before a program is opened (DRS-5).
- The extension's popup and options page, on the same design system; the settings moved from the popup to the options page.

Screenshots of every state on sample data are in `apps/desktop/docs/screenshots` (`pnpm e2e:screenshots`).

The screens phase 2 rebuilds still read `apps/desktop/src/app.css` in the `legacy` layer ([ENGINEERING](ENGINEERING.md), section 2), so they keep the old teal accent whatever accent is chosen.

## Not seen working yet

For the maintainer: these were built and compile, but nobody has seen them in the real app.

- Acrylic on Windows 10. Every real-app check ran on Windows 11, where the windows show Mica.
- The solid themed background on Linux. The desktop crate's Linux side is compiled only by CI's Ubuntu job; the app has not been run on Linux with the revamp.

## Phase 2

It starts once phase 1 is on main and the maintainer has reviewed it, and rebuilds the remaining screens on the approved look, in parallel.

- Settings pages, including a "Sites on your network" list (D39) and an "Open log folder" action (D40). Offering to add a site adds the exact page host, never its registrable domain: multi-tenant domains such as sharepoint.com are not on the Public Suffix List.
- The add-ons page.
- Every dialog still styled by the old stylesheet, among them the queue form and the file type editor (which calls `normalizeFileType`).
- The command palette.
- The in-page media button.
- The tray menu and the installer pages, as far as the platforms allow.
- The "already running" experience: a second copy of the app that finds the list open (`AlreadyOpen`, D41) says Wirecatch is already running instead of showing an "unavailable" notice.
- "Copy diagnostics" in the extension popup, reading `readDiagnostics(browser.storage.session)` (EXT-7).
- What phase 1 left open, listed in [AUDIT](AUDIT.md) under "Parts of the interface revamp's phase 1 carried into later work".
