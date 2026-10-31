//! Types shared by every Wirecatch component.
//!
//! These Rust types are the single source of truth. `pnpm protocol:generate` generates the TypeScript definitions for the desktop UI and the browser extension from them with `ts-rs` into `packages/protocol/src/generated`, together with the limits and defaults the apps need as values (see `exported.rs`); a plain `cargo test` writes them into `target/ts-rs` instead.
//!
//! - [`wire`]: messages exchanged between the extension, the bridge and the app.
//! - [`extension`]: what the extension sends and the settings it can change.
//! - [`app`]: downloads, queues and settings as the desktop UI sees them.
//! - [`rules`]: what an auto queue takes of the new downloads.
//! - [`media_start`]: where a video's quality window starts, and whether it opens at all.
//! - [`desktop`]: the commands and events between the desktop UI and its Rust shell.
//! - [`frame`]: length-prefixed JSON framing used on every local connection.
//! - [`version`]: the protocol versions each end speaks, and the one a connection agrees on.
//! - [`secret`]: values that grant access, which cross the wire but never print.
//! - [`secrets`]: what a secret is, for the log's redaction and the extension's.

mod exported;

pub mod app;
pub mod desktop;
pub mod extension;
pub mod frame;
pub mod languages;
pub mod media_start;
pub mod proxy;
pub mod rules;
pub mod secret;
pub mod secrets;
pub mod version;
pub mod wire;

pub use secret::{Link, Secret};
pub use version::{MIN_PROTOCOL_VERSION, PROTOCOL_VERSION};

use serde::{Deserialize, Serialize};
use ts_rs::TS;

exported::exported_consts! {
    /// The default queue, which always exists and cannot be deleted: downloads go to it when
    /// nothing names another queue.
    pub const DEFAULT_QUEUE_ID: QueueId = QueueId(1);
}

/// Identifies one download for its whole life.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct DownloadId(#[ts(type = "number & { readonly __brand: \"DownloadId\" }")] pub i64);

/// Identifies one queue.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct QueueId(#[ts(type = "number & { readonly __brand: \"QueueId\" }")] pub i64);

/// Identifies one proxy server (D73).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ProxyServerId(#[ts(type = "number & { readonly __brand: \"ProxyServerId\" }")] pub i64);

/// Identifies one question the app asks the person about a browser download: a file to
/// confirm ([`app::PendingCapture`]), media being read ([`app::MediaReading`]), a quality to pick
/// ([`app::MediaChoice`]) or a page waiting for the media add-on ([`app::AddonRequestItem`]).
/// They share one id space, and a question keeps its id from one step to the next. Never a
/// download id.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct QuestionId(#[ts(type = "number & { readonly __brand: \"QuestionId\" }")] pub u64);

/// A moment, as milliseconds since the Unix epoch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct UnixMillis(#[ts(type = "number & { readonly __brand: \"UnixMillis\" }")] pub i64);

impl UnixMillis {
    /// `by` later, or the last moment there is.
    #[must_use]
    pub fn saturating_add(self, by: std::time::Duration) -> Self {
        Self(
            self.0
                .saturating_add(i64::try_from(by.as_millis()).unwrap_or(i64::MAX)),
        )
    }

    /// How long after `earlier` this is; zero when it is not later.
    #[must_use]
    pub fn since(self, earlier: Self) -> std::time::Duration {
        std::time::Duration::from_millis(
            u64::try_from(self.0.saturating_sub(earlier.0)).unwrap_or(0),
        )
    }
}
