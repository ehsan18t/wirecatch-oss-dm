# wirecatch-engine

Fast, resumable HTTP and HTTPS downloads over parallel connections. This is the download engine of Wirecatch, an open-source download manager for Windows and Linux, published as a library so other programs can use it too.

## What it does

- **Parallel connections.** One file is downloaded over several connections at once (8 by default), each with its own TCP connection, so servers that cap speed per connection are really downloaded from in parallel.
- **Dynamic splitting.** When a connection finishes its part, it takes over half of the largest part still left, so no single slow connection holds up the end.
- **Straight to disk.** Every byte is written once, at its final offset, in large buffered writes off the async threads. Space is reserved up front without writing it twice (sparse files on NTFS, `fallocate` on Linux).
- **Resume anywhere.** Data goes to `<name>.wcpart` next to a small `<name>.wcstate` resume file. Pausing, a dropped connection or a killed process all continue from where they stopped. If the file changed on the server (size, `ETag` or `Last-Modified`), the download starts over cleanly instead of producing a corrupt file.
- **Polite with busy servers.** Network errors, timeouts (including a server that never answers) and the statuses `408`, `425`, `429`, `500`, `502`, `503` and `504` are retried with growing pauses; any other status ends the download. `Retry-After` is honored, up to five minutes. A server that refuses extra connections gets only as many as it allows, and the download still completes.
- **Safe with credentials.** Cookies, `Authorization`, `user:password@` in the URL and other headers you pass are only sent to the origin you gave them for, never to another host a redirect leads to; other hosts get the `Referer` cut down to its origin. A redirect from `https` to `http` is refused unless you allow it. Proxy credentials go in the proxy URL (for example `HTTPS_PROXY=http://user:password@proxy:8080`), not in a header.
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

`cargo run --release -p wirecatch-engine --example bench_local` downloads a 256 MiB file from a local server that caps each connection at 16 MiB/s, with 1 to 16 connections, and then from an uncapped server to show the engine's own ceiling. The speedup is close to linear in the number of connections.

## Requirements

- A Tokio runtime with the time and I/O drivers enabled (`#[tokio::main]` has both).
- Rust 1.88 or newer.
- TLS uses rustls with the operating system's root certificates.

## Status

Pre-1.0: the API may still change between minor versions. HTTP and HTTPS are supported; FTP, HLS and DASH are planned.

## License

GPL-3.0-only.
