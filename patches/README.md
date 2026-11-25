# Temporary Rust dependency patches

`node scripts/prepare-rust-backports.mjs` verifies the pinned crates.io archives, applies the patches, and makes the resulting crates available to Cargo. The generated source lives under `.cache/rust-backports` in directories named for the exact archive, patch, and licence inputs. After editing a patch, run setup and use the printed directory in the Cargo override and workspace exclusion.

- `gpui-pre-windows` 0.3.14 comes from Zed revision `f126950f9d3b62666f97437ae5d4ccac2272c4db`. The patch disconnects activation callbacks before native teardown and stops redraws of destroyed windows. [Upstream issue](https://github.com/zed-industries/zed/issues/67282)
- `accesskit_windows` 0.34.0 comes from AccessKit revision `c88605b96d04431f9c3c792464a0f2f253480e94`. The patch returns the UI Automation element-unavailable error for a closed window. The licence texts come from the same revision. [Crate release](https://crates.io/crates/accesskit_windows/0.34.0)

Once a compatible release contains a fix, remove its patch, its entry in the preparation script, and its Cargo override and workspace exclusion. Remove its extra Windows test command from `scripts/gate/rust.mjs` as well.
