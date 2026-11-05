//! A system message, for what must be said before the app has a window of its own: a box on
//! Windows. Linux has none without a toolkit, so there it is a notification (D103).

/// How the box is marked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tone {
    Information,
    Warning,
}

/// Shows `text` under `title`. A box waits until the person closes it.
pub fn show(tone: Tone, title: &str, text: &str) {
    tracing::info!("{title}: {text}");
    #[cfg(target_os = "linux")]
    if let Err(e) = notify_rust::Notification::new()
        .appname(title)
        .summary(title)
        .body(&crate::system_notifications::escape_text(text))
        .show()
    {
        tracing::warn!("could not show a notification: {e}");
    }
    #[cfg(windows)]
    {
        use windows_sys::Win32::UI::WindowsAndMessaging::{
            MB_ICONINFORMATION, MB_ICONWARNING, MB_OK, MessageBoxW,
        };
        use wirecatch_ipc::wide;

        let (title, text) = (wide(title), wide(text));
        let icon = match tone {
            Tone::Information => MB_ICONINFORMATION,
            Tone::Warning => MB_ICONWARNING,
        };
        // SAFETY: both buffers are NUL-terminated UTF-16 that outlive the call, and a null owner window is allowed.
        unsafe {
            MessageBoxW(
                std::ptr::null_mut(),
                text.as_ptr(),
                title.as_ptr(),
                MB_OK | icon,
            )
        };
    }
    #[cfg(not(windows))]
    let _ = tone;
}
