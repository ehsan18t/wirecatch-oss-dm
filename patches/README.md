# Temporary Rust dependency patches

`node scripts/prepare-rust-backports.mjs` verifies the pinned crates.io archives, applies the patches, and makes the resulting crates available to Cargo. The generated source lives under `.cache/rust-backports`.

- `gpui-pre-windows` 0.3.12 comes from Zed revision `d2a4fb6ded7c470be25d1541281da3882495b65b`. The patch disconnects activation callbacks before native teardown and stops redraws of destroyed windows. [Upstream issue](https://github.com/zed-industries/zed/issues/67282)
- `accesskit_windows` 0.34.0 comes from AccessKit revision `c88605b96d04431f9c3c792464a0f2f253480e94`. The patch returns the UI Automation element-unavailable error for a closed window. The licence texts come from the same revision. [Crate release](https://crates.io/crates/accesskit_windows/0.34.0)

Once a compatible release contains a fix, remove its patch, its entry in the preparation script, and its Cargo override and workspace exclusion. Remove the extra Windows activation test command in `scripts/gate/rust.mjs` with the GPUI patch.
