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

XC-1, XC-2, XC-3, XC-4, XC-5, XC-6, XC-7, XC-14, IPB-2, IPB-3, IPB-16, DRS-6, DRS-7, DRS-14, DUI-24, EXT-14

### W3. Generated IPC contract and typed errors

Every type, command name, event name, limit and error code that crosses a process boundary is defined once in `crates/protocol` and generated for TypeScript. No layer decides anything from the wording of a message.

XC-8, XC-9, XC-10, XC-11, IPB-4, IPB-5, IPB-12, IPB-13, IPB-14, IPB-15, DRS-1, DRS-2, DRS-11, DUI-3, DUI-4, DUI-11, DUI-19, DUI-21, CORE-15, CORE-26, EXT-6, EXT-8, EXT-9

### W4. Security hardening

CORE-2, CORE-3, CORE-22, ENG-6, ENG-9, ENG-11, CMD-11, CMD-17, CMD-18, IPB-8, IPB-9, IPB-10, IPB-11, DRS-4, DRS-5, DRS-18, DRS-19, EXT-3, EXT-15

### W5. Logging and diagnostics

XC-15, DRS-3, EXT-7, CMD-14

### W6. Shared test support and CI

One unpublished test-support crate replaces the copied local servers and helpers. Every suite that exists runs in CI.

XC-13, XC-18, XC-20, XC-21, STR-10, STR-11, STR-19, STR-20, STR-21, DRS-24, IPB-20, CORE-29, DUI-17, DUI-18, DUI-27

### W7. Engine

ENG-3, ENG-4, ENG-5, ENG-7, ENG-8, ENG-10, ENG-12, ENG-13, ENG-14, ENG-15, ENG-16, ENG-17, ENG-18, ENG-19, ENG-20, ENG-21

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

### W13. Extension

EXT-4, EXT-5, EXT-10, EXT-11, EXT-12, EXT-13, EXT-16

### W14. Zero lint warnings

The W0 baseline found 1,477 clippy warnings and 787 ESLint errors. Each package is brought to zero while its workstream is open, and the gates then hold it there.

Done on 2026-10-12: every crate follows the workspace lints, both apps' suppression files are deleted, and `std::thread::sleep` is banned in `clippy.toml`. 2,204 violations were fixed by cause, not silenced. The exceptions left are these eight, each a single attribute or comment with its reason in place. `LINT_EXCEPTIONS` in `scripts/gate/exceptions.mjs` lists the same eight by file and lint, and the `exceptions` check fails on any other `#[expect]`, `#[allow]`, ESLint disable comment or `svelte-ignore`, and on any of these that is gone:

- E1: `apps/desktop/src/lib/backend/mock/rng.ts`, `wirecatch/no-magic-numbers` (one block, switched back on after it): the constants of the published mulberry32 generator, which define the algorithm.
- E2: `crates/protocol/src/wire.rs`, `clippy::large_enum_variant` on `Message`: messages are decoded, handled and dropped one at a time.
- E3: `crates/engine/src/transport.rs`, `clippy::large_enum_variant` on `Remote`: one value per download, shared by its connections.
- E4: `crates/protocol/src/app.rs`, `clippy::struct_excessive_bools` on `AppSettings`: each bool is an independent setting.
- E5: `crates/e2e/src/lib.rs`, crate level, `clippy::expect_used` and `clippy::missing_panics_doc`: test support, where a panic is the failing test's report.
- E6: `crates/test-support/src/lib.rs`, crate level, `clippy::unwrap_used` and `clippy::missing_panics_doc`: the same.
- E7: `crates/engine/src/numeric.rs`, `clippy::cast_precision_loss` on `approx_f64`: exact below 2^53, which byte counts and rates stay far below.
- E8: `crates/engine/src/numeric.rs`, `clippy::cast_possible_truncation` and `clippy::cast_sign_loss` on `f64_to_u64`: the value is checked to be finite, not negative and in range first.

