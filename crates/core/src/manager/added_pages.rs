//! Video pages added in the app (D51). A link on a site the apps know shows
//! videos is read for its qualities at once, like a page sent from the browser;
//! any other link starts as a plain download, and if its server answers with a
//! web page while the media add-on is installed, that page is read instead. Each
//! then asks for a quality or downloads with the saved choices (D50). A page
//! read this way on which no video is found downloads as the page it is.

use std::path::PathBuf;

use tracing::warn;
use wirecatch_engine::Url;
use wirecatch_protocol::app::{AddRequest, PortablePath};
use wirecatch_protocol::extension::MediaKind;
use wirecatch_protocol::{DownloadId, QuestionId};

use super::Manager;
use super::arrival::{Fetch, Kind, NewEntry, Wanted};
use super::downloads::parse_web_url;
use super::media::{Held, MediaPending, MediaSource, check_room};
use super::placement::{Candidate, Placed, QueueRequest};
use super::state::Entry;
use crate::error::Result;
use crate::naming;

/// A download added in the app whose server answered with a web page, to be
/// read for its video instead.
pub(super) struct AnsweredPage {
    pub fetch: Fetch,
    pub placed: Placed,
    /// The folder the person chose for it, if any.
    pub folder: Option<PathBuf>,
}

impl AnsweredPage {
    /// The page download `entry` answered with.
    pub(super) fn of(entry: &Entry) -> Option<Self> {
        Some(Self {
            fetch: Fetch {
                url: Url::parse(&entry.row.url).ok()?,
                origin: entry.origin.clone(),
                chosen_by: entry.chosen_by,
            },
            placed: Placed {
                queue: entry.row.queue,
                reason: entry.row.queue_reason.clone(),
            },
            folder: entry.naming.folder.as_ref().map(PathBuf::from),
        })
    }
}

impl Manager {
    /// Reads a video page added in the app (paste, the Add dialog) for its
    /// qualities, as a page sent from the browser is: its quality window opens,
    /// or it downloads with the saved choices (D50), paused when `request.start`
    /// is false. The window names the file from the video's title, so
    /// `request.file_name` is not used. Returns the question's id; the same page
    /// added again while it waits gets the same id.
    ///
    /// # Errors
    ///
    /// [`CoreError::InvalidUrl`](crate::CoreError::InvalidUrl) for a link that
    /// is not http or https; [`CoreError::QueueNotFound`](crate::CoreError::QueueNotFound)
    /// for an unknown queue; [`CoreError::Path`](crate::CoreError::Path) when the
    /// folder does not resolve; [`CoreError::LimitReached`](crate::CoreError::LimitReached)
    /// when too many videos are waiting; and [`CoreError::Closed`](crate::CoreError::Closed)
    /// once the manager has shut down.
    pub fn add_video_page(&self, request: AddRequest) -> Result<QuestionId> {
        let url = parse_web_url(request.url.as_str())?;
        let folder = request
            .folder
            .map(|f| self.inner.folders.resolve(&PortablePath(f)))
            .transpose()?;
        let mut state = self.lock_open()?;
        let request_queue = QueueRequest::in_app(request.queue);
        let name = naming::file_name(None, &url)?;
        let placed = state.place(request_queue, &Candidate::video(&url, &name))?;
        if let Some((&id, _)) = state
            .media
            .iter()
            .find(|(_, m)| m.source.fetch.url == url && m.source.kind == MediaKind::Page)
        {
            return Ok(id);
        }
        check_room(&state)?;
        let source = MediaSource::page(Fetch::typed(url));
        let held = Held {
            handovers: Vec::new(),
            folder,
            later: !request.start,
            page_otherwise: false,
            request: request_queue,
        };
        Ok(self.hold_media(&mut state, source, placed, held))
    }

    /// Removes download `id`, stopped because its server answered with a web
    /// page, and reads that page for its video instead; with no video on it, it
    /// downloads as the page it is ([`add_page_as_file`](Self::add_page_as_file)).
    pub(super) fn read_answered_page(&self, id: DownloadId, page: AnsweredPage) {
        self.remove_in_background(vec![id]);
        let Ok(mut state) = self.lock_open() else {
            return;
        };
        let source = MediaSource::page(page.fetch);
        let held = Held {
            handovers: Vec::new(),
            folder: page.folder,
            later: false,
            page_otherwise: true,
            request: page.placed.request(),
        };
        self.hold_media(&mut state, source, page.placed, held);
    }

    /// Downloads a page on which no video was found as the page it is, without
    /// reading it for a video again.
    pub(super) async fn add_page_as_file(&self, pending: MediaPending) {
        let source = pending.source;
        let added = self
            .insert(NewEntry {
                fetch: source.fetch,
                wanted: Wanted {
                    folder: pending.folder,
                    ..Wanted::default()
                },
                placed: pending.placed,
                start: !pending.later,
                kind: Kind::File { video_check: false },
            })
            .await;
        if let Err(e) = added {
            warn!(error = %e, "could not download a page without a video as a file");
        }
    }
}
