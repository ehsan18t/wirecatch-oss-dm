# wirecatch-engine

Fast, resumable HTTP, HTTPS, FTP and FTPS downloads over parallel connections. This is the download engine of Wirecatch, an open-source download manager for Windows and Linux, published as a library so other programs can use it too.

## What it does

- **Parallel connections.** One file is downloaded over several connections at once (8 by default), each with its own TCP connection, so servers that cap speed per connection are really downloaded from in parallel.
- **Dynamic splitting.** When a connection finishes its part, it takes over half of the largest part still left, so no single slow connection holds up the end.
- **Straight to disk.** Every byte is written once, at its final offset, in large buffered writes off the async threads. Space is reserved up front without writing it twice (sparse files on NTFS, `fallocate` on Linux).
- **Resume anywhere.** Data goes to `<name>.wcpart` next to a small `<name>.wcstate` resume file. Pausing, a dropped connection, a killed process or a power cut all continue from where they stopped: the resume file only counts what is already on the disk. If the file changed on the server (size, `ETag` or `Last-Modified`), the download starts over cleanly instead of producing a corrupt file.
- **Polite with busy servers.** Network errors, timeouts (including a server that never answers) and the statuses `408`, `425`, `429`, `500`, `502`, `503` and `504` are retried with growing pauses; any other status ends the download. `Retry-After` is honored, up to five minutes. A server that refuses extra connections gets only as many as it allows, and the download still completes.
- **Requests of a size the server likes.** Some servers slow down any single request for more than a certain amount (video sites do this so players fetch a little at a time). With `DownloadOptions::max_request_bytes` set, no request asks for more than that: each connection fetches its part as a series of requests one after another on the same connection, with splitting and resume working as usual. Off by default.
- **Safe with credentials.** Cookies, `Authorization`, `user:password@` in the URL and other headers you pass are only sent to the origin you gave them for, never to another host a redirect leads to; other hosts get the `Referer` cut down to its origin. A redirect from `https` to `http` is refused unless you allow it. Proxy credentials go in the proxy URL (for example `HTTPS_PROXY=http://user:password@proxy:8080`), not in a header.
- **FTP and FTPS too.** `ftp://`, `ftps://` and `ftpes://` links go through the same call, with the same parallel connections, resume and speed limits: passive mode (`EPSV`, then `PASV`), `REST` for each range, `SIZE` and `MDTM` to notice a changed file. `ftps://` is TLS from the start (port 990), or `AUTH TLS` on port 21; `ftpes://` is always `AUTH TLS`. An encrypted link never falls back to plain FTP, and data connections are encrypted too. The login comes from the URL (anonymous without one), is only sent to that server and is never logged. A server that limits sessions gets only as many as it allows. FTP does not go through proxies yet: while `ALL_PROXY` or `FTP_PROXY` is set, an FTP link is refused rather than going around the proxy, unless `NO_PROXY` names its host.
- **HTTP/3, experimental.** With the `http3` cargo feature and `DownloadOptions::http3` on, a server that advertises HTTP/3 (`Alt-Svc`) is downloaded from over QUIC, one QUIC connection per connection; any QUIC failure falls back to HTTP/2 or HTTP/1.1 without stopping the download. Off by default.
- **Speed limits** shared across any number of downloads, changeable while they run.
- **Safe file names** from `Content-Disposition` or the URL, valid on Windows and Linux; an existing file is never overwritten.

## Example

```rust
use std::sync::Arc;

use wirecatch_engine::{CancellationToken, DownloadRequest, Outcome, Progress, Url, download};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let url = Url::parse("https://example.com/report.pdf")?;
    let mut request = DownloadRequest::new(url, "downloads");
    request.options.connections = 8;

    // Read `progress` from any task or thread to show a progress bar.
    let progress = Arc::new(Progress::default());
    // `cancel.cancel()` pauses the download; running the same request again resumes it.
    let cancel = CancellationToken::new();

    match download(request, progress, cancel).await? {
        Outcome::Completed { path, bytes, .. } => {
            println!("saved {bytes} bytes to {}", path.display());
        }
        Outcome::Paused { path, remaining, .. } => {
            println!("paused with {remaining} bytes left for {}", path.display());
        }
        // Outcome may gain variants in later versions.
        other => println!("{other:?}"),
    }
    Ok(())
}
```

The [API documentation](https://docs.rs/wirecatch-engine) covers speed limits, request headers, pausing and resuming, and errors in detail.

## Performance

`cargo run --release -p wirecatch-engine --example bench_local` downloads a 256 MiB file from a local server that caps each connection at 16 MiB/s, with 1 to 16 connections, and then from an uncapped server to show the engine's own ceiling. Each case runs five times (`-- bench <runs>` to change that) and the median is reported with the fastest and slowest run, since a single run can vary by 40 percent. The speedup is close to linear in the number of connections; the uncapped cases include flushing the finished file to the disk.

`cargo run --release -p wirecatch-engine --features http3 --example bench_http3` compares HTTP/3 with HTTP/2 over localhost. On a busy Windows 11 machine, with 4 and 8 connections, both moved a 256 MiB file in about 0.85 to 1 second, but the HTTP/3 runs used 2.4 to 3 seconds of processor time against 0.45 to 0.7 for HTTP/2 (client and server together), with only about half the file going over QUIC. That is why HTTP/3 stays off by default.

## Requirements

- A Tokio runtime with the time and I/O drivers enabled (`#[tokio::main]` has both).
- Rust 1.88 or newer.
- TLS uses rustls with the operating system's root certificates.

## Status

Pre-1.0: the API may still change between minor versions. HTTP, HTTPS, FTP and FTPS are supported, HTTP/3 is experimental; HLS and DASH are planned.

## Cargo features

- `http3` (off): experimental HTTP/3 through quinn and h3, used only when `DownloadOptions::http3` is also on. Without the feature that option is accepted and ignored, and none of the QUIC crates are built.

## License

GPL-3.0-only.
