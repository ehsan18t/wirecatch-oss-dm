//! What the desktop app does besides drawing its windows, shared by every interface that hosts the
//! download manager (D13, D99): opening the manager, serving the browser bridge, handing a second launch to the running copy, the tray menu, quitting, notifications, start at sign-in,
//! refusing to run elevated, and each interface command as a plain function on the manager.

pub mod activity;
pub mod api;
pub mod appimage;
pub mod autostart;
pub mod bridge;
pub mod browser_notices;
pub mod elevation;
pub mod host;
pub mod launch;
pub mod message_box;
pub mod notifications;
pub mod quit;
pub mod system_notifications;
pub mod teller;
pub mod tray;
pub mod tray_proxy;
