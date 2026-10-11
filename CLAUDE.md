# Wirecatch

Open-source (GPL-3.0-only) download manager for Windows and Linux: Rust engine and core, Tauri 2 + Svelte 5 desktop app, WXT + Svelte 5 browser extension.

Read before working: `docs/DECISIONS.md` (why things are the way they are), `docs/PLAN.md` (work packages, ownership, acceptance checklists), `docs/ROADMAP.md`.

## Layout

- `crates/engine`: download engine library, published to crates.io. Public API must stay documented and stable.
- `crates/protocol`: single source of truth for every type crossing a process boundary. TypeScript is generated from it.
- `crates/core`, `crates/bridge`, `crates/cli`: manager, native messaging host, CLI.
- `apps/desktop` (Tauri, its Rust side in `apps/desktop/src-tauri`), `apps/extension` (WXT).
- `packages/protocol`: generated TypeScript types. Never edit `src/generated` by hand.

## Commands

```bash
cargo fmt --all
cargo clippy --workspace --all-targets --features wirecatch-bridge/test-overrides -- -D warnings
cargo test --workspace --exclude wirecatch-desktop --features wirecatch-bridge/test-overrides
cargo test -p wirecatch-desktop
pnpm install
pnpm protocol:generate
pnpm check && pnpm lint && pnpm test && pnpm build
```

Benchmark: `cargo run --release -p wirecatch-engine --example bench_local`.

Notes: the desktop crate needs its frontend built once (`pnpm --filter @wirecatch/desktop build`) before any cargo command touching it. The desktop crate's tests run separately because Tauri's build script breaks other crates' doctests when linked together. `wirecatch-bridge/test-overrides` is for tests only and must never be enabled in a build.

## Rules

- Quality before speed. Definition of done is D18 in `docs/DECISIONS.md`. Run the gates yourself before reporting done; report real output, including failures.
- Changing `crates/protocol` means running `pnpm protocol:generate` and committing the generated files.
- Never edit outside the folder your work package owns (see `docs/PLAN.md`). Propose shared changes in your report.
- Tests that need a server use the local test server in `crates/engine/tests/common`; tests never reach the internet.
- Performance matters: no blocking calls on async tasks, no per-chunk allocations in hot paths, no polling where an event exists.
- User-facing text (UI copy, CLI output, errors, docs, comments, commit messages): plain language, no em dashes, never hard-wrap a paragraph. Errors say what happened and how to fix it.
- Commit messages follow Conventional Commits: `type(scope): subject`, subject in lower case, imperative, no trailing period, at most 72 characters. Types: `feat`, `fix`, `docs`, `test`, `refactor`, `perf`, `build`, `ci`, `chore`, `style`, `revert`. Scope is the package: `engine`, `protocol`, `core`, `ipc`, `bridge`, `streams`, `cli`, `e2e`, `desktop`, `extension`; omit it for repo-wide changes. Breaking changes use `!` after the scope and a `BREAKING CHANGE:` footer. The body explains why, one line per paragraph. Merge commits into main: `chore: merge <what> (<work package>)`, for example `chore: merge hls and dash streams (WP8)`. Never add `Co-Authored-By` or any other AI or tool attribution trailer or line to a commit message; the author is the person who owns the repository.
- Svelte: Svelte 5 runes only (`$state`, `$derived`, `$effect`, `$props`), strict TypeScript, no `any`.
- Rust: no `unwrap()`/`expect()` on data from outside the process; `thiserror` in libraries, `anyhow` only in binaries; doc comment on every public item.
