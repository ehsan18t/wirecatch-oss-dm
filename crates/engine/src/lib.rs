//! Wirecatch download engine.
//!
//! Downloads a file over several parallel connections, splits the largest
//! remaining range whenever a connection frees up, writes straight to the right
//! offsets on disk, and keeps a resume file so an interrupted download continues
//! where it stopped.

mod download;
mod error;
mod http;
mod limiter;
pub mod naming;
mod progress;
mod segment;
mod state;
mod storage;
mod worker;

pub use download::{DownloadOptions, DownloadRequest, Outcome, download};
pub use error::{Error, Result};
pub use limiter::RateLimiter;
pub use progress::Progress;
pub use reqwest::header;
pub use storage::{PART_EXT, STATE_EXT};
pub use tokio_util::sync::CancellationToken;
pub use url::Url;
