//! Refuses a start with "Run as administrator". The app would hold rights the programs talking to it do not, and the browser extension, which runs without them, could not reach it anyway (docs/audit/ipc-bridge.md, IPB-1). An account that is always elevated (user account control off, or the built-in Administrator) is not refused: its browser runs elevated too, and there is no normal start to ask for.

use std::io;

use wirecatch_ipc::Elevation;

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

/// Decides from [`wirecatch_ipc::elevation`]. When Windows cannot say, the app starts: its endpoint is labelled at its own integrity level, so unelevated programs stay out even if the process were elevated.
pub fn decide(elevation: io::Result<Elevation>) -> Start {
    match elevation {
        Ok(Elevation::Elevated) => Start::Refuse,
        Ok(Elevation::Standard) => Start::Continue,
        Ok(Elevation::AlwaysAdministrator) => {
            tracing::info!("this account always runs with administrator rights");
            Start::Continue
        }
        Err(e) => {
            tracing::warn!("could not tell whether Wirecatch runs as administrator: {e}");
            Start::Continue
        }
    }
}

/// Shows the refusal. A message box, because nothing else of the app exists yet.
pub fn explain_refusal() {
    tracing::error!("{ELEVATED_TITLE}: {ELEVATED_MESSAGE}");
    crate::message_box::show(
        crate::message_box::Tone::Warning,
        ELEVATED_TITLE,
        ELEVATED_MESSAGE,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_elevated_start_is_refused() {
        assert_eq!(decide(Ok(Elevation::Elevated)), Start::Refuse);
        assert_eq!(decide(Ok(Elevation::Standard)), Start::Continue);
    }

    // With user account control off, or as the built-in Administrator, every start is elevated, so refusing would lock the person out.
    #[test]
    fn an_account_that_is_always_elevated_still_starts() {
        assert_eq!(decide(Ok(Elevation::AlwaysAdministrator)), Start::Continue);
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
