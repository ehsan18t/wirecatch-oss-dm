//! A desktop window can watch for the next capture of one unfinished download.

use tokio::sync::oneshot;
use wirecatch_engine::{CancellationToken, Url};
use wirecatch_protocol::app::DownloadStatus;
use wirecatch_protocol::extension::{
    AddAction, AddedDownload, CaptureSource, MAX_DOWNLOAD_REDIRECTS, MediaKind, NewDownload,
};
use wirecatch_protocol::{DownloadId, Secret};
use wirecatch_records::origin::BrowserSource;
use wirecatch_sites::local_network::ChosenBy;

use super::Manager;
use super::captures::{expectation_of, origin_of};
use super::downloads::Replacement;
use super::handovers::{Handover, Target};
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
        let Some(source) = origin.browser_source.as_ref() else {
            return Ok(None);
        };
        let known_size = if entry.media_job.is_none() {
            entry
                .row
                .total
                .or_else(|| entry.expect.as_ref().and_then(|expected| expected.size))
        } else {
            None
        };
        let sizes = known_size.zip(capture.size_hint);
        if entry.row.status == DownloadStatus::Completed
            || entry.refuse_if_stranded().is_err()
            || sizes.is_some_and(|(old, new)| old != new)
            || !matches_capture(entry, capture, source, url, sizes.is_some())
        {
            return Ok(None);
        }

        let expect = entry.media_job.is_none().then(|| {
            let mut expect = expectation_of(capture);
            expect.size = expect.size.or(known_size);
            expect
        });
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
        if let Some(watch) = state.link_refresh.take()
            && watch.saved.send(()).is_err()
        {
            tracing::debug!(
                id = id.0,
                "the link watch's window closed after its capture was matched"
            );
        }
        Ok(Some(reply))
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
        page_redirect_urls: safe_redirects(route.page_redirect_urls.as_deref().unwrap_or_default()),
        redirect_urls: safe_redirects(&route.redirect_urls),
        private_window: route.private_window,
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

fn safe_redirects(urls: &[Secret]) -> Vec<Secret> {
    urls.iter()
        .take(MAX_DOWNLOAD_REDIRECTS)
        .filter(|address| web_url(address.expose()).is_some())
        .cloned()
        .collect()
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

/// The address a navigation began at, when it led to `page` by adding a query:
/// two landing pages reached from one such entry are the same page with a rotated query.
fn stable_entry(source: &BrowserSource, page: &Url) -> Option<Url> {
    let entry = source
        .page_redirect_urls
        .first()
        .and_then(|url| web_url(url.expose()));
    entry.filter(|entry| {
        page.query().is_some()
            && page.fragment().is_none()
            && entry.origin() == page.origin()
            && entry.query().is_none()
    })
}

fn matches_capture(
    entry: &super::Entry,
    capture: &NewDownload,
    source: &BrowserSource,
    url: &Url,
    same_size: bool,
) -> bool {
    let kind = entry.row.media.as_ref().map(|media| media.source);
    if kind != capture.media.as_ref().map(|media| media.kind) {
        return false;
    }
    let old = entry.origin.browser_source.as_deref();
    if old.is_some_and(|source| source.private_window) {
        return false;
    }
    let Some(old) = old.filter(|_| kind != Some(MediaKind::Page)) else {
        // A video page is its own address; a row saved without its browser source has nothing else to compare.
        return web_url(&entry.row.url).as_ref() == Some(url);
    };
    let same_page = match (source_page(old), source_page(source)) {
        (None, _) => false,
        (Some(was), Some(now))
            if was == now
                || stable_entry(old, &was)
                    .is_some_and(|began| stable_entry(source, &now) == Some(began)) =>
        {
            true
        }
        (Some(_), _) => return false,
    };
    let names = old.file_name.as_ref().zip(source.file_name.as_ref());
    if kind.is_none() && names.is_some_and(|(was, now)| was != now) {
        return false;
    }
    // A file's address can carry a token that changes with every click: the same page, name and size stand in for the rest of it.
    let same_file = same_page && same_size && kind.is_none() && names.is_some();
    let signed_stream = same_page && matches!(kind, Some(MediaKind::Hls | MediaKind::Dash));
    let new = addresses(source, url.as_str());
    addresses(old, &entry.row.url).iter().flatten().any(|old| {
        new.iter().flatten().any(|new| {
            old == new
                || (same_file && same_place(old, new))
                || (signed_stream && same_signed_resource(old, new))
        })
    })
}

fn addresses(source: &BrowserSource, final_url: &str) -> [Option<Url>; 2] {
    [
        source
            .redirect_urls
            .first()
            .and_then(|url| web_url(url.expose())),
        web_url(final_url),
    ]
}

fn same_place(old: &Url, new: &Url) -> bool {
    old.origin() == new.origin() && old.path() == new.path()
}

fn same_signed_resource(old: &Url, new: &Url) -> bool {
    same_place(old, new)
        && old
            .query_pairs()
            .filter(|(name, _)| !wirecatch_streams::is_signing_parameter(name))
            .eq(new
                .query_pairs()
                .filter(|(name, _)| !wirecatch_streams::is_signing_parameter(name)))
}
