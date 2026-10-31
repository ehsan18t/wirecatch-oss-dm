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

#[cfg(test)]
mod tests {
    #[cfg(windows)]
    #[test]
    fn the_running_windows_says_its_accent() {
        assert!(super::system_accent().is_some());
    }
}
