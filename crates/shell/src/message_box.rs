//! A system message box, for what must be said before the app has a window of its own. Only
//! Windows has one without a toolkit; elsewhere the text goes to the log alone.

/// How the box is marked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tone {
    Information,
    Warning,
}

/// Shows `text` under `title` and waits until the person closes it.
pub fn show(tone: Tone, title: &str, text: &str) {
    tracing::info!("{title}: {text}");
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
