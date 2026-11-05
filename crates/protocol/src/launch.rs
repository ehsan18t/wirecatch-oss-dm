//! How the bridge starts the app, and what tests set to point a bridge built with `test-overrides` at their own.

/// The flag the bridge and the start-with-system entry pass: run with no window.
pub const BACKGROUND_FLAG: &str = "--background";

/// The endpoint to connect to, in place of the app's.
pub const ENDPOINT_VAR: &str = "WIRECATCH_BRIDGE_TEST_ENDPOINT";

/// A JSON array `[program, args...]` started in place of the app.
pub const LAUNCH_VAR: &str = "WIRECATCH_BRIDGE_TEST_LAUNCH";

/// How long, in milliseconds, to wait for an app the bridge started to listen.
pub const START_BUDGET_VAR: &str = "WIRECATCH_BRIDGE_TEST_START_BUDGET_MS";
