# Wirecatch

Open-source (GPL-3.0-only) download manager for Windows and Linux, built to ship commercially: Rust engine and core, Tauri 2 + Svelte 5 desktop app, WXT + Svelte 5 browser extension.

The bar is code a principal engineer would hold up as an example. Passing the gates is the minimum, not the goal.

## Read first

- @docs/ENGINEERING.md: the engineering standard. Every rule there applies to every change.
- `docs/DECISIONS.md`: why things are the way they are. Do not reverse a decision without recording a new one.
- `docs/AUDIT.md`: the open quality workstreams. Work in the order it gives, and update its status table when a workstream finishes.
- `docs/PLAN.md` and `docs/ROADMAP.md`: work packages and what comes next.

## Layout

| Path | What it is |
|---|---|
| `crates/engine` | Download engine library, published to crates.io. Its public API is documented and kept stable. |
| `crates/protocol` | Every type, name and limit crossing a process boundary. TypeScript is generated from it. |
| `crates/core` | The download manager: queue, persistence, media, add-ons. |
| `crates/streams` | HLS and DASH downloads. |
| `crates/files` | File helpers: writing a file whole, discarding one, keeping one private to its owner, and the Mark of the Web. |
| `crates/sites` | Site rules: the Public Suffix List with its built-in snapshot, and which links may reach the person's own network (D39, D44). |
| `crates/server` | The local protocol server (D14): one client connection over any byte stream, answered by the manager. |
| `crates/queues` | Queue rules as pure functions: which downloads start under each queue's limits, and when a scheduled queue runs (D74). |
| `crates/ipc` | Local app endpoint (named pipe on Windows, Unix socket on Linux). |
| `crates/bridge` | Native messaging host between the extension and the app. |
| `crates/cli` | The `wirecatch` command-line downloader. |
| `crates/logging` | Every binary's logging (stderr and a capped file in the log folder) and the redaction that keeps secrets out of log lines (D40). |
| `crates/e2e` | End-to-end tests across extension, bridge and app. |
| `crates/product` | Product identity constants, generated from `config/product.toml`. Never edit `src` by hand. |
| `crates/test-support` | Unpublished test servers and helpers every crate's tests share (D26). |
| `apps/desktop` | Tauri app; its Rust side is `apps/desktop/src-tauri`. |
| `apps/extension` | WXT browser extension for Chromium and Firefox. |
| `packages/protocol` | Generated TypeScript types. Never edit `src/generated` by hand. |
| `packages/eslint-config` | The one ESLint configuration, and the `wirecatch/*` rules. |
| `packages/product` | Product identity for TypeScript, generated from `config/product.toml`. Never edit `src` by hand. |
| `packages/units` | Byte units, and how sizes and speeds are shown and typed, for both apps. |
| `packages/ui` | The design system both apps share: tokens, themes, motion and primitives (D36). |
| `config/` | Product identity (`product.toml`) and the templates of the package hooks and `SECURITY.md`; everything else that carries identity is generated from them (D29). |
| `scripts/gate.mjs` | Every quality check: the runner, with the checks in `scripts/gate/`. |

## Gates

```bash
pnpm gate
```

That runs every check CI runs (`node scripts/gate.mjs --list` shows them; `pnpm gate rust` or `pnpm gate js` runs a group). The supply-chain checks need two cargo tools: `node scripts/gate.mjs --install-tools`. It takes about 25 minutes, so locally run only the checks a change can break (D78): `cargo clippy -p <crate>`, `cargo test -p <crate>`, `pnpm --filter <package> check`, `npx vitest run <files>`, or a gate group such as `node scripts/gate.mjs text docs commits` for prose. The full gate runs locally only for a wide change (lint or build config, dependencies, `crates/protocol`, gate scripts, several crates and apps at once), once, on the final tree; the pull request's CI runs every check. Report the checks you ran and their real output, failures included. The definition of done is D18 in `docs/DECISIONS.md`.

- Install once with `pnpm install`; it also turns on the commit-msg hook.
- Lint and format from the root only (`pnpm lint`, `pnpm format`); packages have no lint or format scripts, and list ESLint or Prettier only when their own code imports it (D42).
- Build the desktop interface (`pnpm --filter @wirecatch/desktop build`) before building or running the desktop app itself; the gate compiles the desktop crate against a placeholder page when it is missing.
- Changing `crates/protocol` means running `pnpm protocol:generate` and committing the generated files; the gate fails otherwise.
- Changing `config/product.toml` or `config/templates/` means running `pnpm product:generate` and committing the generated files; the gate fails otherwise. Before a release, `node scripts/gate.mjs publish` must pass: no identity value may still be a `TODO-PRODUCT-` placeholder.
- `wirecatch-bridge/test-overrides` is for tests only and is never enabled in a build.
- Benchmark: `cargo run --release -p wirecatch-engine --example bench_local`.

## How to work here

- Fix the cause, not the symptom. When the same problem shows up in several places, fix it once at its source (docs/ENGINEERING.md, sections 1 and 3).
- Data two languages or two packages need, test data included, lives in one file both load (docs/ENGINEERING.md, section 1). Before writing a list of values, samples or expected outputs, look for the file that already holds them; a reviewer flags any second copy.
- Leave every file you touch better. No lint suppressions: fix the cause. The only exceptions are a single `#[expect(..., reason = "...")]`, an ESLint disable comment with a `--` reason, or a `svelte-ignore` with its reason in parentheses (`<!-- svelte-ignore code (reason) -->`), where the rule is wrong for that code, each listed in docs/AUDIT.md W14 and in `LINT_EXCEPTIONS` in `scripts/gate/exceptions.mjs` (the `exceptions` check fails on any other).
- A bug fix starts with a failing test that proves the bug.
- A test that passes here proves nothing about CI, whose runners are slower, busier and also Linux. Never assert how long something took, and never give a wait that has to succeed a short fixed time: wait for the event under `PATIENCE`, make the slow path outlast `PATIENCE` (hold the server, lengthen the timeout under test), count what was sent or tried, or run on tokio's paused clock (docs/ENGINEERING.md, section 7). The `test-timing` check refuses both in Rust; in TypeScript only review catches them.
- When CI fails on something that passed locally, find what differs between the two (load, the other system, a time limit in the code under test, an input that changes by itself such as a new advisory) before changing anything, and fix that at its source. Never rerun until it passes, widen a bound, or call it flaky without naming the cause.
- When a rule or gate is too weak to catch a problem you found, say so and propose the check, rather than only fixing the one instance.
- In a work package, edit only the folders it owns (`docs/PLAN.md`). Propose changes to shared files in your report.
- Commit messages follow docs/ENGINEERING.md, section 9. Never add a `Co-Authored-By` line or any other tool or AI credit.
