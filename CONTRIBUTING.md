# Contributing to Wirecatch

Thanks for helping. Quality comes before speed here: a change is done only when every gate below passes and it has tests that would fail without it.

## Before you start

Read [docs/DECISIONS.md](docs/DECISIONS.md) (why things are the way they are) and, for larger work, [docs/PLAN.md](docs/PLAN.md). Types that cross a process boundary live in `crates/protocol`; change them there and regenerate the TypeScript with `pnpm protocol:generate`, committing the generated files.

## Quality gates

Run all of these before sending a change.

```bash
cargo fmt --all --check
```

```bash
cargo clippy --workspace --all-targets --features wirecatch-bridge/test-overrides -- -D warnings
```

```bash
cargo test --workspace --exclude wirecatch-desktop --features wirecatch-bridge/test-overrides
```

```bash
cargo test -p wirecatch-desktop
```

```bash
pnpm check && pnpm lint && pnpm test && pnpm build
```

Notes:

- Build the desktop frontend once (`pnpm --filter @wirecatch/desktop build`) before any cargo command that touches the desktop crate.
- The desktop crate's tests run separately because Tauri's build script breaks other crates' doctests when everything links together.
- `wirecatch-bridge/test-overrides` is for tests only and must never be enabled in a build.
- Every bridge build trusts the same Chromium extension id (`chromium_id` in `config/extension-ids.json`) and the Firefox add-on id (D22). The unpacked development extension (`pnpm dev` or `pnpm build:dev` in `apps/extension`) carries the matching public key, so it gets that id and works with any bridge, including an installed release. To register a bridge you built yourself, run `cargo run -p wirecatch-bridge -- install`.
- The stream joining tests use a real ffmpeg when `WIRECATCH_TEST_FFMPEG` points to one; CI sets `WIRECATCH_REQUIRE_FFMPEG=1` so they cannot be skipped there.
- On Windows or macOS with Docker running, `bash scripts/linux-test.sh` runs clippy and the whole Rust suite on Linux as a normal user (with a real ffmpeg), the same way CI does.
- Tests never reach the internet. Servers in tests are local (see `crates/engine/tests/common`).

## Conventions

- **Commits** follow Conventional Commits: `type(scope): subject`, in lower case, imperative, without a trailing period, at most 72 characters. Types: `feat`, `fix`, `docs`, `test`, `refactor`, `perf`, `build`, `ci`, `chore`, `style`, `revert`. The scope is the package (`engine`, `streams`, `core`, `protocol`, `ipc`, `bridge`, `cli`, `e2e`, `desktop`, `extension`). The body explains why.
- **Writing:** plain language everywhere people read it (UI, errors, docs, comments, commits). No em dashes. Do not hard-wrap paragraphs. Errors say what happened and how to fix it.
- **Rust:** no `unwrap` or `expect` on data from outside the process, `thiserror` in libraries and `anyhow` only in binaries, a doc comment on every public item, no blocking calls on async tasks.
- **Svelte and TypeScript:** Svelte 5 runes only, strict TypeScript, no `any`.
- **Performance** is a feature: no per-chunk allocations in hot paths, no polling where an event exists, no work proportional to the download list on each progress update.

## Security

Anything that handles cookies, credentials, file paths from outside, or downloaded executables gets extra care and tests for the hostile case. Report vulnerabilities privately to the maintainers rather than in a public issue.
