//! Fast, resumable HTTP and HTTPS downloads over parallel connections.
//!
//! This is the download engine of Wirecatch, an open-source download manager. It downloads one file over several connections
//! at once, splits the largest remaining range whenever a connection frees up (so
//! no slow tail holds up the end), writes every byte straight to its final offset
//! on disk, and keeps a small resume file so an interrupted download continues
//! where it stopped, even after a crash.
//!
//! # Quick start
//!
//! Build a [`DownloadRequest`], hand it to [`download`] with a [`Progress`] to
//! watch and a [`CancellationToken`] to pause with, and await the [`Outcome`]:
//!
//! ```
//! # fn local_test_server() -> wirecatch_engine::Url {
//! #     // The doctest downloads from a one-shot local server instead of the internet.
//! #     use std::io::{BufRead, BufReader, Write};
//! #     let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
//! #     let address = listener.local_addr().unwrap();
//! #     std::thread::spawn(move || {
//! #         let (stream, _) = listener.accept().unwrap();
//! #         let mut reader = BufReader::new(&stream);
//! #         let mut line = String::new();
//! #         while reader.read_line(&mut line).unwrap() > 2 {
//! #             line.clear();
//! #         }
//! #         (&stream)
//! #             .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 5\r\nConnection: close\r\n\r\nhello")
//! #             .unwrap();
//! #     });
//! #     wirecatch_engine::Url::parse(&format!("http://{address}/report.pdf")).unwrap()
//! # }
//! use std::sync::Arc;
//!
//! use wirecatch_engine::{CancellationToken, DownloadRequest, Outcome, Progress, Url, download};
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     let url = Url::parse("https://example.com/report.pdf")?;
//! #   let url = local_test_server();
//!     let mut request = DownloadRequest::new(url, std::env::temp_dir().join("downloads"));
//!     request.options.connections = 8;
//!
//!     // Read `progress` from any task or thread to show a progress bar.
//!     let progress = Arc::new(Progress::default());
//!     // `cancel.cancel()` pauses the download; running the same request again resumes it.
//!     let cancel = CancellationToken::new();
//!
//!     match download(request, progress, cancel).await? {
//!         Outcome::Completed { path, bytes, .. } => {
//!             println!("saved {bytes} bytes to {}", path.display());
//! #           std::fs::remove_file(&path)?;
//!         }
//!         Outcome::Paused { path, remaining, .. } => {
//!             println!("paused with {remaining} bytes left for {}", path.display());
//!         }
//!         // Outcome may gain variants in later versions.
//!         other => println!("{other:?}"),
//!     }
//!     Ok(())
//! }
//! ```
//!
//! [`download`] needs a Tokio runtime with the time and I/O drivers enabled
//! (`#[tokio::main]` has both).
//!
//! # What happens during a download
//!
//! 1. The first request asks for `bytes=0-`. A `206` answer proves the server
//!    accepts ranges and gives the size; its body is already the start of the
//!    file, so no separate `HEAD` request is spent.
//! 2. Space for the whole file is reserved without writing it (a sparse file on
//!    NTFS, `fallocate` on Linux), as `<name>.wcpart` ([`PART_EXT`]) next to a
//!    `<name>.wcstate` resume file ([`STATE_EXT`]).
//! 3. Up to [`DownloadOptions::connections`] connections each take a range.
//!    When one finishes, it takes over the upper half of the largest range still
//!    left, as long as both halves are at least [`DownloadOptions::min_split`].
//! 4. On success the part file is renamed to its final name and the resume file
//!    is removed. A server without range support falls back to one connection.
//!
//! The file name comes from [`DownloadRequest::file_name`], then the server's
//! `Content-Disposition`, then the URL, and is made safe for Windows and Linux
//! (see [`naming`]). An existing file is never overwritten: a free name such as
//! `report (1).pdf` is used instead. Set [`DownloadRequest::exact_path`] to own
//! the naming yourself, or [`DownloadRequest::resolve_path`] to pick the path
//! from the server's answer (its suggested name, size and type) once it arrives.
//! [`Progress::path`] tells where the file goes as soon as the server has
//! answered.
//!
//! # Pausing and resuming
//!
//! Cancelling the [`CancellationToken`] stops every connection, writes what has
//! arrived, saves the resume file and returns [`Outcome::Paused`]. Running the
//! same request again continues from there, as long as the server still reports
//! the same size, `ETag` and `Last-Modified`; if the file changed on the server,
//! the download starts over cleanly instead of mixing old and new bytes. A killed
//! process resumes the same way, from the last checkpoint
//! ([`DownloadOptions::checkpoint_interval`]).
//!
//! # Speed limits
//!
//! A [`RateLimiter`] caps the combined speed of every connection that shares it.
//! Give one limiter to every download for a global limit and another to the
//! downloads of one group for a group limit; a download obeys all of its
//! [`DownloadOptions::rate_limiters`]. Limits can change while downloads run.
//!
//! ```no_run
//! # use std::sync::Arc;
//! # use wirecatch_engine::{DownloadRequest, RateLimiter, Url};
//! let global = Arc::new(RateLimiter::new(Some(10 * 1024 * 1024))); // 10 MiB/s for everything
//! let queue = Arc::new(RateLimiter::new(None)); // unlimited for now
//!
//! # let url = Url::parse("https://example.com/a.iso").unwrap();
//! let mut request = DownloadRequest::new(url, "downloads");
//! request.options.rate_limiters = vec![global.clone(), queue.clone()];
//!
//! queue.set_limit(Some(2 * 1024 * 1024)); // takes effect within one chunk
//! ```
//!
//! # Cookies and other request headers
//!
//! [`DownloadOptions::headers`] carries what a browser hands over, such as
//! cookies, `Authorization`, `Referer` and `User-Agent`. Credentials are only
//! ever sent to the origin (scheme, host and port) of [`DownloadRequest::url`].
//! When that URL redirects to another origin, the requests there carry only
//! headers that hold no credentials (`User-Agent`, `Accept`, `Accept-Language`,
//! `Cache-Control`, `DNT`, `Pragma`, and `Referer` cut down to its origin). A
//! `user:password@` in the URL is handled the same way, as an `Authorization`
//! header for its own origin. A redirect from `https` to `http` is refused
//! unless [`DownloadOptions::allow_insecure_redirects`] is set.
//!
//! # Busy servers
//!
//! Network errors, timeouts and `408`, `425`, `429`, `500`, `502`, `503` and
//! `504` answers are retried with growing pauses (see [`Error::is_retryable`]).
//! A server that accepts a connection but never answers counts as a timeout
//! after [`DownloadOptions::stall_timeout`], and cancelling never waits for it.
//! A `429` or `503` with `Retry-After` is retried no sooner than the server asked,
//! up to five minutes. A server that limits how many connections one client may
//! open is detected: an extra connection it keeps refusing (or asks to wait)
//! hands its work to the connections that are running, and the download
//! finishes with those.
//!
//! All connections share one TLS session cache for the whole process, so only
//! the first connection to a host does a full handshake and later ones, in the
//! same download or the next, resume its session. The system's root
//! certificates are read once per process. Each connection
//! still has its own TCP connection, so a server that limits speed per
//! connection is really downloaded from in parallel.

#![deny(missing_docs)]

mod download;
mod error;
pub mod fetch;
mod http;
mod limiter;
pub mod naming;
mod progress;
mod segment;
mod state;
mod storage;
mod worker;

pub use download::{DownloadOptions, DownloadRequest, Outcome, PathResolver, RemoteFile, download};
pub use error::{Error, NetworkError, Result};
pub use fetch::{Fetched, Fetcher};
pub use limiter::RateLimiter;
pub use progress::Progress;
/// HTTP header names and values, for [`DownloadOptions::headers`].
pub use reqwest::header;
pub use storage::{PART_EXT, STATE_EXT};
pub use tokio_util::sync::CancellationToken;
pub use url::Url;
