//! Build script: Tauri's code generation (which also decides how the Visual C++ runtime is linked), and the Windows manifest the integration tests need.

/// The empty library Tauri's static runtime switch writes to replace the dynamic runtime's import library.
const STATIC_RUNTIME_STUB: &str = "msvcrt.lib";

fn main() {
    let windows_msvc = std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows")
        && std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc");

    // How the Visual C++ runtime is linked is `build.windows.staticVCRuntime`: off in tauri.conf.json, on in tauri.bundle.conf.json, which every packaging build (`bundle` and `build:tauri`) merges in, so every installer's app links it statically and needs no runtime installer. When it is on, Tauri writes an empty `msvcrt.lib` into this crate's build folder, and cargo gives every doctest in the same build that folder's link paths, so with the desktop crate in a workspace test run no other crate's doctest could link. The stub is removed first, so only a build with the switch on has one (Tauri writes it again then). The deprecated `STATIC_VCRUNTIME` variable overrides the configuration and must stay unset.
    if windows_msvc {
        remove_static_runtime_stub();
    }
    tauri_build::build();

    // Tauri embeds a manifest in the app that asks Windows for version 6 of the common controls,
    // which the dialog code needs. Test executables get none, so a test that links that code
    // fails to start (STATUS_ENTRYPOINT_NOT_FOUND). Give integration tests the same dependency.
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

/// Removes the empty `msvcrt.lib` an earlier build with the static runtime left in this build folder: it would shadow the real one and no binary of this crate could link.
fn remove_static_runtime_stub() {
    let Some(out_dir) = std::env::var_os("OUT_DIR") else {
        return;
    };
    let stub = std::path::Path::new(&out_dir).join(STATIC_RUNTIME_STUB);
    if let Err(error) = std::fs::remove_file(&stub)
        && error.kind() != std::io::ErrorKind::NotFound
    {
        panic!("could not remove {}: {error}", stub.display());
    }
}
