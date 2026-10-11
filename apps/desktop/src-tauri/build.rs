fn main() {
    tauri_build::build();

    // Tauri embeds a manifest in the app that asks Windows for version 6 of the common controls,
    // which the dialog code needs. Test executables get none, so a test that links that code
    // fails to start (STATUS_ENTRYPOINT_NOT_FOUND). Give integration tests the same dependency.
    let windows_msvc = std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows")
        && std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc");
    if windows_msvc {
        let dir = std::env::var("CARGO_MANIFEST_DIR").unwrap_or_default();
        let manifest = std::path::Path::new(&dir)
            .join("windows")
            .join("tests.manifest");
        println!("cargo:rerun-if-changed={}", manifest.display());
        println!("cargo:rustc-link-arg-tests=/MANIFEST:EMBED");
        println!(
            "cargo:rustc-link-arg-tests=/MANIFESTINPUT:{}",
            manifest.display()
        );
    }
}
