# Code quality audit

On 2026-10-11 nine independent reviewers read the whole repository and reported 214 findings: 1 critical, 20 high, 113 medium and 80 low. The full reports, with locations and the recommended fix for each finding, are in [`audit/`](audit/):

| Report | Area | Prefix |
|---|---|---|
| [engine.md](audit/engine.md) | `crates/engine` | ENG |
| [streams-e2e.md](audit/streams-e2e.md) | `crates/streams`, `crates/e2e` | STR |
| [core-media.md](audit/core-media.md) | `crates/core/src/media`, `crates/core/src/addons` | CMD |
| [core.md](audit/core.md) | the rest of `crates/core` | CORE |
| [ipc-bridge.md](audit/ipc-bridge.md) | `crates/ipc`, `crates/bridge`, `crates/cli`, `crates/protocol` | IPB |
| [desktop-rust.md](audit/desktop-rust.md) | `apps/desktop/src-tauri` | DRS |
| [desktop-ui.md](audit/desktop-ui.md) | `apps/desktop/src` | DUI |
| [extension.md](audit/extension.md) | `apps/extension` | EXT |
| [cross-cutting.md](audit/cross-cutting.md) | seams between packages, CI, tooling, identity inventory | XC |

Many findings are one root cause seen from several packages, so they are fixed together. Every finding belongs to exactly one workstream below, and workstreams are done in this order.

The maintainer set the scope of the current round on 2026-10-13: it finishes W4, lands W5 and finishes phase 1 of the interface revamp ([REVAMP](REVAMP.md)). Everything else waits: the rest of W6, W9, W10, W11, W12, W13 and W15, and phase 2 of the revamp. The status table says which is which.

## How a finding gets fixed

1. A bug starts with a test that fails because of it. The failing test is the proof the finding is real; a finding that cannot be shown is closed as not reproduced.
2. The fix lands in its own commit with the gates from `CLAUDE.md` passing.
3. Someone other than the author of the fix reviews it before the workstream is marked done.

## Workstreams

### W0. Tooling and standards

Rules first, so the rest of the work is held to them and cannot regress.

XC-17, XC-19, XC-22, XC-23, XC-24, XC-25, XC-26, XC-27, XC-28, XC-29, XC-30, XC-31, XC-32

### W1. Critical and high bugs

Each one gets a failing regression test first.

STR-1, ENG-1, ENG-2, CORE-1, IPB-1, EXT-1, DUI-1, DUI-2, STR-2, STR-3, EXT-2, XC-16

### W2. One product configuration

`config/product.toml` becomes the only place product identity is written. Rust constants, a TypeScript module and every JSON manifest are generated from it, and CI fails when a generated file is out of date.

XC-1, XC-2, XC-3, XC-4, XC-5, XC-6, XC-7, IPB-2, IPB-3, DRS-6, DRS-7, DRS-14, DUI-24, EXT-14

### W3. Generated IPC contract and typed errors

Every type, command name, event name, limit and error code that crosses a process boundary is defined once in `crates/protocol` and generated for TypeScript. No layer decides anything from the wording of a message.

XC-8, XC-9, XC-10, XC-11, IPB-4, IPB-5, IPB-12, IPB-13, IPB-14, IPB-15, DRS-1, DRS-2, DRS-11, DUI-3, DUI-4, DUI-11, DUI-19, DUI-21, CORE-15, CORE-26, EXT-6, EXT-8, EXT-9

### W4. Security hardening

CORE-2, CORE-3, CORE-22, ENG-6, ENG-9, ENG-11, CMD-11, CMD-17, CMD-18, IPB-8, IPB-9, IPB-10, IPB-11, DRS-4, DRS-5, DRS-18, DRS-19, EXT-3, EXT-15

Planned inside W4 (maintainer, 2026-10-13): the Public Suffix List is kept up to date at run time, and the app owns it. The app bundles a snapshot and refreshes it weekly while idle from publicsuffix.org; settings show the date it was last updated and a "Check now" button, and a setting turns the refresh off, with a privacy note saying what the request tells publicsuffix.org. A downloaded list is validated (complete, within a size cap, parses, not suspiciously smaller than the current one) and saved atomically, and any failure keeps the current list. The app decides cookie scope and the extension passes the cookies the browser would send. D35 stays as the floor: the list bundled at a release is current when the release is made. The protocol gains the setting and the call, which is possible now that W3 is done, and its settings page belongs to the revamp ([REVAMP](REVAMP.md)).

### W5. Logging and diagnostics

XC-15, DRS-3, EXT-7, CMD-14

### W6. Shared test support and CI

One unpublished test-support crate replaces the copied local servers and helpers. Every suite that exists runs in CI.

XC-13, XC-18, XC-20, XC-21, STR-10, STR-11, STR-19, STR-20, STR-21, DRS-24, IPB-20, CORE-29, DUI-17, DUI-18, DUI-27, IPB-16

