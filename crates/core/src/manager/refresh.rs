//! A desktop window can watch for the next capture of one unfinished download.

use tokio::sync::oneshot;
use wirecatch_engine::{CancellationToken, Url};
use wirecatch_protocol::DownloadId;
use wirecatch_protocol::app::DownloadStatus;
use wirecatch_protocol::extension::{
    AddAction, AddedDownload, CaptureSource, MediaKind, NewDownload,
};
use wirecatch_records::origin::BrowserSource;
use wirecatch_sites::local_network::ChosenBy;

use super::Manager;
use super::captures::{expectation_of, origin_of};
use super::downloads::{RemoveFiles, Replacement};
use super::handovers::{Handover, Target};
use super::state::State;
use crate::error::{CoreError, Result};

/// What a window that asked to watch for a fresh link is told.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LinkWatch {
    /// The next matching capture is taken. Holds the source page, when it is known and may be reopened.
    Started(Option<Url>),
    /// No capture can refresh this download, so only a pasted link will.
    Unavailable,
}

impl LinkWatch {
    /// The page to reopen in the browser.
    #[must_use]
    pub fn page(&self) -> Option<&Url> {
        match self {
            Self::Started(page) => page.as_ref(),
            Self::Unavailable => None,
        }
    }
}

pub(super) struct WatchingLink {
    pub id: DownloadId,
    saved: oneshot::Sender<()>,
    stop: CancellationToken,
}

impl Manager {
    /// Watches for a fresh link to `id`, unless the window has gone, the download came from a
    /// private window or its capture is still undecided. The receiver of `saved` must stay
    /// open and `stop` must stay live while waiting. Starting a new watch ends the previous one.
    ///
    /// # Errors
    ///
    /// The manager is closed, the download is missing or finished, or its files
    /// are stranded after a failed move.
    pub fn watch_link(
        &self,
        id: DownloadId,
        saved: oneshot::Sender<()>,
        stop: CancellationToken,
    ) -> Result<LinkWatch> {
        let mut state = self.lock_open()?;
        if saved.is_closed() || stop.is_cancelled() {
            return Ok(LinkWatch::Unavailable);
        }
        let entry = state.entry(id)?;
        if entry.row.status == DownloadStatus::Completed {
            return Err(CoreError::WrongState(
                "This download has already finished, so its link cannot be changed.",
            ));
        }
        entry.refuse_if_stranded()?;
        if state
            .handovers
            .undecided_downloads()
            .any(|pending| pending == id)
        {
            return Ok(LinkWatch::Unavailable);
        }
        let source = entry.origin.browser_source.as_deref();
        if source.is_some_and(|source| source.private_window) {
            return Ok(LinkWatch::Unavailable);
        }
        let page = source.and_then(source_page).or_else(|| {
            let page_media = entry
                .row
                .media
                .as_ref()
                .is_some_and(|media| media.source == MediaKind::Page);
            (page_media && (source.is_some() || entry.chosen_by == ChosenBy::Person))
                .then(|| web_url(&entry.row.url))
                .flatten()
        });
        state.link_refresh = Some(WatchingLink { id, saved, stop });
        Ok(LinkWatch::Started(page))
    }

    /// Applies a matching capture under the same lock that consumes the watch
    /// and records the handover. Repeating or withdrawing the handover cannot
    /// add a second download or remove the original.
    pub(super) fn refresh_from_capture(
        &self,
        capture: &NewDownload,
        url: &Url,
        chosen_by: ChosenBy,
    ) -> Result<Option<AddedDownload>> {
        if !has_browser_source(capture.source)
            || capture
                .browser_route
                .as_ref()
                .is_none_or(|route| route.private_window)
        {
            return Ok(None);
        }
        let mut state = self.lock_open()?;
        if state.handovers.get(&capture.handover_id).is_some() {
            return Ok(None);
        }
        let Some(watch) = state
            .link_refresh
            .as_ref()
            .filter(|watch| !watch.saved.is_closed() && !watch.stop.is_cancelled())
        else {
            return Ok(None);
        };
        let id = watch.id;
        let Ok(entry) = state.entry(id) else {
            return Ok(None);
        };
        let origin = origin_of(capture);
        if origin.browser_source.is_none() {
            return Ok(None);
        }
        if entry.row.status == DownloadStatus::Completed
            || entry.refuse_if_stranded().is_err()
            || from_private_window(entry)
        {
            return Ok(None);
        }
        if !same_kind(entry, capture) {
            // The watch ends once the capture is a question or a download (`settle_kind_switch`).
            state.handovers.replace_with(&capture.handover_id, id);
            return Ok(None);
        }

        // Without a size from the browser the server's decides: the engine starts another file over (D286.4).
        let expect = entry.media_job.is_none().then(|| expectation_of(capture));
        self.replace_link(
            &mut state,
            id,
            url,
            Replacement::Captured {
                origin,
                chosen_by,
                expect,
            },
        )?;
        let reply = AddedDownload {
            id: Some(id),
            action: AddAction::Queued,
        };
        state.handovers.insert(
            &capture.handover_id,
            Handover {
                reply: reply.clone(),
                target: Target::Confirmed(id),
                deciding: None,
            },
        );
        answer_watch(&mut state, id);
        Ok(Some(reply))
    }

