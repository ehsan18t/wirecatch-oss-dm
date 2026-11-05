//! Video pages added in the app (D51). A link on a site the apps know shows
//! videos is read for its qualities at once, like a page sent from the browser;
//! any other link starts as a plain download, and if its server answers with a
//! web page while the media add-on is installed, that page is read instead. Each
//! then asks for a quality or downloads with the saved choices (D50). A page
//! read this way on which no video is found downloads as the page it is.

use std::path::PathBuf;

use tracing::warn;
use wirecatch_engine::Url;
use wirecatch_protocol::app::{AddRequest, PortablePath, UiEvent};
use wirecatch_protocol::extension::MediaKind;
use wirecatch_protocol::{DownloadId, QuestionId};

use super::Manager;
use super::arrival::{Fetch, Kind, NewEntry, Wanted};
use super::downloads::parse_web_url;
use super::media::{Held, MediaPending, MediaSource};
use super::placement::{Candidate, Placed, QueueRequest};
use super::state::Entry;
use super::state::State;
use crate::error::Result;
use crate::naming;

/// A download added in the app whose server answered with a web page, to be
/// read for its video instead.
pub(super) struct AnsweredPage {
    pub fetch: Fetch,
    pub placed: Placed,
    /// The folder the person chose for it, if any.
    pub folder: Option<PathBuf>,
    /// The name the person chose for it, if any.
    pub file_name: Option<String>,
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
            file_name: entry.naming.name.clone(),
        })
    }
}

impl Manager {
    /// Reads a video page added in the app (paste, the Add dialog) for its
    /// qualities, as a page sent from the browser is: its quality window opens,
    /// or it downloads with the saved choices (D50), paused when `request.start`
    /// is false. `request.file_name` goes before the video's title. Returns the
    /// question's id; the same page added again while it waits gets the same id.
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
        state.media.check_room()?;
        let source = MediaSource::page(Fetch::typed(url));
        let held = Held {
            handovers: Vec::new(),
            folder,
            later: !request.start,
            page_otherwise: false,
            file_name: request.file_name,
            replaces: Vec::new(),
            request: request_queue,
        };
        Ok(self.hold_media(&mut state, source, placed, held))
    }

    /// Takes download `id`, stopped because its server answered with a web page,
    /// out of the list, and reads that page for its video instead; with no video on
    /// it, it downloads as the page it is ([`add_page_as_file`](Self::add_page_as_file)).
    /// Its saved row stays until the video is decided, as the question lives only in memory: after a quit the download runs again and asks again.
    pub(super) fn read_answered_page(&self, state: &mut State, id: DownloadId, page: AnsweredPage) {
        if state.closed {
            return;
        }
        let Some(entry) = state.remove_entry(id) else {
            return;
        };
        self.emit(UiEvent::DownloadsRemoved(vec![id]));
        self.inner.rows_changed.notify_one();
        let partial = entry.partial();
        let manager = self.clone();
        self.spawn(async move {
            let deleted = manager.blocking(move || {
                partial.delete();
                Ok(())
            });
            if let Err(e) = deleted.await {
                warn!(error = %e, "could not delete what a page's download left behind");
            }
        });
        let source = MediaSource::page(page.fetch);
        let held = Held {
            handovers: Vec::new(),
            folder: page.folder,
            later: false,
            page_otherwise: true,
            file_name: page.file_name,
            replaces: vec![id],
            request: page.placed.request(),
        };
        self.hold_media(state, source, page.placed, held);
    }

    /// Downloads a page on which no video was found as the page it is, without
    /// reading it for a video again.
    pub(super) async fn add_page_as_file(&self, pending: MediaPending) {
        let added = self
            .insert(NewEntry {
                fetch: pending.source.fetch.clone(),
                wanted: Wanted {
                    file_name: pending.file_name.clone(),
                    folder: pending.folder.clone(),
                    ..Wanted::default()
                },
                placed: pending.placed.clone(),
                start: !pending.later,
                kind: Kind::File { video_check: false },
            })
            .await;
        match added {
            Ok(_) => self.gave_way(&pending),
            Err(e) => warn!(error = %e, "could not download a page without a video as a file"),
        }
    }
}