IPB-16 moved here from W2: a test-only bridge binary only helps once the overrides are chosen by the binary rather than by a package feature, and its variable names belong with XC-13's shared test support.

#### Proposed gate checks

Every check proposed during the audit run, so none lives only in a reviewer's notes. Each is cheap to run and would have caught a defect found by hand; none is built yet. Proposals already written in the carried lists below (fixed sleeps in tests, `Result<_, String>`, a bare `vi.waitFor`, the `RuleTester` check, `cargo doc` of private items, a log line that formats an error with `%` alone, and history words in comments, W15) are not repeated here.

- A `cargo check` of every commit in `origin/main..HEAD`, and generated files (protocol, product) current at every commit, so each commit compiles and bisects.
- A test run elevated on Windows: the endpoint key ownership bug (IPB-10) showed only when tests ran as administrator, as GitHub's Windows runners do.
- The desktop's `a11y.spec.ts` and `layout.spec.ts` Playwright specs in the gate (DUI-18); they found three real bugs in the main window rebuild.
- `e2e-gallery` on Linux CI: it needs a Chromium install and `PW_CHANNEL=chromium`.
- Control characters other than tab, carriage return and line feed rejected by the `text` check.
- `eslint-plugin-regexp` with `no-super-linear-backtracking`, for patterns that can backtrack catastrophically.
- Prettier over `docs/` and `docs/audit/`: `DECISIONS.md` and the audit reports fail `prettier --check` today and no lint script covers them.
- An exemption from pnpm's release-age rule written into `pnpm-workspace.yaml` is refused.
- The `exceptions` check reads the string literals in rule test fixtures (`packages/eslint-config/rules/*.test.js`) as real disable comments: skip string literals.
- Unsafe code: deny `unsafe_code` workspace-wide, and allow it only in listed wrapper modules, with `#![forbid(unsafe_code)]` in every module outside them.
- No `tracing_subscriber` setup (`fmt()`, `EnvFilter`) outside `crates/logging` and tests.
- No wall-clock assertion and no hand-installed log subscriber in a test (use the shared log capture in `crates/test-support`).
- `cargo test -p wirecatch-bridge` without `--features test-overrides` silently skips its integration test and passes: make the skip explicit or fail.
- The add-on tests run the `fake_media_tool` example from `<target>/<profile>/examples`, which a targeted `cargo test --test` does not rebuild, so a stale build fails the wrong tests. Make the tool a test-only bin target, so cargo builds it for the test.
- CI caches are actually saved (a cache hit on the second run) and runner disk use is watched, so the disk exhaustion that failed the Linux job (D33) is seen before it fails.
- `wirecatch/no-silent-catch` for every package, not the extension only (the desktop has 14 sites and protocol one to handle first).
- `no-magic-numbers` applied to object property values, which it skips today.

### W7. Engine

ENG-3, ENG-4, ENG-5, ENG-7, ENG-8, ENG-10, ENG-12, ENG-13, ENG-14, ENG-15, ENG-16, ENG-17, ENG-18, ENG-19, ENG-20, ENG-21, XC-14

XC-14 moved here from W2, engine part only (its default user agent `wirecatch-engine/<version>` and messages without the product name): the published engine cannot depend on the product configuration. Core's user agent is done in W2.

### W8. Media pipeline

One ffmpeg layer, one codec and container policy and one list of media extensions, shared by core and streams.

CMD-1, CMD-2, CMD-3, CMD-4, CMD-5, CMD-6, CMD-7, CMD-8, CMD-9, CMD-10, CMD-12, CMD-13, CMD-15, CMD-16, CMD-19, CMD-20, CMD-21, CMD-22, CMD-23, STR-4, STR-5, STR-6, STR-7, STR-8, STR-9, STR-12, STR-13, STR-14, STR-15, STR-16, STR-17, STR-18, STR-22

### W9. Core manager

CORE-4, CORE-5, CORE-6, CORE-7, CORE-8, CORE-9, CORE-10, CORE-11, CORE-12, CORE-13, CORE-14, CORE-16, CORE-17, CORE-18, CORE-19, CORE-20, CORE-23, CORE-24, CORE-25, CORE-27, CORE-28

### W10. IPC, bridge and CLI

IPB-6, IPB-7, IPB-17, IPB-18, IPB-19, CORE-21

### W11. Desktop shell

DRS-8, DRS-9, DRS-10, DRS-12, DRS-13, DRS-15, DRS-16, DRS-17, DRS-20, DRS-21, DRS-22, DRS-23

### W12. Desktop interface

DUI-5, DUI-6, DUI-7, DUI-8, DUI-9, DUI-10, DUI-12, DUI-13, DUI-14, DUI-15, DUI-16, DUI-20, DUI-22, DUI-23, DUI-25, DUI-26, XC-12

