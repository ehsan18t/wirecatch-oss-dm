# Contributing to Wirecatch

Thanks for helping. Quality comes before speed here: a change is done only when the gate passes and it has tests that would fail without it.

## Before you start

Read [docs/DECISIONS.md](docs/DECISIONS.md) (why things are the way they are) and, for larger work, [docs/PLAN.md](docs/PLAN.md). Types that cross a process boundary live in `crates/protocol`; change them there and regenerate the TypeScript with `pnpm protocol:generate`, committing the generated files.

## Quality gates

Run the gate before sending a change. It runs every check CI runs, from one script (`scripts/gate.mjs`):

```bash
pnpm gate
```

`node scripts/gate.mjs --list` shows each check and group; `pnpm gate rust` or `pnpm gate js` runs one group.

Notes:

- `pnpm install` also turns on a commit-msg hook that checks your commit messages.
- The desktop crate's tests run separately because Tauri's build script breaks other crates' doctests when everything links together.
- `wirecatch-bridge/test-overrides` is for tests only and must never be enabled in a build.
- Every bridge build trusts the same Chromium extension id (`chromium_id` in `config/extension-ids.json`) and the Firefox add-on id (D22). Every Chromium build of the extension (`pnpm dev`, `pnpm build` or `pnpm build:dev` in `apps/extension`) carries the matching public key, so an unpacked load gets that id and works with any bridge, including an installed release. Only `pnpm build:store` leaves the key out, for the Chrome Web Store (`apps/extension/PUBLISHING.md`). To register a bridge you built yourself, run `cargo run -p wirecatch-bridge -- install`.
- The stream joining tests use a real ffmpeg when `WIRECATCH_TEST_FFMPEG` points to one; CI sets `WIRECATCH_REQUIRE_FFMPEG=1` so they cannot be skipped there.
- On Windows or macOS with Docker running, `bash scripts/linux-test.sh` runs the Rust gate on Linux as a normal user, with a real ffmpeg and the same Rust release as CI.
- Tests never reach the internet. Servers in tests are local (see `crates/engine/tests/common`).

## Conventions

[docs/ENGINEERING.md](docs/ENGINEERING.md) is the engineering standard: where every shared value lives, errors, unsafe code, async, tests, writing and commit messages. Every change follows it.

## Security

Anything that handles cookies, credentials, file paths from outside, or downloaded executables gets extra care and tests for the hostile case. Report vulnerabilities privately to the maintainers rather than in a public issue.
