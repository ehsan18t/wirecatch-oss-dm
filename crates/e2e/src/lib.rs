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

#![expect(
    clippy::expect_used,
    clippy::missing_panics_doc,
    reason = "test support: a panic here is the failing test's report, as clippy.toml already allows inside tests, and the whole crate is only ever used by tests"
)]

pub mod app;
pub mod bridge;
pub mod extension;
pub mod server;

pub use wirecatch_test_support::PATIENCE;