The revamp ([REVAMP](REVAMP.md)) replaces the interface the component, layout and focus findings were written against, so it absorbs them. What stays W12's: DUI-8 and DUI-16, DUI-3, DUI-5, DUI-6, DUI-12, DUI-14, DUI-25 and XC-12 (state, backend, tests and tokens, which a redesign does not remove), and DUI-9 and DUI-10, which stay as requirements the new components are reviewed against.

### W13. Extension

EXT-4, EXT-5, EXT-10, EXT-11, EXT-12, EXT-13, EXT-16

### W14. Zero lint warnings

The W0 baseline found 1,477 clippy warnings and 787 ESLint errors. Each package is brought to zero while its workstream is open, and the gates then hold it there.

Done on 2026-10-12: every crate follows the workspace lints, both apps' suppression files are deleted, and `std::thread::sleep` is banned in `clippy.toml`. 2,204 violations were fixed by cause, not silenced. The exceptions left are these eleven, each a single attribute or comment with its reason in place. `LINT_EXCEPTIONS` in `scripts/gate/exceptions.mjs` lists the same eleven by file and lint, and the `exceptions` check fails on any other `#[expect]`, `#[allow]`, ESLint disable comment or `svelte-ignore`, and on any of these that is gone:

- E1: `apps/desktop/src/lib/backend/mock/rng.ts`, `wirecatch/no-magic-numbers` (one block, switched back on after it): the constants of the published mulberry32 generator, which define the algorithm.
- E2: `crates/protocol/src/wire.rs`, `clippy::large_enum_variant` on `Message`: messages are decoded, handled and dropped one at a time.
- E3: `crates/engine/src/transport.rs`, `clippy::large_enum_variant` on `Remote`: one value per download, shared by its connections.
- E4: `crates/protocol/src/app.rs`, `clippy::struct_excessive_bools` on `AppSettings`: each bool is an independent setting.
- E5: `crates/e2e/src/lib.rs`, crate level, `clippy::expect_used` and `clippy::missing_panics_doc`: test support, where a panic is the failing test's report.
- E6: `crates/test-support/src/lib.rs`, crate level, `clippy::unwrap_used` and `clippy::missing_panics_doc`: the same.
- E7: `crates/engine/src/numeric.rs`, `clippy::cast_precision_loss` on `approx_f64`: exact below 2^53, which byte counts and rates stay far below.
- E8: `crates/engine/src/numeric.rs`, `clippy::cast_possible_truncation` and `clippy::cast_sign_loss` on `f64_to_u64`: the value is checked to be finite, not negative and in range first.
- E9: `apps/desktop/src/lib/window/FileQuestion.svelte`, `a11y_no_noninteractive_element_interactions` on the question's form: Enter in any field, radio buttons and checkboxes included, is Download now (`keys.ts`); browsers do not submit those by themselves, so the key is taken on the form.
- E10: `apps/desktop/src/lib/window/MediaQuestion.svelte`, the same, on the quality question's form.
- E11: `apps/desktop/src/lib/window/ProgressView.svelte`, `a11y_no_static_element_interactions` on the progress window's root: it watches pointer and focus inside the window so a finished one does not close under the person, and takes Escape for its inline questions first; the controls inside are the interactive elements.
- E12: `crates/core/src/addons/manifest.rs`, `clippy::expect_used` on `pinned_address`, which parses the official manifest's addresses: it is given only addresses put together from that file's constants, every one of which `the_official_sources_are_pinned_and_consistent` parses on each system it builds for. `url::Url` has no infallible constructor, and `MediaAddonSources::official` has no error to return (CMD-11, D38.1).

The lead's decisions on the way: `wirecatch/no-magic-numbers` lets named module constants be written as they read; a shared, unpublished `crates/test-support` holds every test server (D26); engine locks recover from poisoning through one accessor per struct; Rust-owned facts the apps need are generated into `@wirecatch/protocol`; `@wirecatch/units` holds byte units for both apps; icon sizes and dialog widths are a scale of tokens in `app.css` (approved by the maintainer). New gate rules keep the result: `wirecatch/no-assertion-on-input` (no `as`, `<T>` or `!` on a value whose type says nothing about its shape, such as `unknown` or `object`, every package), `wirecatch/use-ui-primitives` (in the desktop app, no element outside `Button` and `IconButton` carries their `btn` or `icon-btn` class, and no Lucide icon is rendered other than through `Icon`, following names, runes, members, choices, objects, arrays and `{#each}` items within a file; left to review are icons and classes passed in as props or returned by functions, class helpers such as `clsx`, spread attributes and class names built in template strings) and the `std::thread::sleep` ban; after the workstream review, gate checks for `allow` inside `cfg_attr` (`cfg-allow`), Linux lints from Windows and macOS for the crates that build no native code (`clippy-linux`), a reason on every disable comment in scripts and in Svelte markup and no ESLint configuration in comments, the exception list (`exceptions`), the effective lint configuration of every package (`js-policy`), and no value of a backend type in desktop components and windows by the paths `wirecatch/no-value-of-type` follows (each and snippet bindings and look-alike types are left to review). The desktop tests share one test window per worker (`isolate: false`), guarded by a reset after every file and a ban on module mocks, which made them pass reliably under load.

