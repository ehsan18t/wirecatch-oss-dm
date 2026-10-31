//! What the desktop app keeps for itself and shares with the extension: which files open without
//! asking, what the main window brings up for someone, and what the download complete window lists.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::app::DownloadSummary;
use crate::{DownloadId, QueueId};

crate::exported::exported_consts! {
    /// The most downloads one [`FinishedGroup`] lists; a queue's summary counts the rest.
    pub const MAX_FINISHED_LISTED: usize = 100;
    /// The most groups the download complete window keeps; the oldest go first.
    pub const MAX_FINISHED_GROUPS: usize = 20;
    /// File types that open directly, without asking first: common video, audio, image, document
    /// and archive types that do not run code when opened. Every other type, and a file with no
    /// extension, could run code (programs, installers, scripts, shortcuts, disk images, Office
    /// files with macros and many more), so opening it needs the person's confirmation. An
    /// allowlist fails safe: a type nobody thought of asks first.
    ///
    /// In order: video, audio, subtitles, images, documents, archives. Left out on purpose among
    /// documents: Office files with macros (`docm` and friends), `rtf` (embedded objects), the
    /// OpenDocument formats (macros) and `csv` (formulas that run when a spreadsheet opens it).
    /// `docx`, `xlsx` and `pptx` stay: they cannot hold macros, and Office opens downloaded ones,
    /// which carry the mark of the web, in Protected View.
    ///
    /// The shell decides with it (`is_program` in `crates/shell/src/api.rs`) and asks in a
    /// warning of its own.
    pub const SAFE_EXTENSIONS: &[&str] = &[
        "mp4", "m4v", "mkv", "webm", "mov", "avi", "wmv", "flv", "mpg", "mpeg", "ts", "m2ts", "3gp",
        "ogv", "mp3", "m4a", "aac", "flac", "wav", "ogg", "oga", "opus", "wma", "aiff", "mka", "srt",
        "vtt", "jpg", "jpeg", "png", "gif", "webp", "bmp", "tif", "tiff", "heic", "heif", "avif",
        "pdf", "txt", "md", "docx", "xlsx", "pptx", "epub", "zip", "7z", "rar", "tar", "gz", "tgz",
        "bz2", "xz", "zst",
    ];
}

/// What the main window shows when it comes to the front for someone: downloads it selects and
/// scrolls to, because the person clicked a notification about them (D52), the browser
/// settings, for the extension's settings button (D61), or the proxy settings, for a progress
/// window's fix of a proxy error (D71).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Reveal {
    /// The queue they are in, or finished in for a queue's summary: the list shows that queue.
    /// `None` shows every download.
    pub queue: Option<QueueId>,
    /// The downloads, the one to scroll to first.
    pub ids: Vec<DownloadId>,
    /// Show the browser settings instead of downloads.
    #[serde(default)]
    #[ts(optional, as = "Option<bool>")]
    pub browser_settings: bool,
    /// Show the proxy settings instead of downloads.
    #[serde(default)]
    #[ts(optional, as = "Option<bool>")]
    pub proxy_settings: bool,
    /// Open the details of the first download, where its queue is chosen (D73.4).
    #[serde(default)]
    #[ts(optional, as = "Option<bool>")]
    pub details: bool,
}

impl Reveal {
    /// The browser settings, for the extension's settings button (D61).
    #[must_use]
    pub fn browser_settings() -> Self {
        Self {
            queue: None,
            ids: Vec::new(),
            browser_settings: true,
            proxy_settings: false,
            details: false,
        }
    }

    /// Download `id` with its details open, for a progress window's Change queue (D73.4).
    #[must_use]
    pub fn details(id: DownloadId) -> Self {
        Self {
            queue: None,
            ids: vec![id],
            browser_settings: false,
            proxy_settings: false,
            details: true,
        }
    }

    /// The proxy settings, for a progress window's fix of a proxy error (D71).
    #[must_use]
    pub fn proxy_settings() -> Self {
        Self {
            queue: None,
            ids: Vec::new(),
            browser_settings: false,
            proxy_settings: true,
            details: false,
        }
    }
}

/// What the download complete window shows: what finished or failed since it opened, the
/// newest group first (D52).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct FinishedList {
    /// The groups, newest first.
    pub groups: Vec<FinishedGroup>,
}

/// Downloads that finished or failed one by one, or a queue's summary once it was done.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct FinishedGroup {
    /// Identifies the group while the window is open, for keyed lists.
    pub key: u32,
    /// The queue whose summary this is; `None` for downloads told one by one.
    pub queue: Option<FinishedQueue>,
    /// The downloads as they were when they ended, newest first, at most
    /// [`MAX_FINISHED_LISTED`]; the window follows their later changes from the core's events.
    pub downloads: Vec<DownloadSummary>,
}

/// The queue a [`FinishedGroup`] sums up.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct FinishedQueue {
    /// The queue.
    pub id: QueueId,
    /// Its name when it finished.
    pub name: String,
    /// How many of its downloads finished, listed or not.
    pub completed: u32,
    /// How many failed, listed or not.
    pub failed: u32,
}
