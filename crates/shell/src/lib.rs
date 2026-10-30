//! What the desktop app does besides drawing its windows, shared by every interface that hosts the
//! download manager (D13, D99): opening the manager, serving the browser bridge, start at sign-in,
//! refusing to run elevated, and each interface command as a plain function on the manager.

pub mod activity;
pub mod api;
pub mod appimage;
pub mod autostart;
pub mod bridge;
pub mod elevation;
pub mod host;
pub mod tray_proxy;
