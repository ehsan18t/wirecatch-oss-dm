//! Extension identities and the native host name, embedded at compile time from
//! `config/extension-ids.json` (the file the extension build reads too).
//!
//! Every bridge build trusts exactly one Chromium extension id, the one that
//! follows from the extension's public key, and the Firefox add-on id (D22). The
//! same id is used by unpacked development builds and by the store, because the
//! store upload carries the same key.

use anyhow::{Context, bail};
use serde::Deserialize;

/// The shared identity file, as it was when the bridge was built.
const EMBEDDED: &str = include_str!("../../../config/extension-ids.json");

/// The file's shape. Unknown fields are an error on purpose: when an id is added
/// to the file, this struct (and the allow list built from it) must be updated
/// too, and the unit test below fails until it is.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ExtensionIds {
    #[serde(rename = "$comment", default)]
    _comment: Option<String>,
    chromium_id: String,
    /// Used by the extension build to pin its id; the bridge does not need it.
    #[serde(rename = "chromium_public_key")]
    _chromium_public_key: String,
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
        Self::parse(EMBEDDED).context("The extension id list built into the bridge is invalid")
    }

    /// Reads and checks the identity file.
    fn parse(raw: &str) -> anyhow::Result<Self> {
        let ids: ExtensionIds = serde_json::from_str(raw)?;
        if !is_chromium_id(&ids.chromium_id) {
            bail!("{:?} is not a Chromium extension id", ids.chromium_id);
        }
        if !is_firefox_id(&ids.firefox_id) {
            bail!("{:?} is not a Firefox add-on id", ids.firefox_id);
        }
        if !is_host_name(&ids.native_host_name) {
            bail!("{:?} is not a valid native host name", ids.native_host_name);
        }
        Ok(Self {
            native_host_name: ids.native_host_name,
            chromium_ids: vec![ids.chromium_id],
            firefox_ids: vec![ids.firefox_id],
        })
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
    fn every_build_trusts_the_one_chromium_id_and_the_firefox_id() {
        // No cargo feature changes this: a release bridge built with default
        // features has exactly this configuration.
        let config = Config::embedded().unwrap();
        assert_eq!(config.native_host_name, "app.wirecatch.bridge");
        assert_eq!(config.chromium_ids, ["chdgnmpjoipbgfeldicipccjjghmagki"]);
        assert_eq!(config.firefox_ids, ["extension@wirecatch.app"]);
        assert_eq!(
            config.chromium_origins(),
            ["chrome-extension://chdgnmpjoipbgfeldicipccjjghmagki/"]
        );
    }

    #[test]
    fn a_malformed_chromium_id_is_an_error() {
        let raw = EMBEDDED.replace("chdgnmpjoipbgfeldicipccjjghmagki", "not-an-id");
        assert!(Config::parse(&raw).is_err());
    }

    #[test]
    fn an_unknown_field_is_an_error_so_new_ids_are_never_ignored() {
        let raw = EMBEDDED.replacen('{', r#"{"chromium_store_id": "x","#, 1);
        assert!(Config::parse(&raw).is_err());
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
