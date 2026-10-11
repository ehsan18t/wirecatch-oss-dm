# Wirecatch

A fast, efficient, open-source download manager for Windows and Linux, with browser integration for Chromium-based and Firefox-based browsers. It aims to be what Internet Download Manager is, without the hidden features: every option is visible, queues are in plain sight, and errors tell you how to fix them.

License: GPL-3.0-only. Decisions behind the design are in [docs/DECISIONS.md](docs/DECISIONS.md); how the work is organised is in [docs/PLAN.md](docs/PLAN.md).

## Features

- **Fast downloads.** Several parallel connections per file, each its own TCP connection; when a connection finishes its part it takes over half of the largest remaining part, so there is no slow tail. Speed scales almost linearly with connections against servers that cap each connection.
- **Resume anything resumable.** Pause, quit, crash or lose power: the download continues where it stopped. A file that changed on the server starts over cleanly instead of producing a corrupt file. Expired links can be refreshed without losing the downloaded part.
- **Browser integration.** The extension takes over downloads from Chrome, Edge, Brave, Vivaldi, Opera, Chromium and Firefox, with your cookies so downloads behind a login work. If the app does not confirm, the browser keeps the download: nothing is ever lost.
- **Video and audio.** HLS and DASH streams (including AES-128), direct media, and a download button over videos. Sites such as YouTube work with the optional one-click Media add-on (yt-dlp, ffmpeg and Deno, pinned and verified by checksum).
- **Queues and scheduling.** Queues live in the sidebar with their schedule, speed limit and running count visible. Every waiting download says why it waits.
- **Protocols.** HTTP/1.1, HTTP/2, FTP and FTPS (implicit and explicit). HTTP/3 is available as an experimental option.
- **Safe by default.** Credentials only go to the site they belong to, downloaded programs are marked as coming from the internet on Windows, the browser bridge only accepts the Wirecatch extension and only your user account can talk to the app.
- **Settings you can carry.** Export your settings to one file and import them on another computer, even across Windows and Linux.

## Components

| Path | What it is |
|---|---|
| `crates/engine` | `wirecatch-engine`: the download engine library (HTTP, FTP, segmented, resumable). Published to crates.io. |
| `crates/streams` | `wirecatch-streams`: HLS and DASH download and joining. |
| `crates/core` | `wirecatch-core`: the download manager (queues, schedules, persistence, settings, add-ons, protocol server). |
| `crates/protocol` | Every type that crosses a process boundary; TypeScript is generated from it. |
| `crates/ipc` | The per-user local connection between the app and the bridge. |
| `crates/bridge` | `wirecatch-bridge`: the native messaging host browsers start. |
| `crates/cli` | `wirecatch`: a command-line downloader built on the engine. |
| `crates/e2e` | End-to-end tests over the real bridge, socket and manager. |
| `apps/desktop` | The desktop app (Tauri 2, SvelteKit, Svelte 5). |
| `apps/extension` | The browser extension (WXT, Svelte 5) for Chromium and Firefox. |
| `packages/protocol` | Generated TypeScript types. |

## Getting started (development)

Requirements: Rust 1.90 or newer, Node 22 or newer, pnpm 10. On Linux, the Tauri system packages (WebKitGTK and friends, see `.github/workflows/ci.yml`).

```bash
pnpm install
```

```bash
pnpm --filter @wirecatch/desktop build
```

```bash
cargo test --workspace --exclude wirecatch-desktop --features wirecatch-bridge/test-overrides
```

Run the desktop app in development mode:

```bash
pnpm --filter @wirecatch/desktop tauri dev
```

Try the interface in a normal browser with realistic sample data:

```bash
pnpm --filter @wirecatch/desktop dev:mock
```

Build the extension for both browsers (output in `apps/extension/.output`):

```bash
pnpm --filter @wirecatch/extension build
```

Download from the command line:

```bash
cargo run --release -p wirecatch-cli -- https://example.com/file.iso
```

See [CONTRIBUTING.md](CONTRIBUTING.md) for the quality gates, conventions and how to send changes.

## Benchmark

`cargo run --release -p wirecatch-engine --example bench_local` downloads a 256 MiB file from a local server that caps each connection at 16 MiB/s, the way many file hosts do. Measured on Windows 11:

| Connections | MiB/s | Speedup |
|---:|---:|---:|
| 1 | 12.7 | 1.0x |
| 2 | 25.1 | 2.0x |
| 4 | 49.3 | 3.9x |
| 8 | 94.2 | 7.4x |
| 16 | 168.6 | 13.3x |

## Status

Version 0.1 is in development; see [docs/ROADMAP.md](docs/ROADMAP.md). Not yet released: no installers are published and the browser extension is not in the stores yet.
