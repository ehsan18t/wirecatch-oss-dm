//! Checks which extension started the bridge.
//!
//! The browser passes the caller on the command line:
//!
//! - Chromium browsers: `chrome-extension://<id>/` as the first argument; on
//!   Windows also `--parent-window=<handle>` after it.
//! - Firefox: the path of the host manifest, then the add-on id.
//!
//! Browsers only start a host for origins listed in its manifest, so this check is
//! defense in depth: it stops the bridge from serving a caller that is not one of
//! ours even if a manifest is edited or copied. It is not a boundary against local
//! programs running as the same user; those could reach the app socket directly.

use std::ffi::OsString;

use crate::config::{Config, chromium_origin};

/// An allowed caller.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Caller {
    /// A Chromium-based browser extension.
    Chromium {
        /// The extension id.
        id: String,
    },
    /// A Firefox add-on.
    Firefox {
        /// The add-on id.
        id: String,
    },
}

/// Identifies the caller from the arguments after the program name, or explains
/// why it is refused.
pub(crate) fn identify(args: &[OsString], config: &Config) -> Result<Caller, String> {
    let Some(first) = args.first() else {
        return Err("no caller was given (the browser passes the extension origin)".into());
    };
    let first = first
        .to_str()
        .ok_or_else(|| "the first argument is not valid text".to_string())?;

    if first.starts_with("chrome-extension://") {
        let id = config
            .chromium_ids
            .iter()
            .find(|id| chromium_origin(id) == first)
            .ok_or_else(|| format!("the extension origin {first:?} is not allowed"))?;
        // Chromium may add flags after the origin; anything else is unexpected.
        if let Some(extra) = args[1..]
            .iter()
            .find(|arg| !arg.to_str().is_some_and(|a| a.starts_with("--")))
        {
            return Err(format!(
                "unexpected argument {:?} after the origin",
                extra.to_string_lossy()
            ));
        }
        return Ok(Caller::Chromium { id: id.clone() });
    }

    // Firefox: exactly the manifest path and the add-on id.
    if args.len() != 2 {
        return Err(format!(
            "the arguments do not match what Chrome or Firefox pass ({} given)",
            args.len()
        ));
    }
    let addon = args[1]
        .to_str()
        .ok_or_else(|| "the add-on id is not valid text".to_string())?;
    let id = config
        .firefox_ids
        .iter()
        .find(|id| id.as_str() == addon)
        .ok_or_else(|| format!("the add-on id {addon:?} is not allowed"))?;
    Ok(Caller::Firefox { id: id.clone() })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The configuration every build has, release builds with default features
    /// included: no cargo feature changes which extensions are trusted.
    fn config() -> Config {
        Config::embedded().unwrap()
    }

    fn args(list: &[&str]) -> Vec<OsString> {
        list.iter().map(OsString::from).collect()
    }

    const CHROME: &str = "chrome-extension://chdgnmpjoipbgfeldicipccjjghmagki/";

    #[test]
    fn allowed_chromium_origin_is_accepted() {
        let expected = Caller::Chromium {
            id: "chdgnmpjoipbgfeldicipccjjghmagki".into(),
        };
        assert_eq!(identify(&args(&[CHROME]), &config()), Ok(expected.clone()));
        assert_eq!(
            identify(&args(&[CHROME, "--parent-window=1234"]), &config()),
            Ok(expected)
        );
    }

    #[test]
    fn allowed_firefox_addon_is_accepted() {
        assert_eq!(
            identify(
                &args(&[
                    "/home/u/.mozilla/native-messaging-hosts/app.wirecatch.bridge.json",
                    "extension@wirecatch.app"
                ]),
                &config()
            ),
            Ok(Caller::Firefox {
                id: "extension@wirecatch.app".into()
            })
        );
    }

    #[test]
    fn everything_else_is_refused() {
        let refused: &[&[&str]] = &[
            &[],
            &["chrome-extension://aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa/"],
            &["chrome-extension://chdgnmpjoipbgfeldicipccjjghmagki"],
            &["chrome-extension://chdgnmpjoipbgfeldicipccjjghmagki/x"],
            &["CHROME-EXTENSION://chdgnmpjoipbgfeldicipccjjghmagki/"],
            &[CHROME, "install"],
            &["manifest.json", "evil@example.com"],
            &["manifest.json", "EXTENSION@wirecatch.app"],
            &["manifest.json"],
            &["manifest.json", "extension@wirecatch.app", "extra"],
            &["moz-extension://extension@wirecatch.app/"],
            &["install"],
        ];
        for list in refused {
            assert!(identify(&args(list), &config()).is_err(), "{list:?}");
        }
    }
}