    /// The download added for one of `handovers` takes the place of the watched row a capture of another kind was made for, which is then removed with its partial data (D286.5).
    pub(super) async fn take_over(&self, handovers: &[String], new: DownloadId) {
        let replaced = {
            let Ok(mut state) = self.lock_open() else {
                return;
            };
            let mut replaced = None;
            for handover in handovers {
                replaced = state.handovers.take_replaced(handover).or(replaced);
            }
            let Some(old) = replaced.filter(|old| *old != new) else {
                return;
            };
            let Some(position) = state
                .downloads
                .get(&old)
                .filter(|entry| entry.row.status != DownloadStatus::Completed)
                .map(|entry| entry.position)
            else {
                return;
            };
            if let Some(entry) = state.downloads.get_mut(&new) {
                entry.position = position;
            }
            // Before the removal ends the watch, which the window would take as a failed one.
            if state
                .link_refresh
                .as_ref()
                .is_some_and(|watch| watch.id == old)
            {
                answer_watch(&mut state, old);
            }
            self.save(&mut state, new);
            self.reconcile(&mut state);
            old
        };
        if let Err(e) = self.remove(&[replaced], RemoveFiles::Keep).await {
            tracing::warn!(error = %e, "could not remove the download a new capture took the place of");
        }
    }

    /// Ends the watch that the capture for `handover` switched kind for, once it became a question or a download (`added`); forgets the takeover of one that did not (D286.5).
    pub(super) fn settle_kind_switch(&self, handover: &str, added: bool) {
        let Ok(mut state) = self.lock_open() else {
            return;
        };
        if !added {
            state.handovers.take_replaced(handover);
            return;
        }
        if let Some(watched) = state.handovers.replaced_by(handover)
            && state
                .link_refresh
                .as_ref()
                .is_some_and(|watch| watch.id == watched)
        {
            answer_watch(&mut state, watched);
        }
    }
}

/// Closes the window watching for `id`'s link: a capture answered it.
fn answer_watch(state: &mut State, id: DownloadId) {
    if let Some(watch) = state.link_refresh.take()
        && watch.saved.send(()).is_err()
    {
        tracing::debug!(
            id = id.0,
            "the link watch's window closed after its capture was matched"
        );
    }
}

pub(super) fn browser_source(capture: &NewDownload) -> Option<BrowserSource> {
    if !has_browser_source(capture.source) {
        return None;
    }
    let route = capture.browser_route.as_ref()?;
    Some(BrowserSource {
        observed_page: route
            .observed_page
            .as_ref()
            .filter(|page| web_url(page.expose()).is_some())
            .cloned(),
        private_window: route.private_window,
        private_session: route.private_session.clone(),
        profile: route.profile.clone(),
        file_name: capture
            .suggested_name
            .as_ref()
            .filter(|name| !name.is_empty())
            .cloned(),
    })
}

fn has_browser_source(source: CaptureSource) -> bool {
    matches!(
        source,
        CaptureSource::BrowserDownload | CaptureSource::MediaButton | CaptureSource::ContextMenu
    )
}

fn web_url(text: &str) -> Option<Url> {
    Url::parse(text).ok().filter(|url| {
        matches!(url.scheme(), "https" | "http")
            && url.username().is_empty()
            && url.password().is_none()
    })
}

fn source_page(source: &BrowserSource) -> Option<Url> {
    source
        .observed_page
        .as_ref()
        .and_then(|page| web_url(page.expose()))
}

/// D286: while the watch is open, the next capture of the same kind refreshes the row, whatever its address or size.
fn same_kind(entry: &super::Entry, capture: &NewDownload) -> bool {
    // A video file sent by the download button downloads as any other file.
    let delivery = |kind: Option<MediaKind>| kind.filter(|kind| *kind != MediaKind::Direct);
    delivery(entry.row.media.as_ref().map(|media| media.source))
        == delivery(capture.media.as_ref().map(|media| media.kind))
}

fn from_private_window(entry: &super::Entry) -> bool {
    entry
        .origin
        .browser_source
        .as_deref()
        .is_some_and(|source| source.private_window)
}
