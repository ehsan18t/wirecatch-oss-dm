//! What the browser extension hands over, and the settings it can change (D5).

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::{DownloadId, QueueId};

/// A download captured in the browser.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct NewDownload {
    pub url: String,
    pub referrer: Option<String>,
    /// The page the download started from.
    pub page_url: Option<String>,
    /// Name the browser or page suggested; the server's name wins if this is absent.
    pub suggested_name: Option<String>,
    pub mime: Option<String>,
    #[ts(type = "number | null")]
    pub size_hint: Option<u64>,
    /// Value for the `Cookie` header, so downloads behind a login keep working.
    pub cookies: Option<String>,
    pub user_agent: Option<String>,
    pub extra_headers: Vec<Header>,
    pub source: CaptureSource,
    /// Queue to add to; the default queue if absent.
    pub queue: Option<QueueId>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Header {
    pub name: String,
    pub value: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum CaptureSource {
    /// The browser started a download and the extension took it over.
    BrowserDownload,
    ContextMenu,
    /// The download button shown over a video or audio element.
    MediaButton,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct AddedDownload {
    /// Absent when the app is waiting for the user to confirm (`ask_before_download`).
    pub id: Option<DownloadId>,
    pub action: AddAction,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum AddAction {
    Started,
    /// Added to a queue that is not running right now.
    Queued,
    /// The app is showing the confirmation dialog.
    AwaitingConfirmation,
}

/// The subset of settings the extension popup shows. The app stores them; the
/// extension only edits them remotely and caches the last copy for display.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ExtensionSettings {
    /// Master switch: take over browser downloads at all.
    pub capture_enabled: bool,
    /// Show the confirmation dialog for every captured download (D4; off by default).
    pub ask_before_download: bool,
    /// Show a download button over video and audio on web pages.
    pub show_media_button: bool,
    /// Host names whose downloads stay in the browser, for example `example.com`.
    pub ignored_sites: Vec<String>,
    /// File extensions to capture, lower case and without the dot. Empty means all.
    pub capture_extensions: Vec<String>,
    /// Files known to be smaller than this stay in the browser.
    #[ts(type = "number")]
    pub min_capture_size: u64,
    pub default_queue: QueueId,
}

impl Default for ExtensionSettings {
    fn default() -> Self {
        Self {
            capture_enabled: true,
            ask_before_download: false,
            show_media_button: true,
            ignored_sites: Vec::new(),
            capture_extensions: Vec::new(),
            min_capture_size: 0,
            default_queue: QueueId(1),
        }
    }
}

/// A partial update: only fields that are `Some` change.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ExtensionSettingsPatch {
    pub capture_enabled: Option<bool>,
    pub ask_before_download: Option<bool>,
    pub show_media_button: Option<bool>,
    pub ignored_sites: Option<Vec<String>>,
    pub capture_extensions: Option<Vec<String>>,
    #[ts(type = "number | null")]
    pub min_capture_size: Option<u64>,
    pub default_queue: Option<QueueId>,
}

impl ExtensionSettings {
    pub fn apply(&mut self, patch: ExtensionSettingsPatch) {
        let ExtensionSettingsPatch {
            capture_enabled,
            ask_before_download,
            show_media_button,
            ignored_sites,
            capture_extensions,
            min_capture_size,
            default_queue,
        } = patch;
        if let Some(v) = capture_enabled {
            self.capture_enabled = v;
        }
        if let Some(v) = ask_before_download {
            self.ask_before_download = v;
        }
        if let Some(v) = show_media_button {
            self.show_media_button = v;
        }
        if let Some(v) = ignored_sites {
            self.ignored_sites = v;
        }
        if let Some(v) = capture_extensions {
            self.capture_extensions = v;
        }
        if let Some(v) = min_capture_size {
            self.min_capture_size = v;
        }
        if let Some(v) = default_queue {
            self.default_queue = v;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn patch_changes_only_given_fields() {
        let mut s = ExtensionSettings::default();
        s.apply(ExtensionSettingsPatch {
            ask_before_download: Some(true),
            ..Default::default()
        });
        assert!(s.ask_before_download);
        assert!(s.capture_enabled);
    }
}
