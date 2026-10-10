//! Downloads handed over by the browser extension, and the confirmation flow for
//! `ask_before_download` (D4).

use std::path::PathBuf;

use wirecatch_engine::Url;
use wirecatch_protocol::QueueId;
use wirecatch_protocol::app::{
    CaptureConfirmation, DownloadStatus, DownloadSummary, PendingCapture, PortablePath, UiEvent,
};
use wirecatch_protocol::extension::{AddAction, AddedDownload, Header, NewDownload};

use super::Manager;
use super::downloads::{NewEntry, parse_url};
use super::state::Pending;
use crate::error::{CoreError, Result};
use crate::naming;
use crate::settings;
use crate::store::Origin;

impl Manager {
    /// Takes over a download captured in the browser. Starts it at once and tells
    /// the UI to show its toast, or, when the user asked to confirm each download,
    /// holds it until [`confirm_capture`](Manager::confirm_capture) or
    /// [`reject_capture`](Manager::reject_capture).
    pub async fn add_capture(&self, capture: NewDownload) -> Result<AddedDownload> {
        let url = parse_url(&capture.url)?;
        let (ask, queue) = {
            let state = self.lock_open()?;
            let wanted = capture
                .queue
                .unwrap_or(state.settings.extension.default_queue);
            // A queue deleted since the extension last synced falls back to the default.
            let queue = state
                .slot(wanted)
                .map_or_else(|_| state.default_queue(), |s| s.queue.id);
            (state.settings.extension.ask_before_download, queue)
        };
        if ask {
            return self.hold_for_confirmation(capture, url, queue);
        }

        let row = self
            .insert(NewEntry {
                file_name: capture.suggested_name.clone(),
                folder: None,
                queue,
                start: true,
                origin: origin_of(&capture),
                url,
            })
            .await?;
        // The row is a snapshot taken right after the add, so a download that has
        // already finished by now still counts as started.
        let started = matches!(
            row.status,
            DownloadStatus::Connecting | DownloadStatus::Downloading
        );
        self.emit(UiEvent::Captured(row.clone()));
        Ok(AddedDownload {
            id: Some(row.id),
            action: if started {
                AddAction::Started
            } else {
                AddAction::Queued
            },
        })
    }

    /// Captures waiting for the user's answer, oldest first.
    pub fn pending_confirmations(&self) -> Vec<PendingCapture> {
        self.lock()
            .pending
            .values()
            .map(|p| p.capture.clone())
            .collect()
    }

    /// Accepts a waiting capture, with the user's changes, and starts it.
    pub async fn confirm_capture(
        &self,
        id: u64,
        answer: CaptureConfirmation,
    ) -> Result<DownloadSummary> {
        let pending = self
            .lock_open()?
            .pending
            .remove(&id)
            .ok_or(CoreError::ConfirmationNotFound(id))?;
        match self.insert_confirmed(&pending, answer).await {
            Ok(row) => Ok(row),
            Err(e) => {
                // Keep it so the user can fix the problem (another folder, say) and confirm again.
                if let Ok(mut state) = self.lock_open() {
                    state.pending.insert(id, pending);
                }
                Err(e)
            }
        }
    }

    /// Dismisses a waiting capture; nothing is downloaded.
    pub fn reject_capture(&self, id: u64) -> Result<()> {
        self.lock_open()?
            .pending
            .remove(&id)
            .map(|_| ())
            .ok_or(CoreError::ConfirmationNotFound(id))
    }

    fn hold_for_confirmation(
        &self,
        capture: NewDownload,
        url: Url,
        queue: QueueId,
    ) -> Result<AddedDownload> {
        let mut state = self.lock_open()?;
        let file_name = naming::file_name(capture.suggested_name.as_deref(), &url);
        let category = settings::category_of(&state.settings, &file_name);
        let folder = settings::folder_for(&state.settings, category, &self.inner.folders)?;
        let id = state.ids.pending();
        let proposal = PendingCapture {
            id,
            url: url.to_string(),
            file_name,
            folder: folder.to_string_lossy().into_owned(),
            category,
            size: capture.size_hint,
            queue,
            page_url: capture.page_url.clone(),
        };
        state.pending.insert(
            id,
            Pending {
                capture: proposal.clone(),
                request: capture,
            },
        );
        self.emit(UiEvent::ConfirmationRequested(proposal));
        Ok(AddedDownload {
            id: None,
            action: AddAction::AwaitingConfirmation,
        })
    }

    async fn insert_confirmed(
        &self,
        pending: &Pending,
        answer: CaptureConfirmation,
    ) -> Result<DownloadSummary> {
        let proposal = &pending.capture;
        let folder = match answer.folder {
            Some(folder) => self.inner.folders.resolve(&PortablePath(folder))?,
            None => PathBuf::from(&proposal.folder),
        };
        self.insert(NewEntry {
            url: parse_url(&proposal.url)?,
            file_name: Some(
                answer
                    .file_name
                    .unwrap_or_else(|| proposal.file_name.clone()),
            ),
            folder: Some(folder),
            queue: answer.queue.unwrap_or(proposal.queue),
            start: true,
            origin: origin_of(&pending.request),
        })
        .await
    }
}

/// The headers that make a captured download work outside the browser.
fn origin_of(capture: &NewDownload) -> Origin {
    let named = |name: &str, value: &Option<String>| {
        value.as_ref().map(|value| Header {
            name: name.into(),
            value: value.clone(),
        })
    };
    let headers = [
        named("Referer", &capture.referrer),
        named("Cookie", &capture.cookies),
    ]
    .into_iter()
    .flatten()
    .chain(capture.extra_headers.iter().cloned())
    .collect();
    Origin {
        user_agent: capture.user_agent.clone(),
        headers,
    }
}
