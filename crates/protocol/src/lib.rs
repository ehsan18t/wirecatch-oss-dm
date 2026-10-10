//! Types shared by every Wirecatch component.
//!
//! These Rust types are the single source of truth. TypeScript definitions for the
//! desktop UI and the browser extension are generated from them with `ts-rs` into
//! `packages/protocol/src/generated` (run `cargo test -p wirecatch-protocol`).
//!
//! - [`wire`]: messages exchanged between the extension, the bridge and the app.
//! - [`extension`]: what the extension sends and the settings it can change.
//! - [`app`]: downloads, queues and settings as the desktop UI sees them.
//! - [`frame`]: length-prefixed JSON framing used on every local connection.

pub mod app;
pub mod extension;
pub mod frame;
pub mod wire;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Bumped on any incompatible change to the wire format.
pub const PROTOCOL_VERSION: u32 = 1;

/// Identifies one download for its whole life.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct DownloadId(#[ts(type = "number")] pub i64);

/// Identifies one queue.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct QueueId(#[ts(type = "number")] pub i64);
