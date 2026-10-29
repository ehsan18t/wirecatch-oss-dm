//! The `wirecatch-bridge` executable built with its `test-overrides` feature, for tests that start it the way a browser does, pointed at a private endpoint and a stand-in app.
//!
//! The gate names the bridge its workspace test build already made, which has that feature, in [`BRIDGE_VARIABLE`]. Without it, the first call in a process runs `cargo build -p wirecatch-bridge --features test-overrides` into the target folder the test was built in: cargo cannot build another package's binary for an integration test on stable (artifact dependencies are unstable). That build resolves features apart from the workspace's and so relinks the executable the bridge's own tests run, which is harmless one test binary at a time but not while other processes run it.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::OnceLock;

use wirecatch_product::BRIDGE_EXECUTABLE;

/// Names a bridge already built with `test-overrides` to use instead of building one. The gate sets it (`scripts/gate/rust.mjs`).
pub const BRIDGE_VARIABLE: &str = "WIRECATCH_TEST_BRIDGE";

/// Read by the bridge (with `test-overrides`, `crates/bridge/src/app.rs`): the endpoint to connect to.
pub const ENDPOINT_VAR: &str = "WIRECATCH_BRIDGE_TEST_ENDPOINT";

/// Read by the bridge (with `test-overrides`): a JSON array `[program, args...]` started instead of the app.
pub const LAUNCH_VAR: &str = "WIRECATCH_BRIDGE_TEST_LAUNCH";

/// Read by the bridge (with `test-overrides`): how long, in milliseconds, to wait for an app it started to listen.
pub const START_BUDGET_VAR: &str = "WIRECATCH_BRIDGE_TEST_START_BUDGET_MS";

/// The path of a bridge built with `test-overrides`, building it on first use unless [`BRIDGE_VARIABLE`] names one.
#[must_use]
pub fn bridge_exe() -> &'static Path {
    static BRIDGE: OnceLock<PathBuf> = OnceLock::new();
    BRIDGE.get_or_init(|| match std::env::var_os(BRIDGE_VARIABLE) {
        Some(path) => PathBuf::from(path),
        None => build_bridge(),
    })
}

fn build_bridge() -> PathBuf {
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| OsString::from("cargo"));
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../Cargo.toml");
    let mut command = Command::new(cargo);
    command
        .args([
            "build",
            "-p",
            "wirecatch-bridge",
            "--features",
            "test-overrides",
            "--locked",
            "--message-format",
            "json-render-diagnostics",
            "--manifest-path",
        ])
        .arg(&manifest)
        .stdin(Stdio::null());
    if let Some((target_dir, release)) = this_target_dir() {
        command.arg("--target-dir").arg(target_dir);
        if release {
            command.arg("--release");
        }
    }
    let output = command
        .output()
        .unwrap_or_else(|e| panic!("could not run cargo to build {BRIDGE_EXECUTABLE}: {e}"));
    assert!(
        output.status.success(),
        "building {BRIDGE_EXECUTABLE} with test-overrides failed:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    stdout
        .lines()
        .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
        .filter(|m| m["reason"] == "compiler-artifact" && m["target"]["name"] == BRIDGE_EXECUTABLE)
        .find_map(|m| m["executable"].as_str().map(PathBuf::from))
        .unwrap_or_else(|| panic!("cargo built no {BRIDGE_EXECUTABLE} executable:\n{stdout}"))
}

/// The target folder this test executable was built in, and whether it is a
/// release build, so the bridge is built next to it instead of in a second
/// folder. The executable sits in `<target>/[<triple>/]<profile>/deps/`.
fn this_target_dir() -> Option<(PathBuf, bool)> {
    let exe = std::env::current_exe().ok()?;
    let profile_dir = exe.parent()?.parent()?;
    let release = profile_dir.file_name()? == "release";
    profile_dir
        .ancestors()
        .skip(1)
        .take(2)
        .find(|dir| dir.join("CACHEDIR.TAG").is_file())
        .map(|dir| (dir.to_path_buf(), release))
}
