# Wirecatch

A fast, efficient, open-source download manager for Windows and Linux, with browser integration for Chromium-based and Firefox-based browsers.

Status: **Phase 1** (download engine and command-line tool). See [docs/ROADMAP.md](docs/ROADMAP.md) for what comes next and [docs/DECISIONS.md](docs/DECISIONS.md) for the decisions behind the design.

## What works today

- Downloads over several parallel connections (8 by default), each its own TCP connection.
- Dynamic splitting: when a connection finishes its part, it takes over half of the largest remaining part, so there is no slow tail at the end.
- Resume after Ctrl+C, a crash, or a killed process: run the same command again. If the file changed on the server, it starts over cleanly instead of producing a corrupt file.
- Retries with backoff on dropped connections, timeouts, 5xx and 429 responses.
- Falls back to one connection when a server does not support byte ranges.
- Reserves disk space without writing the file twice (sparse file on NTFS, `fallocate` on Linux).
- Safe file names from `Content-Disposition` or the URL; never overwrites an existing file.

## Usage

```bash
cargo run --release -p wirecatch-cli -- https://example.com/file.iso
```

Options: `-o <dir>` output folder, `-n <name>` file name, `-c <n>` connections (1 to 64), `-H "Name: value"` extra header (repeatable), `-q` no progress bar. Set `WIRECATCH_LOG=debug` for detailed logs.

## Benchmark

`cargo run --release -p wirecatch-engine --example bench_local` downloads a 256 MiB file from a local server that caps each connection at 16 MiB/s, the way many file hosts and CDNs do. Measured on Windows 11:

| Connections | MiB/s | Speedup |
|---:|---:|---:|
| 1 | 12.7 | 1.0x |
| 2 | 25.1 | 2.0x |
| 4 | 49.3 | 3.9x |
| 8 | 94.2 | 7.4x |
| 16 | 168.6 | 13.3x |

Without the cap, the engine moves about 930 MiB/s over localhost with 8 connections, well above any real internet link.

## Development

```bash
cargo test --workspace
```

```bash
cargo clippy --all-targets
```

Layout:

- `crates/engine`: the download engine library (`wirecatch-engine`).
- `crates/cli`: the `wirecatch` command-line tool.
- `crates/engine/tests`: integration tests against a local HTTP server with range support, throttling and injected disconnects.

## License

GPL-3.0-only.
