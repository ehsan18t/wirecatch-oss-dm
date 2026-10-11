//! Shared helpers for Wirecatch's end-to-end tests (`tests/e2e.rs`).
//!
//! The pieces are real wherever it matters: a [`wirecatch_core::Manager`] in a
//! temporary folder serving a real [`wirecatch_ipc::Listener`] ([`app::App`]), the
//! real `wirecatch-bridge` executable started the way a browser starts it
//! ([`extension::FakeExtension`]), and a local HTTP server ([`server::TestServer`]).
//! Nothing here reaches the internet.
//!
//! - [`bridge`]: finds or builds the bridge with its test overrides.
//! - [`app`]: a private folder and endpoint per test, and the app side.
//! - [`extension`]: a scripted extension speaking native messaging to the bridge.
//! - [`server`]: the HTTP server, which can hold its answers until released.

#![warn(missing_docs)]

pub mod app;
pub mod bridge;
pub mod extension;
pub mod server;

use std::time::Duration;

/// How long any single wait in a test may take before the test fails.
pub const PATIENCE: Duration = Duration::from_secs(10);
