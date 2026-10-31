# Contributing to Wirecatch

Thanks for helping. Quality comes before speed here: a change is done only when the gate passes and it has tests that would fail without it.

## Before you start

Read [docs/DECISIONS.md](docs/DECISIONS.md) (why things are the way they are) and, for larger work, [docs/PLAN.md](docs/PLAN.md) and [docs/ROADMAP.md](docs/ROADMAP.md). Types that cross a process boundary live in `crates/protocol`; change them there and regenerate the TypeScript with `pnpm protocol:generate`, committing the generated files. Product identity (name, ids, links) lives in `config/product.toml`; change it there and run `pnpm product:generate`, committing the generated files.

## Getting started

Requirements: rustup (`rust-toolchain.toml` names the latest stable Rust; `rustup update` keeps it current), Node 22 or newer, pnpm 10. On Linux, the libraries the desktop app links against through GPUI (`scripts/ci/linux-packages.txt`).

```bash
pnpm install
```

Run the desktop app (a development build with GPUI optimised, so it draws at the display's rate):

```bash
cargo ui
```

Build the installers for this machine (output in `target/packages`; see [crates/desktop/PACKAGING.md](crates/desktop/PACKAGING.md)):

```bash
node scripts/package.mjs
```

Build the extension for both browsers (output in `apps/extension/.output`):

```bash
pnpm --filter @wirecatch/extension build
```

Download from the command line:

```bash
cargo run --release -p wirecatch-cli -- https://example.com/file.iso
```

Run the benchmark, which downloads a 256 MiB file from a local server that caps each connection at 16 MiB/s (the README has the latest numbers; measure on a quiet machine):

```bash
cargo run --release -p wirecatch-engine --example bench_local
```

## Repository layout

| Path                     | What it is                                                                                                                                        |
| ------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------- |
| `crates/engine`          | `wirecatch-engine`: the download engine library (HTTP, FTP, segmented, resumable), for crates.io. Its public API is documented and kept stable.   |
| `crates/streams`         | `wirecatch-streams`: HLS and DASH download and joining.                                                                                           |
| `crates/queues`          | `wirecatch-queues`: queue rules as pure functions, such as which downloads start under each queue's limits and when a scheduled queue runs (D74). |
| `crates/core`            | `wirecatch-core`: the download manager (queues, schedules, running downloads and media downloads, add-on installs).                               |
| `crates/addons`          | `wirecatch-addons`: the media add-on's releases, checks and unpacking (D20, D21).                                                                 |
| `crates/records`         | `wirecatch-records`: the saved types of a download, a queue's batch and the proxy passwords.                                                      |
| `crates/store`           | `wirecatch-store`: the download list in SQLite, its migrations and repairs (D17).                                                                 |
| `crates/messages`        | `wirecatch-messages`: the messages for a failed download and the fix each offers.                                                                 |
| `crates/media`           | `wirecatch-media`: the quality choice, yt-dlp pages, ffmpeg conversions and joining.                                                              |
| `crates/settings`        | `wirecatch-settings`: settings defaults and checks, category rules, the proxy setting and settings files.                                         |
| `crates/files`           | `wirecatch-files`: writing a file whole, discarding, owner-only access and the Mark of the Web.                                                   |
| `crates/sites`           | `wirecatch-sites`: the Public Suffix List and the local network rules (D39, D44).                                                                 |
| `crates/server`          | `wirecatch-server`: the local protocol server (D14) that answers the extension and the CLI from the manager.                                      |
| `crates/protocol`        | Every type, name and limit that crosses a process boundary; TypeScript is generated from it.                                                      |
| `crates/ipc`             | The per-user local connection between the app and the bridge (named pipe on Windows, Unix socket on Linux).                                       |
| `crates/bridge`          | `wirecatch-bridge`: the native messaging host browsers start.                                                                                     |
| `crates/cli`             | `wirecatch`: a command-line downloader built on the engine.                                                                                       |
| `crates/logging`         | `wirecatch-logging`: the one logging setup every binary uses, with capped files and redaction (D40).                                              |
| `crates/e2e`             | End-to-end tests over the real bridge, socket and manager.                                                                                        |
| `crates/product`         | Product identity constants, generated from `config/product.toml`. Never edit `src` by hand.                                                       |
| `crates/test-support`    | Unpublished test servers and helpers every crate's tests share (D26).                                                                             |
| `crates/desktop`         | `wirecatch-desktop`: the desktop app, drawn with GPUI (D95, D99), and its installers (`packaging/`, D106).                                        |
| `apps/extension`         | The browser extension (WXT, Svelte 5) for Chromium and Firefox.                                                                                   |
| `packages/protocol`      | Generated TypeScript types. Never edit `src/generated` by hand.                                                                                   |
| `packages/product`       | Product identity for TypeScript, generated from `config/product.toml`. Never edit `src` by hand.                                                  |
| `packages/ui`            | The design system both apps share: tokens, themes, motion and primitives.                                                                         |
| `packages/units`         | Byte units, and how sizes and speeds are shown and typed, for both apps.                                                                          |
| `packages/eslint-config` | The one ESLint configuration every package uses, and the `wirecatch/*` rules.                                                                     |
| `config/`                | Product identity (`product.toml`) and the templates everything else that carries identity is generated from (D29).                                |
| `scripts/gate.mjs`       | Every quality check: the runner, with the checks in `scripts/gate/`.                                                                              |

## Quality gates

Run the gate before sending a change. It runs every check CI runs, from one script (`scripts/gate.mjs`). The supply-chain checks need cargo-deny and cargo-machete; `node scripts/gate.mjs --install-tools` installs the versions CI uses.

```bash
pnpm gate
```

`node scripts/gate.mjs --list` shows each check and group; `pnpm gate rust`, `pnpm gate js` or `pnpm gate browser` runs one group.

Notes:

- Lint and format from the root only (`pnpm lint`, `pnpm format`); packages have no lint or format scripts of their own (D42).
- `pnpm install` also turns on a commit-msg hook that checks your commit messages (unless you already use another hooks folder; it says so).
- The desktop crate's tests run with every other crate's, in one build. Cargo builds of the desktop app link the Visual C++ runtime dynamically; `scripts/package.mjs` links it statically (D33).
- Debug builds carry line tables for the workspace's crates and no debug info for dependencies (D33). For a debugger session, set `CARGO_PROFILE_DEV_DEBUG=full`.
- `wirecatch-bridge/test-overrides` is for tests only and must never be enabled in a build.
- Every bridge build trusts the same Chromium extension id (`chromium_id` in `config/product.toml`) and the Firefox add-on id (D22). Every Chromium build of the extension (`pnpm dev`, `pnpm build` or `pnpm build:dev` in `apps/extension`) carries the matching public key, so an unpacked load gets that id and works with any bridge, including an installed release. Only `pnpm build:store` leaves the key out, for the Chrome Web Store (`apps/extension/PUBLISHING.md`). To register a bridge you built yourself, run `cargo run -p wirecatch-bridge -- install`.
- The stream joining tests use a real ffmpeg when `WIRECATCH_TEST_FFMPEG` points to one; CI sets `WIRECATCH_REQUIRE_FFMPEG=1` so they cannot be skipped there, with the build the media add-on installs (`cargo run -p wirecatch-core --example ffmpeg_release` prints where it comes from, D54).
- The browser check (`e2e-extension`, the extension's pages with every word pseudo-translated longer, the gate's `browser` group) runs in Edge, or in the browser `PW_CHANNEL` names; where that browser is not installed it is skipped with a note, and any other failure to start it fails. CI runs it in a Browser job of its own on each system, with `WIRECATCH_REQUIRE_BROWSER=1` so it cannot be skipped there.
- On Windows or macOS with Docker running, `bash scripts/linux-test.sh` runs the gate's Rust group on Linux as a normal user, with a real ffmpeg and the same Rust release as CI, leaving out the desktop crate (`--no-desktop`: the container has no GPUI libraries).
- Tests never reach the internet. Servers in tests are local (see `crates/engine/tests/common`).

## Conventions

[docs/ENGINEERING.md](docs/ENGINEERING.md) is the engineering standard: where every shared value lives, errors, unsafe code, async, tests, writing and commit messages. Every change follows it.

## Security

Anything that handles cookies, credentials, file paths from outside, or downloaded executables gets extra care and tests for the hostile case. Report vulnerabilities privately, as `SECURITY.md` says, never in a public issue.
