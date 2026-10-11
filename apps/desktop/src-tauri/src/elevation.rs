//! Refuses to run with administrator rights. An elevated app could be driven by any program of the same user through its endpoint, and the browser extension, which runs unelevated, could not reach it anyway (docs/audit/ipc-bridge.md, IPB-1).

use std::io;

/// Shown when Wirecatch is started with administrator rights.
pub const ELEVATED_TITLE: &str = "Wirecatch does not need administrator rights";
/// The explanation under [`ELEVATED_TITLE`].
pub const ELEVATED_MESSAGE: &str = "Wirecatch was started as administrator, which would let other programs on this computer control it. Close this message and start Wirecatch normally.";

/// What to do about how the process was started.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Start {
    /// A normal start.
    Continue,
    /// Started elevated: say why and exit.
    Refuse,
}

/// Decides from [`wirecatch_ipc::is_elevated`]. When Windows cannot say, the app starts: its endpoint is labelled at its own integrity level, so unelevated programs stay out even if the process were elevated.
pub fn decide(elevated: io::Result<bool>) -> Start {
    match elevated {
        Ok(true) => Start::Refuse,
        Ok(false) => Start::Continue,
        Err(e) => {
            tracing::warn!("could not tell whether Wirecatch runs as administrator: {e}");
            Start::Continue
        }
    }
}

/// Shows the refusal. A message box, because nothing else of the app exists yet.
pub fn explain_refusal() {
    tracing::error!("{ELEVATED_TITLE}: {ELEVATED_MESSAGE}");
    #[cfg(windows)]
    message_box(ELEVATED_TITLE, ELEVATED_MESSAGE);
}

#[cfg(windows)]
fn message_box(title: &str, text: &str) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{MB_ICONWARNING, MB_OK, MessageBoxW};

    let wide = |s: &str| {
        s.encode_utf16()
            .chain(std::iter::once(0))
            .collect::<Vec<u16>>()
    };
    let (title, text) = (wide(title), wide(text));
    // SAFETY: both buffers are NUL-terminated UTF-16 that outlive the call, and a null owner window is allowed.
    unsafe {
        MessageBoxW(
            std::ptr::null_mut(),
            text.as_ptr(),
            title.as_ptr(),
            MB_OK | MB_ICONWARNING,
        )
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_elevated_start_is_refused() {
        assert_eq!(decide(Ok(true)), Start::Refuse);
        assert_eq!(decide(Ok(false)), Start::Continue);
    }

    #[test]
    fn an_unknown_elevation_still_starts() {
        assert_eq!(decide(Err(io::Error::other("no token"))), Start::Continue);
    }

    #[test]
    fn the_refusal_is_plain_language() {
        for text in [ELEVATED_TITLE, ELEVATED_MESSAGE] {
            assert!(!text.contains('\u{2014}'), "{text}");
        }
        assert!(ELEVATED_MESSAGE.ends_with('.'));
    }
}
