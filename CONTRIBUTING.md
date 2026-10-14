# Contributing to Wirecatch

Thanks for helping. Quality comes before speed here: a change is done only when the gate passes and it has tests that would fail without it.

## Before you start

Read [docs/DECISIONS.md](docs/DECISIONS.md) (why things are the way they are) and, for larger work, [docs/PLAN.md](docs/PLAN.md). Types that cross a process boundary live in `crates/protocol`; change them there and regenerate the TypeScript with `pnpm protocol:generate`, committing the generated files.

## Quality gates

Run the gate before sending a change. It runs every check CI runs, from one script (`scripts/gate.mjs`). The supply-chain checks need cargo-deny and cargo-machete; `node scripts/gate.mjs --install-tools` installs the versions CI uses.

```bash
pnpm gate
```

`node scripts/gate.mjs --list` shows each check and group; `pnpm gate rust` or `pnpm gate js` runs one group.

Notes:

- `pnpm install` also turns on a commit-msg hook that checks your commit messages (unless you already use another hooks folder; it says so).
- Build the desktop interface once (`pnpm --filter @wirecatch/desktop build`) before building or running the desktop app; the gate compiles the desktop crate against a placeholder page when it is missing.
- The desktop crate's tests run with every other crate's, in one build. Cargo builds of the desktop crate link the Visual C++ runtime dynamically; packaging builds, which merge `tauri.bundle.conf.json`, link it statically (D33).
- Debug builds carry line tables for the workspace's crates and no debug info for dependencies (D33). For a debugger session, set `CARGO_PROFILE_DEV_DEBUG=full`.
- `wirecatch-bridge/test-overrides` is for tests only and must never be enabled in a build.
- Every bridge build trusts the same Chromium extension id (`chromium_id` in `config/product.toml`) and the Firefox add-on id (D22). Every Chromium build of the extension (`pnpm dev`, `pnpm build` or `pnpm build:dev` in `apps/extension`) carries the matching public key, so an unpacked load gets that id and works with any bridge, including an installed release. Only `pnpm build:store` leaves the key out, for the Chrome Web Store (`apps/extension/PUBLISHING.md`). To register a bridge you built yourself, run `cargo run -p wirecatch-bridge -- install`.
- The stream joining tests use a real ffmpeg when `WIRECATCH_TEST_FFMPEG` points to one; CI sets `WIRECATCH_REQUIRE_FFMPEG=1` so they cannot be skipped there.
- The browser checks (`e2e-gallery`, the design system's gallery, `e2e-desktop`, the main window's accessibility and layout on sample data, and `e2e-extension`, the extension's pages with every word pseudo-translated longer, in the gate's `js` group) run in Edge, or in the browser `PW_CHANNEL` names; where that browser is not installed they are skipped with a note, and any other failure to start it fails. CI's Windows JavaScript job sets `WIRECATCH_REQUIRE_BROWSER=1` so they cannot be skipped there.
- On Windows or macOS with Docker running, `bash scripts/linux-test.sh` runs the gate's Rust group on Linux as a normal user, with a real ffmpeg and the same Rust release as CI, leaving out the desktop crate (it needs a display).
- Tests never reach the internet. Servers in tests are local (see `crates/engine/tests/common`).

## Conventions

[docs/ENGINEERING.md](docs/ENGINEERING.md) is the engineering standard: where every shared value lives, errors, unsafe code, async, tests, writing and commit messages. Every change follows it.

## Security

Anything that handles cookies, credentials, file paths from outside, or downloaded executables gets extra care and tests for the hostile case. Report vulnerabilities privately, as `SECURITY.md` says, never in a public issue.
