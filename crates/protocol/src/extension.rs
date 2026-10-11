//! What the browser extension hands over, and the settings it can change (D5).

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::{DownloadId, QueueId};

/// A download captured in the browser.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct NewDownload {
    /// Chosen by the extension, unique per handover. The app treats a repeated
    /// `add_download` with the same id as the same download (idempotent), and
    /// [`Call::WithdrawDownload`](crate::wire::Call::WithdrawDownload) refers to it.
    pub handover_id: String,
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
    /// Set when the extension knows this is media rather than a plain file.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub media: Option<MediaHint>,
}

/// What the extension knows about captured media.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct MediaHint {
    pub kind: MediaKind,
    /// The page or video title, used for the file name.
    pub title: Option<String>,
}

/// How the media is delivered.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum MediaKind {
    /// `url` is an HLS playlist (`.m3u8`).
    Hls,
    /// `url` is a DASH manifest (`.mpd`).
    Dash,
    /// `url` is a plain video or audio file.
    Direct,
    /// `url` is the page itself; finding the media needs the media add-on.
    Page,
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
    /// The server answered the app with the file (a 2xx response) and data is
    /// flowing. Only now may the extension erase the browser's copy.
    Started,
    /// Accepted into a queue that is not running right now; the link has not been
    /// tried yet.
    Queued,
    /// The app is showing the confirmation dialog; the link has not been tried yet.
    AwaitingConfirmation,
}

/// Asks the app to forget a handover the extension gave up waiting for.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct WithdrawRequest {
    pub handover_id: String,
}

/// The app's answer to a [`WithdrawRequest`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum WithdrawOutcome {
    /// The app dropped it; the browser should keep its copy.
    Withdrawn,
    /// The app never received it; the browser should keep its copy.
    NotFound,
    /// The app already reached the server; the browser should drop its copy.
    AlreadyStarted,
}

/// The subset of settings the extension popup shows. The app stores them; the
/// extension only edits them remotely and caches the last copy for display.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ExtensionSettings {
    /// Master switch: take over browser downloads at all.
    pub capture_enabled: bool,
    /// Show a window for every captured file before it starts (D23; on by default). Videos
    /// always ask for a quality, whatever this says.
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
            ask_before_download: true,
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
    /// Replaces the whole list. Prefer the add and remove fields, which do not
    /// overwrite an edit made at the same time from another browser.
    pub ignored_sites: Option<Vec<String>>,
    /// Hosts to add to `ignored_sites` (applied after `ignored_sites`).
    pub add_ignored_sites: Option<Vec<String>>,
    /// Hosts to remove from `ignored_sites` (applied last).
    pub remove_ignored_sites: Option<Vec<String>>,
    pub capture_extensions: Option<Vec<String>>,
    #[ts(type = "number | null")]
    pub min_capture_size: Option<u64>,
    pub default_queue: Option<QueueId>,
}

impl ExtensionSettings {
    /// Applies a partial update; see [`ExtensionSettingsPatch`] for the order.
    pub fn apply(&mut self, patch: ExtensionSettingsPatch) {
        let ExtensionSettingsPatch {
            capture_enabled,
            ask_before_download,
            show_media_button,
            ignored_sites,
            add_ignored_sites,
            remove_ignored_sites,
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
        for host in add_ignored_sites.into_iter().flatten() {
            if !self.ignored_sites.contains(&host) {
                self.ignored_sites.push(host);
            }
        }
        if let Some(remove) = remove_ignored_sites {
            self.ignored_sites.retain(|h| !remove.contains(h));
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
        assert!(
            s.ask_before_download,
            "a window shows first by default (D23)"
        );
        s.apply(ExtensionSettingsPatch {
            ask_before_download: Some(false),
            ..Default::default()
        });
        assert!(!s.ask_before_download);
        assert!(s.capture_enabled);
    }

    #[test]
    fn site_list_edits_merge_instead_of_overwriting() {
        let mut s = ExtensionSettings {
            ignored_sites: vec!["a.com".into(), "b.com".into()],
            ..Default::default()
        };
        s.apply(ExtensionSettingsPatch {
            add_ignored_sites: Some(vec!["c.com".into(), "a.com".into()]),
            remove_ignored_sites: Some(vec!["b.com".into()]),
            ..Default::default()
        });
        assert_eq!(s.ignored_sites, vec!["a.com", "c.com"]);
    }
}
