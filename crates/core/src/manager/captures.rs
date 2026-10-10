//! Downloads handed over by the browser extension, and the confirmation flow for
//! `ask_before_download` (D4).
//!
//! Every handover carries an id chosen by the extension. A repeated add with the
//! same id gets the first answer and adds nothing, and a withdrawal undoes a
//! handover the app has not acted on yet, so the browser and the app never both
//! download the same file.

use std::path::PathBuf;

use wirecatch_engine::Url;
use wirecatch_protocol::QueueId;
use wirecatch_protocol::app::{
    CaptureConfirmation, DownloadSummary, PendingCapture, PortablePath, UiEvent,
};
use wirecatch_protocol::extension::{AddAction, AddedDownload, NewDownload, WithdrawOutcome};

use super::Manager;
use super::downloads::{Added, NewEntry, capture_action, parse_url};
use super::handovers::{self, Handover, Target};
use super::state::Pending;
use crate::error::{CoreError, Result};
use crate::settings;
use crate::store::Origin;
use crate::{headers, naming};

/// Most captures that can wait for confirmation at once.
const MAX_PENDING: usize = 100;

impl Manager {
    /// Takes over a download captured in the browser. Starts it at once and tells
    /// the UI to show its toast, or, when the user asked to confirm each download,
    /// holds it until [`confirm_capture`](Manager::confirm_capture) or
    /// [`reject_capture`](Manager::reject_capture). A repeated call with the same
    /// handover id returns the first answer and adds nothing.
    pub async fn add_capture(&self, capture: NewDownload) -> Result<AddedDownload> {
        handovers::check_id(&capture.handover_id)?;
        let url = parse_url(&capture.url)?;
        let (ask, queue) = {
            let state = self.lock_open()?;
            if let Some(known) = state.handovers.get(&capture.handover_id) {
                return Ok(known.reply.clone());
            }
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

        let added = self
            .insert(NewEntry {
                file_name: capture.suggested_name.clone(),
                folder: None,
                queue,
                start: true,
                origin: origin_of(&capture),
                handover: Some(capture.handover_id.clone()),
                url,
            })
            .await?;
        match added {
            Added::Known(reply) => Ok(reply),
            Added::New(row) => {
                self.emit(UiEvent::Captured(row.clone()));
                Ok(AddedDownload {
                    id: Some(row.id),
                    action: capture_action(&row),
                })
            }
        }
    }

    /// Undoes a handover the extension gave up waiting for. A capture the app has
    /// not taken to the server yet is dropped completely, leaving no download and
    /// no files behind.
    pub async fn withdraw_capture(&self, handover_id: &str) -> Result<WithdrawOutcome> {
        let download = {
            let mut state = self.lock_open()?;
            let Some(known) = state.handovers.get(handover_id).cloned() else {
                return Ok(WithdrawOutcome::NotFound);
            };
            match known.target {
                Target::Dismissed => return Ok(WithdrawOutcome::NotFound),
                Target::Pending(pending) => {
                    state.pending.remove(&pending);
                    state.handovers.remove(handover_id);
                    return Ok(WithdrawOutcome::Withdrawn);
                }
                Target::Download(id) => match state.entry(id).ok() {
                    None => return Ok(WithdrawOutcome::NotFound),
                    Some(entry) if entry.server_answered() => {
                        return Ok(WithdrawOutcome::AlreadyStarted);
                    }
                    Some(_) => {
                        state.handovers.remove(handover_id);
                        id
                    }
                },
            }
        };
        self.remove(&[download], false).await?;
        Ok(WithdrawOutcome::Withdrawn)
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
            Ok(row) => {
                let target = Target::Download(row.id);
                self.lock().handovers.retarget(&pending.handover, target);
                Ok(row)
            }
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
        let mut state = self.lock_open()?;
        let pending = state
            .pending
            .remove(&id)
            .ok_or(CoreError::ConfirmationNotFound(id))?;
        state
            .handovers
            .retarget(&pending.handover, Target::Dismissed);
        Ok(())
    }

    fn hold_for_confirmation(
        &self,
        capture: NewDownload,
        url: Url,
        queue: QueueId,
    ) -> Result<AddedDownload> {
        let mut state = self.lock_open()?;
        if let Some(known) = state.handovers.get(&capture.handover_id) {
            return Ok(known.reply.clone());
        }
        let reply = AddedDownload {
            id: None,
            action: AddAction::AwaitingConfirmation,
        };
        // The same link captured again joins the confirmation already showing.
        let url_text = url.to_string();
        if let Some(existing) = state.pending.values().find(|p| p.capture.url == url_text) {
            let target = Target::Pending(existing.capture.id);
            let handover = Handover {
                reply: reply.clone(),
                target,
            };
            state.handovers.insert(&capture.handover_id, handover);
            return Ok(reply);
        }
        if state.pending.len() >= MAX_PENDING {
            return Err(CoreError::Invalid(format!(
                "{MAX_PENDING} downloads are already waiting for your confirmation. Answer some of them first."
            )));
        }
        let file_name = naming::file_name(capture.suggested_name.as_deref(), &url)?;
        let category = settings::category_of(&state.settings, &file_name);
        let folder = settings::folder_for(&state.settings, category, &self.inner.folders)?;
        let id = state.ids.pending();
        let proposal = PendingCapture {
            id,
            url: url_text,
            file_name,
            folder: folder.to_string_lossy().into_owned(),
            category,
            size: capture.size_hint,
            queue,
            page_url: capture.page_url.clone(),
        };
        state.handovers.insert(
            &capture.handover_id,
            Handover {
                reply: reply.clone(),
                target: Target::Pending(id),
            },
        );
        state.pending.insert(
            id,
            Pending {
                capture: proposal.clone(),
                handover: capture.handover_id.clone(),
                request: capture,
            },
        );
        self.emit(UiEvent::ConfirmationRequested(proposal));
        Ok(reply)
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
        let added = self
            .insert(NewEntry {
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
                handover: None,
            })
            .await?;
        match added {
            Added::New(row) => Ok(row),
            Added::Known(_) => Err(CoreError::Internal(
                "a confirmed capture was matched to an earlier handover".into(),
            )),
        }
    }
}

/// The headers that make a captured download work outside the browser.
fn origin_of(capture: &NewDownload) -> Origin {
    Origin {
        user_agent: capture.user_agent.clone(),
        headers: headers::for_capture(
            capture.referrer.as_deref(),
            capture.cookies.as_deref(),
            &capture.extra_headers,
        ),
    }
}