Done on 2026-10-12 as its own round: all 72 `let _ =` outside test code (and the few in tests) were decided one by one, handled, propagated, logged with what failed and what it means, or passed to a named function where failure cannot matter (ENGINEERING section 4), and `clippy::let_underscore_must_use` is on for every crate. Two bugs it found are fixed with tests: `wirecatch-bridge --help` reported success when the usage could not be printed, and a data folder copy cut short kept the final name, so the next start opened it and ignored the real data. Clippy cannot see `.ok();` or `drop(result)`, so those stay a review rule.

### W15. Comments only where they earn their place

Added by the maintainer on 2026-10-12, to run after every other workstream. Most files carry comments that restate the code, narrate history or repeat what a commit message says; they add lines and cost every review its reading time. Remove them across every crate and package, keeping the comments that carry what the code cannot: critical paths, decisions and invariants that are not obvious, `SAFETY:` proofs. Then review the rules that ask for comments everywhere: a doc comment on every named constant (ENGINEERING section 2) and `missing_docs` on every public item (section 4), which stays for the published engine API and is reconsidered for internal crates. Propose a gate check, for example one that flags history words such as "now", "no longer", "used to" and "previously" in comments.

## Status

| Workstream | Status |
|---|---|
| W0 | done and independently reviewed; the review's 20 defects are fixed. What is left of its findings moved to later workstreams, listed below |
| W1 | done and independently reviewed: all 12 fixed, each with a test shown to fail without its fix, and the review's 29 defects (2 high, 7 medium, 20 low) fixed the same way. Also done alongside: per-machine install (D25). What is left of the review moved to later workstreams, listed below |
| W14 | done (2026-10-12): every crate and package on the lint rules with the eleven exceptions listed above, reviewed as a whole by four reviewers whose findings were fixed and verified, full gate passing. What is left moved to later workstreams, listed below |
| W2 | done (2026-10-12) and independently reviewed in three rounds, whose 36 defects were fixed with tests: `config/product.toml` is the only place product identity is written, generated into `wirecatch-product`, `@wirecatch/product` and every manifest (D29); the `product` check fails on a stale generated file and the `publish` group fails while a placeholder is left. XC-14 (engine part) moved to W7 and IPB-16 to W6. What is left moved to later workstreams, listed below |
| W7 | done (2026-10-12) and independently reviewed in four rounds, whose defects were fixed with tests, each test shown to fail without its fix: every ENG finding of W7 and the engine half of XC-14, plus `NO_PROXY` for HTTP/3 and FTP, data flushed before a resume file counts it (D30), and no file touched after a download is dropped. The published API changed on purpose (D30, commits marked breaking). What is left moved to later workstreams, listed below |
| W8 | done (2026-10-13) and independently reviewed in four rounds (round 1 by two verifiers, then a streams and a core reviewer), whose defects were fixed with tests, each test shown to fail without its fix: one media layer in `wirecatch-streams` shared by core and streams (D31), with the codec and container policy (D24), the one list of media extensions, the ffmpeg argument layer and the only runner of the add-on's programs (process groups on Unix, job objects on Windows, time limits, and an idle watchdog that also watches the result being written); HLS and DASH readers bounded against hostile playlists (memory measured with a counting allocator, reading linear in the manifest) with typed errors; every media error worded in `media::describe`; and the gate's `docs` check. What is left moved to later workstreams, listed below |
| W3 | done (2026-10-13) and independently reviewed in three rounds by two verifiers (the Rust contract and the TypeScript side), after reviews of its versioning and desktop units, whose defects were fixed with tests, each test shown to fail without its fix: every type, command and event name, limit, default and error code that crosses a process boundary is declared once in `crates/protocol` and generated for TypeScript, with a generated validator for every type and branded ids; the desktop's 50 commands are one contract the shell and the interface are held to; errors cross a boundary as typed codes, never wording; one frame header reader; `hello` agrees on a protocol version from a range, a call one end lacks gets `unsupported_call`, and version 1 is frozen against snapshots of what it shipped (D32); one rule for ignored sites and for file types, in Rust with a TypeScript port tested against fixtures Rust writes, and site lists bounded before the manager's lock; and the lint rules `wirecatch/no-brand-assertion` and `wirecatch/no-type-argument-assertion`. D37 records what the contract carries beyond types. What is left moved to later workstreams, listed below |
| W4 | this round. In progress on `ws/w4-security`: the engine's checks of answers and starting hosts (D34), the extension's cookie scope and Public Suffix List (D35), the media add-on's trust (D38), the private-network guard for captured links (D39) and the local endpoint, secrets and one manager per list (D41: IPB-8, IPB-9, IPB-11, and IPB-10's endpoint key, built but not switched on) are in, each reviewed. Left: ENG-11, DRS-4, DRS-5, DRS-18, DRS-19, switching on the endpoint key (IPB-10) once W5 has landed, and the suffix list's update at run time (planned above); then a review of the whole workstream, regrouping and landing |
| W5 | done (2026-10-13) and independently reviewed: the Rust part in two rounds, the extension's in four, and both together in a milestone review, whose defects were fixed with tests, each test shown to fail without its fix: XC-15, DRS-3, CMD-14 and EXT-7. Every binary logs through `wirecatch-logging` to stderr and a size-capped file in the log folder, chooses lines with `WIRECATCH_LOG`, and hides secrets in every line (D40); what ffmpeg and yt-dlp write to stderr is kept bounded and redacted, logged when a run fails or is stopped, and cut to 300 characters in a person's message. Every error the extension catches is logged through `src/lib/diag`, used, or explained in place, held by the lint rule `wirecatch/no-silent-catch`; entries are redacted (sites only, no paths, e-mail addresses or secrets, found by name and by shape), repeats are counted instead of added, entries made while a private window is open are forgotten when the last one closes, and the background and the popup keep theirs in `storage.session`. What a secret is (its names, the sign-in headers and the auth scheme words) is declared once in `crates/protocol`, and both redactors are held to one corpus Rust writes (D40.10). What is left moved to later workstreams, listed below |
| W6 | later, except what is done. In part done on main: the load-flaky tests (27eb0f83). The rest is open, with the proposed gate checks under W6 |
| W9 | later, after W4 is on main |
| W10 | later |
| W11 | later |
| W12 | later. Most of it folds into the interface revamp; the findings that survive a redesign are listed under W12 |
| W13 | later |
| Interface revamp ([REVAMP](REVAMP.md)) | not an audit workstream. Phase 1 this round: main window, add-download flow, download window, extension popup and options. Phase 2 later: settings pages, add-ons, dialogs, command palette, in-page media button, tray, installer, and the additions from other workstreams |
| W15 | later, last, after W13 (maintainer, 2026-10-12); the rule for new comments is in ENGINEERING section 8 from now on |

