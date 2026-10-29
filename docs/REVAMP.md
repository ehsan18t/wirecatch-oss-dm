# Interface revamp

The whole visible interface is being redesigned on one design system, for the desktop app and the browser extension together. It is a product workstream beside the audit, not a set of audit findings, so it has its own page; [AUDIT](AUDIT.md) W12 and W13 say which of their findings it absorbs, and [ROADMAP](ROADMAP.md) lists it as a phase.

What was decided, and how it was built, is D36 in [DECISIONS](DECISIONS.md), with its amendments: D36.2 (no backdrop blur over the system material, since WebView2 draws none over the transparent page the material needs) and D36.10 (theme, accent and window shell as built). D36.2's second amendment (2026-10-15) gave a window over a material no drawing surface of its own, which kept a white square out of its corner; D45 (2026-10-21) then dropped the material, so every window is opaque in its scheme's base colour from its first frame. This page does not restate D36; it says what is done, what is left and what has not been seen working.

## Phase 1: done (2026-10-14)

Built on `ws/ui-revamp`, reviewed screen by screen and as a whole, and joined with main's W4 and W5, whose rules hold in the rebuilt windows (D43.6).

- Foundation: SvelteKit removed, each window a Vite page (`index.html`, `download.html`); Tailwind 4, tailwind-variants and Lucide icons through unplugin-icons in both apps; `packages/ui` (`@wirecatch/ui`) with the tokens, light and dark schemes, ten accents plus the Windows system accent, motion, 24 primitives and the shell layers (title bar, sidebar, slide-over, dialogs, menus, toasts); a development-only gallery the gate checks in a browser; and the `tokens-only` lint rule, which keeps raw values out of the interface.
- Theme and window: the theme and accent are app settings saved by the core, and every open window follows a change at once. Every window is frameless, with caption buttons and Windows 11's snap layouts, and opaque in its scheme's base colour (D45). The "ask before each download" setting turns download windows off for a note in the main window.
- Main window: a sidebar that collapses to a rail or opens as a drawer, a full-width list with inline progress and hover actions, details in a slide-over, and each start-up notice shown in the window until dismissed (CORE-1).
- The add-download flow.
- The download window: where to save, progress, and the file and media quality questions, with the shell's own warning before a program is opened (DRS-5).
- The extension's popup and options page, on the same design system; the settings moved from the popup to the options page.

Screenshots of every state on sample data are in `apps/desktop/docs/screenshots` (`pnpm e2e:screenshots`).

## Not seen working yet

For the maintainer: these were built and compile, but nobody has seen them in the real app.

- Windows 10. Every real-app check ran on Windows 11.
- The solid themed background on Linux. The desktop crate's Linux side is compiled only by CI's Ubuntu job; the app has not been run on Linux with the revamp.

## Phase 2: done (2026-10-29)

Rebuilt on the approved look, on main with D84, D85, D86 and D87.

- Settings pages: seven pages (D85), every option's help behind an info icon (D84), "Local network sites" on Network (D39), and "Open log folder" under Troubleshooting on General (D40).
- The add-ons page: the media add-on's card heads the Media page, its state on one line with its buttons.
- Dialogs: no old stylesheet is left; every dialog is the design system's, hangs from the top, shows when more lies below its footer, and one opened over another sits lower (D87).
- The command palette, on the design system's dialog surface.
- The in-page media button, in the dark scheme's graphite with the cyan accent, its colours read from the tokens at build time (D87).
- The tray menu (Open, Pause all, Resume all, Proxy, Quit) and the Windows installer's header and sidebar pictures; Linux packages have no pages of their own.
- The "already running" experience: a second copy that finds the list open says Wirecatch is already running and closes (`Unopened::another_copy`).
- "Copy diagnostics" in the extension popup (EXT-7, D87).

Left from the carried list in [AUDIT](AUDIT.md), for W12 and W13: a link that failed after its Add dialog was closed with Escape, the queue editor's length once Auto is on, groups in a select, a host name a toast breaks at its hyphen, and the theme review's open lows.
