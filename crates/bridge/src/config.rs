//! Extension identities and the native host name, embedded at compile time from
//! `config/extension-ids.json` (the file the extension build reads too).
//!
//! The development build of the Chromium extension has a fixed id whose private
//! key is in the repository, so anyone can sign an extension with it. Only bridges
//! built with the `dev-extension` feature trust it (`cargo build -p
//! wirecatch-bridge --features dev-extension`); release bridges trust store ids
//! only. The `test-overrides` feature turns it on for the end-to-end tests.

use anyhow::{Context, bail};
use serde::Deserialize;

/// The shared identity file, as it was when the bridge was built.
const EMBEDDED: &str = include_str!("../../../config/extension-ids.json");

/// Whether this build trusts the development build of the Chromium extension.
const TRUST_DEV_EXTENSION: bool = cfg!(feature = "dev-extension");

/// Said when a bridge trusts no Chromium extension, at startup and on install.
pub(crate) const NO_CHROMIUM_IDS: &str = "This bridge trusts no Chromium extension, so Chrome, Edge, Brave and other Chromium browsers cannot use it (Firefox can). It was built without the dev-extension feature and no store id is listed yet. For development, build it with `cargo build -p wirecatch-bridge --features dev-extension`";

/// The file's shape. Unknown fields are an error on purpose: when a store id is
/// added to the file, this struct (and the allow list built from it) must be
/// updated too, and the unit test below fails until it is.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ExtensionIds {
    #[serde(rename = "$comment", default)]
    _comment: Option<String>,
    chromium_dev_id: String,
    /// Used by the extension build to pin its id; the bridge does not need it.
    #[serde(rename = "chromium_dev_public_key")]
    _chromium_dev_public_key: String,
    firefox_id: String,
    native_host_name: String,
}

/// Who may start the bridge, and the name it registers under.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Config {
    /// The native messaging host name, for example `app.wirecatch.bridge`.
    pub(crate) native_host_name: String,
    /// Allowed Chromium extension ids (32 letters `a` to `p`).
    pub(crate) chromium_ids: Vec<String>,
    /// Allowed Firefox add-on ids.
    pub(crate) firefox_ids: Vec<String>,
}

impl Config {
    /// The configuration built into this binary.
    pub(crate) fn embedded() -> anyhow::Result<Self> {
        Self::parse(EMBEDDED, TRUST_DEV_EXTENSION)
            .context("The extension id list built into the bridge is invalid")
    }

    /// The embedded configuration with the development Chromium id trusted, as a
    /// `dev-extension` build has it, for tests that need a Chromium caller.
    #[cfg(test)]
    pub(crate) fn with_dev_extension() -> Self {
        Self::parse(EMBEDDED, true).unwrap()
    }

    /// Reads the identity file. The development Chromium id is always checked,
    /// but trusted only when `trust_dev_extension` is set.
    fn parse(raw: &str, trust_dev_extension: bool) -> anyhow::Result<Self> {
        let ids: ExtensionIds = serde_json::from_str(raw)?;
        if !is_chromium_id(&ids.chromium_dev_id) {
            bail!("{:?} is not a Chromium extension id", ids.chromium_dev_id);
        }
        if !is_firefox_id(&ids.firefox_id) {
            bail!("{:?} is not a Firefox add-on id", ids.firefox_id);
        }
        if !is_host_name(&ids.native_host_name) {
            bail!("{:?} is not a valid native host name", ids.native_host_name);
        }
        Ok(Self {
            native_host_name: ids.native_host_name,
            chromium_ids: if trust_dev_extension {
                vec![ids.chromium_dev_id]
            } else {
                Vec::new()
            },
            firefox_ids: vec![ids.firefox_id],
        })
    }

    /// Why Chromium browsers cannot use this bridge, when it trusts no Chromium
    /// extension at all; None when it trusts at least one.
    pub(crate) fn chromium_problem(&self) -> Option<&'static str> {
        self.chromium_ids.is_empty().then_some(NO_CHROMIUM_IDS)
    }

    /// The origins Chromium browsers pass for the allowed extensions, in the exact
    /// form they use (`chrome-extension://<id>/`).
    pub(crate) fn chromium_origins(&self) -> Vec<String> {
        self.chromium_ids
            .iter()
            .map(|id| chromium_origin(id))
            .collect()
    }
}

/// The origin string Chromium uses for an extension id.
pub(crate) fn chromium_origin(id: &str) -> String {
    format!("chrome-extension://{id}/")
}

/// Chromium ids are 32 characters from `a` to `p`.
fn is_chromium_id(id: &str) -> bool {
    id.len() == 32 && id.bytes().all(|b| (b'a'..=b'p').contains(&b))
}

/// Firefox ids are either email-like (`name@domain`) or a braced GUID. Only the
/// characters those forms use are accepted.
fn is_firefox_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 255
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._-@{}".contains(&b))
}

/// Chromium's rule: lowercase letters, digits, `_` and `.`, not starting or
/// ending with a dot, and no two dots in a row.
fn is_host_name(name: &str) -> bool {
    !name.is_empty()
        && !name.starts_with('.')
        && !name.ends_with('.')
        && !name.contains("..")
        && name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_' || b == b'.')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_embedded_file_is_valid_and_complete() {
        let config = Config::with_dev_extension();
        assert_eq!(config.native_host_name, "app.wirecatch.bridge");
        assert_eq!(config.chromium_ids, ["chdgnmpjoipbgfeldicipccjjghmagki"]);
        assert_eq!(config.firefox_ids, ["extension@wirecatch.app"]);
        assert_eq!(
            config.chromium_origins(),
            ["chrome-extension://chdgnmpjoipbgfeldicipccjjghmagki/"]
        );
        assert_eq!(config.chromium_problem(), None);
    }

    #[test]
    fn only_a_dev_extension_build_trusts_the_development_chromium_id() {
        let release = Config::parse(EMBEDDED, false).unwrap();
        assert!(release.chromium_ids.is_empty());
        assert!(release.chromium_origins().is_empty());
        assert_eq!(release.firefox_ids, ["extension@wirecatch.app"]);
        assert_eq!(release.chromium_problem(), Some(NO_CHROMIUM_IDS));

        let embedded = Config::embedded().unwrap();
        assert_eq!(
            embedded.chromium_ids.is_empty(),
            !cfg!(feature = "dev-extension"),
            "the embedded configuration must follow the dev-extension feature"
        );
    }

    #[test]
    fn a_malformed_development_id_is_an_error_even_when_not_trusted() {
        let raw = EMBEDDED.replace("chdgnmpjoipbgfeldicipccjjghmagki", "not-an-id");
        assert!(Config::parse(&raw, false).is_err());
    }

    #[test]
    fn an_unknown_field_is_an_error_so_new_ids_are_never_ignored() {
        let raw = EMBEDDED.replacen('{', r#"{"chromium_store_id": "x","#, 1);
        assert!(Config::parse(&raw, true).is_err());
    }

    #[test]
    fn malformed_ids_are_rejected() {
        assert!(!is_chromium_id("chdgnmpjoipbgfeldicipccjjghmagkz"));
        assert!(!is_chromium_id("short"));
        assert!(is_firefox_id("{12345678-1234-1234-1234-123456789abc}"));
        assert!(!is_firefox_id("evil id"));
        assert!(!is_firefox_id(""));
        assert!(is_host_name("app.wirecatch.bridge"));
        assert!(!is_host_name("App.Wirecatch"));
        assert!(!is_host_name(".app"));
        assert!(!is_host_name("app..bridge"));
    }
}
