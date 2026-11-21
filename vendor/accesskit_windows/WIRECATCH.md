# AccessKit Windows backport

This is the Windows adapter from `accesskit_windows` 0.34.0, published
from AccessKit revision `c88605b96d04431f9c3c792464a0f2f253480e94`. The
upstream crate archive has SHA-256
`106c2b961215864d1c2e703ee63269c25c4e80a577ffb2c1017b9c17dcdf83a1`.
The MIT and Apache-2.0 licence texts are from that same revision.

UI Automation can still hold a provider while Windows closes its HWND.
The backport turns the invalid-handle response during bounds and hit
testing into the UI Automation element-unavailable error. Remove the
Cargo patch, workspace exclusion and this directory together when the
pinned GPUI stack carries the fix.

[Crate release](https://crates.io/crates/accesskit_windows/0.34.0)
