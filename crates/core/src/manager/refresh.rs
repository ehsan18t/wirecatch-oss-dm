//! A desktop dialog can watch for the next capture of one unfinished browser download.

use tokio::sync::oneshot;
use wirecatch_engine::Url;
use wirecatch_protocol::app::DownloadStatus;
use wirecatch_protocol::extension::{
    AddAction, AddedDownload, CaptureSource, MAX_DOWNLOAD_REDIRECTS, NewDownload,
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
    /// Watches for a fresh link to `id` and returns the page to reopen. The
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
        let page = entry
            .origin
            .browser_source
            .as_deref()
            .filter(|source| !source.private_window && source.file_name.is_some())
            .and_then(reopen_page);
        let Some(page) = page.filter(|_| entry.row.media.is_none()) else {
            return Ok(None);
        };
        state.link_refresh = Some(WatchingLink { id, saved });
        Ok(Some(page))
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
        if capture.source != CaptureSource::BrowserDownload
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
        if entry.row.status == DownloadStatus::Completed
            || entry.refuse_if_stranded().is_err()
            || entry
                .row
                .total
                .or_else(|| entry.expect.as_ref().and_then(|expected| expected.size))
                .zip(capture.size_hint)
                .is_some_and(|(old, new)| old != new)
            || !entry
                .origin
                .browser_source
                .as_ref()
                .is_some_and(|old| matches_source(old, &entry.row.url, source, url))
        {
            return Ok(None);
        }

        self.replace_link(
            &mut state,
            id,
            url,
            Replacement::Captured {
                origin,
                chosen_by,
                expect: expectation_of(capture),
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
    if capture.source != CaptureSource::BrowserDownload || capture.media.is_some() {
        return None;
    }
    let route = capture.browser_route.as_ref()?;
    let page = route
        .source_page
        .as_ref()
        .and_then(|page| web_url(page.expose()))
        .map(|url| url.to_string().into());
    Some(BrowserSource {
        page,
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
    source.page.as_ref().and_then(|page| web_url(page.expose()))
}

/// A landing page with a query can expire. Prefer its stable same-origin entry
/// point when it has one, so the browser can reach the landing page again.
fn reopen_page(source: &BrowserSource) -> Option<Url> {
    let page = source_page(source)?;
    let entry = source
        .page_redirect_urls
        .first()
        .and_then(|url| web_url(url.expose()));
    let stable_entry = entry.filter(|entry| {
        page.query().is_some()
            && page.fragment().is_none()
            && entry.origin() == page.origin()
            && entry.query().is_none()
    });
    Some(stable_entry.unwrap_or(page))
}

/// Require the same page and browser filename, and identify the file by the
/// initial or final request. Shared intermediate redirects are insufficient.
fn matches_source(old: &BrowserSource, old_url: &str, new: &BrowserSource, new_url: &Url) -> bool {
    let Some(page) = source_page(old) else {
        return false;
    };
    let Some(name) = old.file_name.as_deref().filter(|name| !name.is_empty()) else {
        return false;
    };
    let same_page =
        source_page(new).as_ref() == Some(&page) || reopen_page(new) == reopen_page(old);
    if !same_page || new.file_name.as_deref() != Some(name) {
        return false;
    }
    let addresses = |source: &BrowserSource, final_url: &str| {
        [
            source
                .redirect_urls
                .first()
                .and_then(|url| web_url(url.expose())),
            web_url(final_url),
        ]
    };
    addresses(old, old_url).into_iter().flatten().any(|old| {
        addresses(new, new_url.as_str())
            .into_iter()
            .flatten()
            .any(|new| old == new)
    })
}
