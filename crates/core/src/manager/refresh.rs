//! A desktop window can watch for the next capture of one unfinished download.

use tokio::sync::oneshot;
use wirecatch_engine::Url;
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

pub(super) struct WatchingLink {
    pub id: DownloadId,
    saved: oneshot::Sender<()>,
}

impl Manager {
    /// Watches for a fresh link to `id` and returns the exact source page when known. The
    /// receiver of `saved` must stay open while waiting for the link. Starting
    /// a new watch ends the previous one.
    ///
    /// # Errors
    ///
    /// The manager is closed, the download is missing or finished, or its files
    /// are stranded after a failed move.
    pub fn watch_link(&self, id: DownloadId, saved: oneshot::Sender<()>) -> Result<Option<Url>> {
        let mut state = self.lock_open()?;
        if saved.is_closed() {
            return Ok(None);
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
            return Ok(None);
        }
        let source = entry.origin.browser_source.as_deref();
        if source.is_some_and(|source| source.private_window) {
            return Ok(None);
        }
        let page = source.and_then(source_page).or_else(|| {
            let page_media = entry
                .row
                .media
                .as_ref()
                .is_some_and(|media| media.source == MediaKind::Page);
            page_media.then(|| web_url(&entry.row.url)).flatten()
        });
        state.link_refresh = Some(WatchingLink { id, saved });
        Ok(page)
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
            .filter(|watch| !watch.saved.is_closed())
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
        if entry.row.status == DownloadStatus::Completed
            || entry.refuse_if_stranded().is_err()
            || known_size
                .zip(capture.size_hint)
                .is_some_and(|(old, new)| old != new)
            || !matches_capture(entry, capture, source, url)
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
                "link refresh dialog closed after its capture was matched"
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
    let page = route
        .observed_page
        .as_ref()
        .and_then(|page| web_url(page.expose()))
        .map(|url| url.to_string().into());
    Some(BrowserSource {
        observed_page: page,
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

/// A stable entry can establish that two observed landing pages came from the
/// same navigation. The browser is always sent to the saved document itself.
fn stable_entry(source: &BrowserSource) -> Option<Url> {
    let page = source_page(source)?;
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
) -> bool {
    let kind = entry.row.media.as_ref().map(|media| media.source);
    if kind != capture.media.as_ref().map(|media| media.kind) {
        return false;
    }
    let old = entry.origin.browser_source.as_deref();
    if old.is_some_and(|source| source.private_window) {
        return false;
    }
    if kind == Some(MediaKind::Page) {
        // The download's URL is already the page. Its selected formats are
        // resolved again using the refreshed browser context.
        return web_url(&entry.row.url).as_ref() == Some(url);
    }
    let Some(old) = old else {
        // Older rows have no saved browser context. An exact address still
        // allows a fresh capture from the tab the person returns to.
        return web_url(&entry.row.url).as_ref() == Some(url);
    };
    let page_matches = source_page(old)
        .is_some_and(|page| source_page(source).as_ref() == Some(&page))
        || stable_entry(old).is_some_and(|page| stable_entry(source).as_ref() == Some(&page));
    if !page_matches && source_page(old).is_some() {
        return false;
    }
    if kind.is_none()
        && old
            .file_name
            .as_ref()
            .is_some_and(|name| source.file_name.as_ref().is_some_and(|new| new != name))
    {
        return false;
    }
    let same_name = old.file_name.is_some() && old.file_name == source.file_name;
    let new = addresses(source, url.as_str());
    addresses(old, &entry.row.url)
        .into_iter()
        .flatten()
        .any(|old| {
            new.iter().flatten().any(|new| {
                old == *new
                    || (page_matches
                        && (same_name || kind.is_some())
                        && same_signed_resource(&old, new))
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

fn same_signed_resource(old: &Url, new: &Url) -> bool {
    old.origin() == new.origin()
        && old.path() == new.path()
        && old
            .query_pairs()
            .filter(|(name, _)| !wirecatch_streams::is_signing_parameter(name))
            .eq(new
                .query_pairs()
                .filter(|(name, _)| !wirecatch_streams::is_signing_parameter(name)))
}
