use std::path::PathBuf;

use wirecatch_engine::Url;
use wirecatch_protocol::QueueId;
use wirecatch_protocol::app::Category;

use super::state::Expectation;
use crate::media::StoredMedia;
use crate::store::Origin;
use wirecatch_sites::local_network::ChosenBy;

#[derive(Debug, Clone)]
pub(super) struct Fetch {
    pub url: Url,
    pub origin: Origin,
    pub chosen_by: ChosenBy,
}

impl Fetch {
    pub fn typed(url: Url) -> Self {
        Self {
            url,
            origin: Origin::default(),
            chosen_by: ChosenBy::Person,
        }
    }
}

/// Choices the server's answer may still improve.
#[derive(Debug, Default)]
pub(super) struct Wanted {
    /// Wins over the server's name.
    pub file_name: Option<String>,
    /// The browser's name, shown until the server answers.
    pub hint_name: Option<String>,
    pub folder: Option<PathBuf>,
    pub category: Option<Category>,
}

pub(super) enum Kind {
    /// `video_check`: a web page the server answers with is read for its video (D51).
    File { video_check: bool },
    Captured {
        /// Set while the extension waits for the answer.
        handover: Option<String>,
        expect: Expectation,
        /// A video or audio file linked directly.
        media: Option<StoredMedia>,
    },
    /// A stream or page; its path is final from the start.
    Media(StoredMedia),
}

impl Kind {
    pub fn handover(&self) -> Option<&str> {
        match self {
            Self::Captured { handover, .. } => handover.as_deref(),
            Self::File { .. } | Self::Media(_) => None,
        }
    }
}

pub(super) struct NewEntry {
    pub fetch: Fetch,
    pub wanted: Wanted,
    pub queue: QueueId,
    pub start: bool,
    pub kind: Kind,
}
