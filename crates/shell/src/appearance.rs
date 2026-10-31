//! What the system says about how the app should look.

/// The accent colour chosen in Windows, as red, green and blue; `None` when the system does not
/// say.
#[cfg(windows)]
#[must_use]
pub fn system_accent() -> Option<[u8; 3]> {
    use windows::UI::ViewManagement::{UIColorType, UISettings};
    let colour = UISettings::new()
        .and_then(|settings| settings.GetColorValue(UIColorType::Accent))
        .inspect_err(|e| tracing::debug!("could not read the system accent colour: {e}"))
        .ok()?;
    Some([colour.R, colour.G, colour.B])
}

/// Linux desktops have no accent colour Wirecatch can read.
#[cfg(not(windows))]
#[must_use]
pub fn system_accent() -> Option<[u8; 3]> {
    None
}

/// Draws the app's native popup menus (the tray's) in the dark scheme or the light one.
#[cfg(windows)]
pub fn set_menu_scheme(dark: bool) {
    use windows_sys::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};

    // Popup menus have no documented dark mode: uxtheme exports these two by ordinal only
    // (Windows 10 1903 and later), the calls Explorer itself makes.
    const SET_PREFERRED_APP_MODE: usize = 135;
    const FLUSH_MENU_THEMES: usize = 136;
    const FORCE_DARK: i32 = 2;
    const FORCE_LIGHT: i32 = 3;

    let name = wirecatch_ipc::wide("uxtheme.dll");
    // SAFETY: `name` is NUL-terminated UTF-16 that outlives the call.
    let module = unsafe { LoadLibraryW(name.as_ptr()) };
    if module.is_null() {
        return;
    }
    // SAFETY: an ordinal in the low word stands for a name, as GetProcAddress documents.
    let (set, flush) = unsafe {
        (
            GetProcAddress(module, SET_PREFERRED_APP_MODE as *const u8),
            GetProcAddress(module, FLUSH_MENU_THEMES as *const u8),
        )
    };
    let (Some(set), Some(flush)) = (set, flush) else {
        tracing::debug!("this Windows cannot draw menus dark");
        return;
    };
    // SAFETY: the two exports take these signatures, `SetPreferredAppMode(PreferredAppMode)` and
    // `FlushMenuThemes()`, and uxtheme stays loaded for the life of the process.
    unsafe {
        let set = std::mem::transmute::<
            unsafe extern "system" fn() -> isize,
            unsafe extern "system" fn(i32) -> i32,
        >(set);
        let flush = std::mem::transmute::<
            unsafe extern "system" fn() -> isize,
            unsafe extern "system" fn(),
        >(flush);
        set(if dark { FORCE_DARK } else { FORCE_LIGHT });
        flush();
    }
}

/// Linux menus are drawn by the desktop, in its own scheme.
#[cfg(not(windows))]
pub fn set_menu_scheme(_dark: bool) {}

#[cfg(test)]
mod tests {
    #[cfg(windows)]
    #[test]
    fn the_running_windows_says_its_accent() {
        assert!(super::system_accent().is_some());
    }

    #[cfg(windows)]
    #[test]
    fn menus_switch_scheme_both_ways() {
        super::set_menu_scheme(true);
        super::set_menu_scheme(false);
    }
}
