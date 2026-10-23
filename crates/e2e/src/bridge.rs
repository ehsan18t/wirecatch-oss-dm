//! The `wirecatch-bridge` executable under test, built with its `test-overrides`
//! feature so it can be pointed at a private endpoint and a stand-in app, and the
//! variables those overrides read.

pub use wirecatch_test_support::bridge::{ENDPOINT_VAR, LAUNCH_VAR, START_BUDGET_VAR, bridge_exe};