Parts of W5 carried into later workstreams:

- EXT-7: the popup's "Copy diagnostics" action comes with the popup's redesign: it copies `readDiagnostics(browser.storage.session)` with the connection status, the browser and the extension version. Until then the buffer is read only from the browser's developer tools (W13).
- EXT-7: content scripts keep their entries in memory only, where nothing reads them; they should send `warn` and `error` entries to the background, which validates them and logs them under the `page` scope (W13).
- `wirecatch/no-silent-catch` is on for the extension only; the shared baseline takes it once the desktop's 14 sites and protocol's one are handled (W12, with the desktop's logging). The gate checks W5 proposed are listed under W6.
- Review rule, proposed: a decode error of a message from another process (`crates/core/src/server.rs`, where `serde_json` quotes the offending value) is logged without its text, since no redaction can tell a secret in such a value (W9).

Parts of W0 findings carried into later workstreams:

- XC-23: every workspace package is linted and format-checked (`packages/eslint-config` lints itself, its rules without type information; `packages/protocol` lints its scripts and index and type-checks the generated types), and the `js-policy` check keeps it so. `scripts/` is linted and format-checked by the root `lint` script with the same configuration, now that the gate is split into modules in `scripts/gate/` that fit the size limits. Done.
- XC-25: `windows-sys` 0.59 and 0.61 both compile until the Win32 code moves to maintained wrappers (W10).
- XC-29: installers and the extension do not yet ship a third-party notices file (W6, with the release workflow of XC-20).
- XC-30: `authors`, `repository` and `homepage` come from `config/product.toml`, and the `product` check requires every crate to inherit them. Done (W2).
- XC-32: `SECURITY.md` is generated from `config/product.toml` with its security contact address, a placeholder until it is decided. Done (W2).

Parts of W14 carried into later workstreams:

- The desktop bridge test's Chromium origin comes from `wirecatch_product::CHROMIUM_EXTENSION_ORIGIN`, and `config/extension-ids.json` is gone. Done (W2).
- `PrivateDir::remove` (`crates/test-support/src/lib.rs:133`) says it catches a handle left open, but only a file opened without delete sharing blocks removal, and std opens files with it, so it cannot prove the engine closed its files: say so in its docs, or check open handles another way (W6).
- Flaky under load, each seen once: `a_capture_starts_after_the_server_answers_and_completes` (`crates/e2e/tests/e2e.rs:46`, a `.wcpart` still listed after the row turned Completed; not reproduced in nine runs of the e2e binary under 16 CPU burners, so its cause is still open), and the media tests in `apps/desktop/e2e/a11y.spec.ts` (90 s waiting for the sample download window, also on the base branch) (W6).
- Fixed sleeps still in tests, which ENGINEERING section 7 now rules out: `crates/core/tests/manager` (`addons.rs`, `media.rs`, `protocol.rs`, `schedule.rs`, and the wall-clock bounds in `limits.rs`), the rest of STR-11 (`speed_limits_apply_to_streams`' upper bound, the e2e latency bounds). Proposed check: `tokio::time::sleep` in a test file outside a named helper (W6).
- `crates/engine/tests/request_limit.rs` `one_connection_sends_its_requests_one_after_another` asks only that the requests reuse a TCP connection at least once, not that all of them share one: hyper-util's pool takes an HTTP/1.1 connection back in a task it spawns once the connection is ready for another request, and the next request's checkout races it. A change that opens a connection per request still fails it; one that loses reuse for some requests does not. Nothing in reqwest lets the engine wait for the pool, so covering every request needs a pool event to wait on or a server-side count with a bound proven under load (W6).
- The real-ffmpeg tests forward the installed fake ffmpeg to the real one instead of installing a 228 MB copy within `PATIENCE`, so no test extracts a full-size binary and runs that installed copy; the archive's size cap has unit tests. If that matters, a CI-only check installs the real archive once, outside the `PATIENCE`-bound tests (W6).
- The media and add-on tests in `crates/core/tests/manager` run the `fake_media_tool` example from `<target>/<profile>/examples` (`media_support.rs`), which only `cargo test -p wirecatch-core` without `--test` (or the gate's workspace run) rebuilds. A targeted run such as `cargo test -p wirecatch-core --test manager addons::` silently uses whatever build is there: in W4, after the add-on trust work changed the tool, 10 add-on update tests failed against a stale build and passed after `cargo build -p wirecatch-core --examples`. The tests should get a build that matches their own source: make the tool a test-only bin target so `CARGO_BIN_EXE_*` names a binary cargo builds for the test, or have the tests build it (or check its age against its source) before use (W6).
- Gate checks: byte sizes written as `n * 1024` or `1 << 20` in crates that depend on the engine, outside protocol, e2e, test-support and bridge (none today; two commits claimed every size converted without one); unused TypeScript exports; a `DownloadOptions::default()` in core outside `headers.rs`, which would send the engine's user agent instead of the app's (W6).
- The HLS and DASH parsers return typed `thiserror` errors. Done (W8).
- Win32 helpers are copied: `win32_size_of` (`crates/ipc/src/windows.rs:51`, `crates/bridge/src/shell_parent.rs:89`), `os_error` (`crates/ipc/src/windows.rs:62`, `crates/bridge/src/install.rs:741`), a fourth `wide` (`crates/core/src/owner_only.rs:163`) and two current-user SID readers (`crates/core/src/owner_only.rs:124`, `crates/ipc/src/windows.rs:196`); add them to IPB-6, and the registry loop's literal 520 (`crates/bridge/src/install.rs:759`) to IPB-17 (W10).
- Desktop lint rules, each clean today: no type argument on `querySelector` or `closest` and no `as` on DOM values (unchecked casts), no `setTimeout` in a `$effect` without its cleanup, no `vi.useRealTimers()` or timer spies outside `vitest-setup.ts`, and no import of `window/**` from `backend/**` (W12).
- Extension lint rule: type predicates (`x is T`) only in `src/lib/validate.ts`, so only its combinators declare guards; two are outside it today (`src/lib/file-types.ts:54`, `src/lib/media/detect.ts:255`) (W13).

Parts of W2 carried into later workstreams:

- The product name inside prose (settings descriptions, help text, error and notice sentences, `format.ts` messages, extension notices and the extension's command description) stays literal until a translation pass gives copy a home; places that name the running app use its name already (D29.8) (W12 for the desktop, W13 for the extension).
- EXT-14's remaining part: extension storage keys, port, menu and message ids, colours, `MEDIA_COMMAND`, `DEFAULT_QUEUE_ID` and `PAGE_SITES` are internal identifiers, not product identity (D29.4) (W13). `DEFAULT_QUEUE_ID` comes from the protocol, and the media site list stays in the extension (D37.4). Done (W3).
- IPB-16: a test-only bridge binary that never overwrites `wirecatch-bridge`, with its override variable names in one place (W6, with XC-13).
- The release workflow (XC-20) runs `node scripts/gate.mjs publish` before it publishes anything; today the release scripts run it themselves (W6).
- The engine's version follows the workspace (product) version; whether the published engine needs a version of its own is undecided (W7).
- XC-14: after the engine's default user agent changes, `a_download_without_a_browser_agent_names_the_app` (`crates/core/tests/manager/lifecycle.rs`) should also assert the header differs from `DownloadOptions::default().user_agent`, and the add-on and media paths (`manager/addons.rs`, `manager/media.rs`) need the same check (W7).
- `Lock::hold` in `crates/core/tests/manager/addons.rs` blocks deletion with a 0555 folder, which root ignores, so `a_failed_removal_leaves_a_consistent_state` and `a_half_finished_update_is_recorded_and_repaired` fail when tests run as root, as in containers (W6).

Parts of the W1 review carried into later workstreams:

- CORE-1: the start-up notice (a damaged list set aside, downloads hidden) reaches the person only as a system notification, which is lost when notifications are off; the main window should show it too (W12).
- EXT-1: with an empty display cache, the first browser download is judged against capture-everything, so it may start the app only for the app to decline it; the extension needs the app's default settings, generated from `crates/protocol`. Done (W3): it judges against `DEFAULT_EXTENSION_SETTINGS`, and a core test holds the app's defaults equal to it.
- DUI-2: applying an `ExtensionSettingsPatch` is written twice in TypeScript, in the extension (`lib/protocol.ts`) and the desktop mock (`state/settings-changes.svelte.ts`); it belongs once in `@wirecatch/protocol`. Done (W3): `applyExtensionSettingsPatch` in `packages/protocol/src/extension-settings.ts`, tested against fixtures the Rust `ExtensionSettings::apply` writes.
- EXT-2: the media content script is injected into every frame even with the button off, where it does one storage read and nothing else; registering it only while the setting is on needs the `scripting` permission (W13).
- ENG-1: `bench_local` varies by about 40 percent between runs on the same machine, too much to show a regression; it should repeat each case and report the median (W7).

Parts of W7 carried into later workstreams:

- The engine's integration tests are not hermetic to `ALL_PROXY`: the public `download()` always reads the system's proxy settings, so on a machine with a proxy set the tests depend on it (the in-crate tests already use clients without proxies). Either the shared test support clears `*_PROXY` and `NO_PROXY` for each test process, or the engine gains a proxy option; a gate idea is to run the engine's tests under a dead `ALL_PROXY` (W6).
- A dropped download's queued file work checks that it was abandoned before it starts and again after each step that can be slow (creating the folder, each flush to the disk, waiting for the last checkpoint), but a moment remains between each last check and the change right after it: deleting the old part and resume files and creating the part file, writing the resume file, sizing and renaming the finished file and deleting its resume file. An owner that aborts a run which overran `stop_timeout` (`crates/core/src/manager/mod.rs`) and then deletes its files, or starts a new run with the same part file, can still race them. Closing them needs a drain handle: queued closures hold a clone, and the owner awaits the handle after the abort before touching the files (W9).
- A check that no log line formats an error that has a source with `%` alone (`error = %e`), which drops the reason below it, unless the error's type is listed as one whose message already includes its source (such as `rustls_native_certs::Error`); the engine logs the others through `error::Chain` (proposed check, W6).
- ENG-4: a debug assertion that file system calls run on the blocking pool, so a new inline call fails a test (proposed check, W6).
- `cargo doc --document-private-items` with `-D warnings` for the engine, and compiling the README's example as a doctest (proposed checks, W6).
- A check for history words ("used to", "no longer", "previously") in Rust comments (W15).
- ENG-6: the comparison of each partial answer's validators belongs in `http::check_partial`, used by HTTP/1.1, HTTP/2, HTTP/3 and the Fetcher (W4).
- ENG-9: the address classifier core needs is `net_guard::is_private`, ready to be made public (W4).
- ENG-11: `naming::unique_path` stays public only because `crates/streams` uses it; it goes private, or becomes a reserving API, with the name reservation (D30.6) (W4).
- XC-14 is done: the engine names itself (W7) and core sets the app's own user agent from `wirecatch-product` (W2), checked on the download, add-on and media paths.

Parts of W8 carried into later workstreams:

- STR-17: a stream is joined without first checking that the disk has room for the joined file; that needs a free-space helper in the engine or core (W7 follow-up, or W9).
- CMD-12: the VA-API render node is assumed to be `/dev/dri/renderD128` instead of being discovered (W9); `apps/desktop/src-tauri/src/bridge.rs` defines `CREATE_NO_WINDOW` by hand instead of taking it from windows-sys (W10); the YouTube host list is also in `apps/extension/src/lib/media/page-sites.ts` (W13).
- CMD-16: the add-on slot's wording when its record cannot be read, if the interface needs one of its own (W9).
- CMD-8: the manager's add-on install errors are still `String` (W9).
- CMD-13: a third `real_ffmpeg` helper in `crates/core/tests/manager/media_support.rs` (W6).
- `crates/core/src/manager/media_run.rs` is about 1530 lines (about 1250 before its tests) and wants splitting along its seams (W9). With it, the messages the page run still words itself (`refresh`, `refused_after_refreshing`, `network_rule`, the internal failure in `finish`, and `addon_missing` in `manager/media.rs`) move into `media::describe`, where D31 puts every media message (W9).
- Flaky on Windows, seen once in W8's runs and once in a W4 gate run, passing alone (five times in a row after the W4 run): `settle::a_part_file_left_by_a_crash_is_picked_up_again` (`crates/core/tests/manager/settle.rs`, "partial files left behind" after the download completed). Not reproduced in ten runs of the `settle` tests and six of the whole `manager` binary under 16 CPU burners; its assertion now names the file left, for the next time (W6).
- On Linux, yt-dlp and ffmpeg keep running after the app crashes: their process group is killed only by a live app. `PR_SET_PDEATHSIG` is not the answer: it fires when the thread that started the program exits, and the runtime's threads come and go. It needs another mechanism, such as a small supervisor process (W4).
- Proposed gate checks: `Result<_, String>` in library crates; tests that match on an error's text; and the unsafe-code check listed under Proposed gate checks in W6 (W6).
- CMD-19: yt-dlp's failures are still told apart by the text of its error line, since yt-dlp has no error codes; a new yt-dlp wording falls back to the plain refusal (W9, if yt-dlp ever gives codes).
- Memory tests like `crates/streams/tests/manifest_memory.rs` (a counting allocator over `inspect`) for HLS master and media playlists, and one for DASH's worst case by design, 4,096 representations each with its own long base address (up to 64 MiB, bounded by `MAX_REPRESENTATIONS`) (W6).
- roxmltree's parsed form takes about twenty bytes per byte of a manifest of small elements (a 1 MB timeline of 100,000 entries held 23 MB), so an 8 MiB DASH manifest can hold close to 200 MB while it is read; a lower DASH size limit or a streaming reader would cut it (W6).
- `a_page_described_at_a_length_past_the_limit_is_refused_as_such` writes 64 MiB per run; a core unit test of how `extract_once` treats a cut-short output would replace it, once `process::Ran` can be built outside streams for tests (W6).

Parts of W3 carried into later workstreams:

- CORE-26's `UnixMillis`: a `UnixMillis(i64)` newtype in `crates/protocol` with `checked_add(Duration)` and `since`, used for the wire's Unix millisecond times (`added_at`, `completed_at`, `retry_at`, `exported_at`) and through core's clock arithmetic, with `CHECK_EVERY_MILLIS` and `DAY_MILLIS` (`crates/core/src/manager/addons.rs`, the same value twice) as one `Duration`. Its value is in core, which W9 owns, and its TypeScript side is a branded type in the desktop interface (W9).
- The release workflow checks that the protocol version it ships is frozen, its generated types copied into `crates/protocol/released` and named in `RELEASED` (`crates/protocol/src/version.rs`, D32.8), before it publishes (W6, with XC-20).
- EXT-14's longer-term step: the app sends the media add-on's site list so the extension's offer tracks what is installed. It is a protocol feature for a later version, not a quality fix; D37.4 says how it is added (ROADMAP). It is the first call newer than version 1, and its caller in the extension is the first user of `NativeClient.supports` (D32.3), which keeps the offer to the extension's own list with an app that lacks the call.
- Lint gaps the final TypeScript review left open, each clean in the code today: `wirecatch/no-type-argument-assertion` checks a callback's annotated parameters only when the callback is written in the call, not when it is passed by name (judge the argument by its type instead); `wirecatch/no-brand-assertion` does not look into the properties of an object type written in place or of a mapped type, so `v is { data: Queue }` and `v is Pick<Queue, 'id'>` pass; and the declarers `no-type-argument-assertion` exempts (`$state`, `createContext`, `writable`) are matched by the callee's text, not its import (W6).
- A lint rule reporting `Number(event.currentTarget.value)` or `Number(event.target.value)` on a `<select>`, which the assertion rules cannot see and which read unchecked menu values before W3 (W12).
- `packages/protocol`'s Vitest runs in Node only, while the extension runs `normalizeSite` on Chromium's and Firefox's own host parsers: run its fixtures in browser mode on both (W6).
- `fileType` in `apps/desktop/src/lib/filetypes.ts` has no fixtures from Rust. Whether a file opens is decided in Rust; the TypeScript copy only words the dialog and drives the stand-in backend, and moving it into the protocol needs a platform parameter, since trailing dots are trimmed only on Windows. The redesign's file type editor calls `normalizeFileType` (W12).
- The product name in the extension's prose, about 60 places in `apps/extension/src/lib`; `versions.ts` uses `PRODUCT_NAME` (W13, with EXT-14).
- `vi.waitFor` gives up after one second unless told otherwise, which races a busy machine against a time limit as ENGINEERING section 7 rules out; the desktop and extension tests call it bare 13 times. Proposed: one shared wait in test setup bounded by `PATIENCE`'s two minutes, and a lint rule against a bare `vi.waitFor` (W6).
- A check that the typed rule tests build their `RuleTester` through `typedRuleTester()`, or the `packages/eslint-config` tests run with `CI=true` on every machine, so local runs see what CI sees (W6).
