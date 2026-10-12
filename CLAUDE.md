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
| `crates/ipc` | Local app endpoint (named pipe on Windows, Unix socket on Linux). |
| `crates/bridge` | Native messaging host between the extension and the app. |
| `crates/cli` | The `wirecatch` command-line downloader. |
| `crates/e2e` | End-to-end tests across extension, bridge and app. |
| `apps/desktop` | Tauri app; its Rust side is `apps/desktop/src-tauri`. |
| `apps/extension` | WXT browser extension for Chromium and Firefox. |
| `packages/protocol` | Generated TypeScript types. Never edit `src/generated` by hand. |
| `packages/eslint-config` | The one ESLint configuration. |
| `config/` | Product identity shared by every package: today the extension ids; all of it after [AUDIT](docs/AUDIT.md) W2. |
| `scripts/gate.mjs` | Every quality check. |

## Gates

```bash
pnpm gate
```

That runs every check CI runs (`node scripts/gate.mjs --list` shows them; `pnpm gate rust` or `pnpm gate js` runs a group). The supply-chain checks need two cargo tools: `node scripts/gate.mjs --install-tools`. Run it yourself before reporting a change done, and report its real output, failures included. The definition of done is D18 in `docs/DECISIONS.md`.

- Install once with `pnpm install`; it also turns on the commit-msg hook.
- Build the desktop interface (`pnpm --filter @wirecatch/desktop build`) before building or running the desktop app itself; the gate compiles the desktop crate against a placeholder page when it is missing.
- Changing `crates/protocol` means running `pnpm protocol:generate` and committing the generated files; the gate fails otherwise.
- `wirecatch-bridge/test-overrides` is for tests only and is never enabled in a build.
- A run that builds Rust needs 15 GiB free where cargo builds (`CARGO_TARGET_DIR`, or `target/`) and 2 GiB on the repository's disk; the gate checks before it starts and says how to free space.
- On Windows and macOS, `clippy-linux` lints for Linux; it needs the target's standard library: `rustup target add x86_64-unknown-linux-gnu`.
- Benchmark: `cargo run --release -p wirecatch-engine --example bench_local`.

## How to work here

- Fix the cause, not the symptom. When the same problem shows up in several places, fix it once at its source (docs/ENGINEERING.md, sections 1 and 3).
- Leave every file you touch better. No lint suppressions: fix the cause. The only exceptions are a single `#[expect(..., reason = "...")]` or a disable comment with a `--` reason, where the rule is wrong for that line.
- A bug fix starts with a failing test that proves the bug.
- When a rule or gate is too weak to catch a problem you found, say so and propose the check, rather than only fixing the one instance.
- In a work package, edit only the folders it owns (`docs/PLAN.md`). Propose changes to shared files in your report.
- Commit messages follow docs/ENGINEERING.md, section 9. Never add a `Co-Authored-By` line or any other tool or AI credit.
