# GPUI Windows backport

This is the Windows backend from `gpui-pre-windows` 0.3.12, published
from Zed revision `d2a4fb6ded7c470be25d1541281da3882495b65b`. The upstream
crate archive has SHA-256
`7201762420222d5b5ce5bdf0056996199212a3349bfb2457a3a0238c688681f3`.
The licence is in `LICENSE-APACHE`.

The local changes prevent Win32 activation callbacks from reaching a window
after GPUI removes it. When the pinned GPUI snapshot includes the lifecycle
fix, remove the Cargo patch, the workspace exclusion, this directory and
the backport test command in `scripts/gate/rust.mjs`.

[Crate release](https://crates.io/crates/gpui-pre-windows/0.3.12) · [Zed report](https://github.com/zed-industries/zed/issues/67282)
