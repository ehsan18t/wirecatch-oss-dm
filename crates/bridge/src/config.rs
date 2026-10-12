//! Extension identities and the native host name, from `config/product.toml`
//! through the `wirecatch-product` crate (the extension build reads the same
//! values), which checks their form when it is generated.
//!
//! Every bridge build trusts exactly one Chromium extension id, the one that
//! follows from the extension's public key, and the Firefox add-on id (D22). The
//! same id is used by unpacked development builds and by the store, because the
//! store upload carries the same key.

use wirecatch_product::{CHROMIUM_EXTENSION_ID, FIREFOX_EXTENSION_ID, NATIVE_HOST_NAME};

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
    pub(crate) fn embedded() -> Self {
        Self {
            native_host_name: NATIVE_HOST_NAME.to_owned(),
            chromium_ids: vec![CHROMIUM_EXTENSION_ID.to_owned()],
            firefox_ids: vec![FIREFOX_EXTENSION_ID.to_owned()],
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    /// The one place the ids are written out: changing which extensions every
    /// bridge trusts is a decision (D22), so it must change this test too.
    #[test]
    fn every_build_trusts_the_one_chromium_id_and_the_firefox_id() {
        // No cargo feature changes this: a release bridge built with default
        // features has exactly this configuration.
        let config = Config::embedded();
        assert_eq!(config.native_host_name, "app.wirecatch.bridge");
        assert_eq!(config.chromium_ids, ["chdgnmpjoipbgfeldicipccjjghmagki"]);
        assert_eq!(config.firefox_ids, ["extension@wirecatch.app"]);
        assert_eq!(
            config.chromium_origins(),
            ["chrome-extension://chdgnmpjoipbgfeldicipccjjghmagki/"]
        );
        assert_eq!(
            config.chromium_origins(),
            [wirecatch_product::CHROMIUM_EXTENSION_ORIGIN]
        );
    }
}