The lead's decisions on the way: `wirecatch/no-magic-numbers` lets named module constants be written as they read; a shared, unpublished `crates/test-support` holds every test server (D26); engine locks recover from poisoning through one accessor per struct; Rust-owned facts the apps need are generated into `@wirecatch/protocol`; `@wirecatch/units` holds byte units for both apps; icon sizes and dialog widths are a scale of tokens in `app.css` (approved by the maintainer). New gate rules keep the result: `wirecatch/no-assertion-on-input` (no `as`, `<T>` or `!` on a value whose type says nothing about its shape, such as `unknown` or `object`, every package), `wirecatch/use-ui-primitives` (in the desktop app, no element outside `Button` and `IconButton` carries their `btn` or `icon-btn` class, and no Lucide icon is rendered other than through `Icon` by any path the rule can follow within a file; an icon passed in as a prop is left to review) and the `std::thread::sleep` ban; after the workstream review, gate checks for `allow` inside `cfg_attr` (`cfg-allow`), Linux lints from Windows and macOS for the crates that build no native code (`clippy-linux`), a reason on every disable comment in scripts and in Svelte markup and no ESLint configuration in comments, the exception list (`exceptions`), the effective lint configuration of every package (`js-policy`), and no value of a backend type in desktop components and windows (`wirecatch/no-value-of-type`). The desktop tests share one test window per worker (`isolate: false`), guarded by a reset after every file and a ban on module mocks, which made them pass reliably under load.

Planned as the next round, not enabled yet: `clippy::let_underscore_must_use`, so a `Result` or other `#[must_use]` value is never dropped with `let _ =` (section 4 of ENGINEERING.md says so today, enforced by review only). It is its own round because each existing `let _ =` needs a decision: handle, propagate or log with a reason.

## Status

| Workstream | Status |
|---|---|
| W0 | done and independently reviewed; the review's 20 defects are fixed. What is left of its findings moved to later workstreams, listed below |
| W1 | done and independently reviewed: all 12 fixed, each with a test shown to fail without its fix, and the review's 29 defects (2 high, 7 medium, 20 low) fixed the same way. Also done alongside: per-machine install (D25). What is left of the review moved to later workstreams, listed below |
| W14 | fixed (2026-10-12): every crate on the workspace lints, no suppression files, `thread::sleep` banned; each part was verified by an agent that did not write it. Review of the whole workstream running |
| W2 to W13 | not started |

Parts of W0 findings carried into later workstreams:

- XC-23: every workspace package is linted and format-checked (`packages/eslint-config` lints itself, its rules without type information; `packages/protocol` lints its scripts and index and type-checks the generated types), and the `js-policy` check keeps it so. `scripts/` is linted and format-checked by the root `lint` script with the same configuration, now that the gate is split into modules in `scripts/gate/` that fit the size limits. Done.
- XC-25: `windows-sys` 0.59 and 0.61 both compile until the Win32 code moves to maintained wrappers (W10).
- XC-29: installers and the extension do not yet ship a third-party notices file (W6, with the release workflow of XC-20).
- XC-30: `authors`, `repository` and `homepage` come from `config/product.toml` (W2).
- XC-32: `SECURITY.md` needs a security contact address, a product identity value (W2).

Parts of the W1 review carried into later workstreams:

- CORE-1: the start-up notice (a damaged list set aside, downloads hidden) reaches the person only as a system notification, which is lost when notifications are off; the main window should show it too (W12).
- EXT-1: with an empty display cache, the first browser download is judged against capture-everything, so it may start the app only for the app to decline it; the extension needs the app's default settings, generated from `crates/protocol` (W3).
- DUI-2: applying an `ExtensionSettingsPatch` is written twice in TypeScript, in the extension (`lib/protocol.ts`) and the desktop mock (`state/settings-changes.svelte.ts`); it belongs once in `@wirecatch/protocol` (W3).
- EXT-2: the media content script is injected into every frame even with the button off, where it does one storage read and nothing else; registering it only while the setting is on needs the `scripting` permission (W13).
- ENG-1: `bench_local` varies by about 40 percent between runs on the same machine, too much to show a regression; it should repeat each case and report the median (W7).
