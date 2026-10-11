//! The `wirecatch-bridge` executable under test, built with its `test-overrides`
//! feature so it can be pointed at a private endpoint and a stand-in app.
//!
//! Cargo cannot build another package's binary for an integration test on stable
//! (artifact dependencies are unstable), so the first test that needs the bridge
//! runs `cargo build -p wirecatch-bridge --features test-overrides` into the same
//! target folder the tests were built in. When nothing changed that is a no-op
//! taking well under a second. Set `WIRECATCH_E2E_BRIDGE` to the path of an
//! already built bridge (with that feature) to skip the build.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::OnceLock;

/// Environment variable naming a prebuilt bridge to use instead of building one.
pub const BRIDGE_VAR: &str = "WIRECATCH_E2E_BRIDGE";

/// Read by the bridge (with `test-overrides`): the endpoint to connect to.
pub const ENDPOINT_VAR: &str = "WIRECATCH_BRIDGE_TEST_ENDPOINT";

/// Read by the bridge (with `test-overrides`): a JSON array `[program, args...]`
/// started instead of the app.
pub const LAUNCH_VAR: &str = "WIRECATCH_BRIDGE_TEST_LAUNCH";

/// The extension identities the bridge was built with, read from the same file.
const EXTENSION_IDS: &str = include_str!("../../../config/extension-ids.json");

/// The origin Chrome passes for the extension (the one Chromium id every build
/// trusts), for example `chrome-extension://<id>/`.
pub fn chrome_origin() -> String {
    format!("chrome-extension://{}/", extension_id("chromium_id"))
}

/// The add-on id Firefox passes for the extension.
pub fn firefox_id() -> String {
    extension_id("firefox_id")
}

fn extension_id(key: &str) -> String {
    let ids: serde_json::Value =
        serde_json::from_str(EXTENSION_IDS).expect("config/extension-ids.json is valid JSON");
    ids[key]
        .as_str()
        .unwrap_or_else(|| panic!("config/extension-ids.json has no {key}"))
        .to_string()
}

/// The path of a bridge built with `test-overrides`, building it on first use.
pub fn bridge_exe() -> &'static Path {
    static BRIDGE: OnceLock<PathBuf> = OnceLock::new();
    BRIDGE.get_or_init(|| match std::env::var_os(BRIDGE_VAR) {
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
        .expect("could not run cargo to build wirecatch-bridge");
    assert!(
        output.status.success(),
        "building wirecatch-bridge with test-overrides failed:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    stdout
        .lines()
        .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
        .filter(|m| m["reason"] == "compiler-artifact" && m["target"]["name"] == "wirecatch-bridge")
        .find_map(|m| m["executable"].as_str().map(PathBuf::from))
        .unwrap_or_else(|| panic!("cargo built no wirecatch-bridge executable:\n{stdout}"))
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
