//! Downloads handed over by the browser extension, and the confirmation flow for
//! `ask_before_download` (D4).
//!
//! Every handover carries an id chosen by the extension. A repeated add with the
//! same id gets the first answer and adds nothing, and a withdrawal undoes a
//! handover the app has not acted on yet, so the browser and the app never both
//! download the same file.

use wirecatch_engine::Url;
use wirecatch_protocol::app::{
    CaptureConfirmation, Category, DownloadSummary, MediaSummary, PendingCapture, PortablePath,
    UiEvent,
};
use wirecatch_protocol::extension::{
    AddAction, AddedDownload, MediaKind, NewDownload, WithdrawOutcome,
};
use wirecatch_protocol::{DownloadId, QueueId};

use super::Manager;
use super::downloads::{NewEntry, capture_action, parse_url};
use super::handovers::{self, Handover, Target};
use super::state::{Expectation, Pending};
use crate::error::{CoreError, Result};
use crate::media::StoredMedia;
use crate::settings;
use crate::store::Origin;
use crate::{headers, naming};

/// Most captures that can wait for confirmation at once.
const MAX_PENDING: usize = 100;
/// Most captures of one link that can share one confirmation.
const MAX_HANDOVERS_PER_CONFIRMATION: usize = 32;

impl Manager {
    /// Takes over a download captured in the browser.
    ///
    /// If its queue can run it, the app starts it and replies `Started` only once
    /// the server has answered the app with the file (within the capture timeout),
    /// so the browser can safely drop its own copy; otherwise the attempt is
    /// dropped and the call fails with [`CoreError::NotStarted`], and the browser
    /// keeps the download. A queue that cannot run it now gives `Queued`. When the
    /// user asked to confirm each download, it is held until
    /// [`confirm_capture`](Manager::confirm_capture) or
    /// [`reject_capture`](Manager::reject_capture). A repeated call with the same
    /// handover id gets the first call's answer and adds nothing.
    pub async fn add_capture(&self, capture: NewDownload) -> Result<AddedDownload> {
        handovers::check_id(&capture.handover_id)?;
        let url = parse_url(&capture.url)?;
        let (known, ask, queue) = {
            let state = self.lock_open()?;
            let known = state.handovers.get(&capture.handover_id).is_some();
            let wanted = capture
                .queue
                .unwrap_or(state.settings.extension.default_queue);
            // A queue deleted since the extension last synced falls back to the default.
            let queue = state
                .slot(wanted)
                .map_or_else(|_| state.default_queue(), |s| s.queue.id);
            (known, state.settings.extension.ask_before_download, queue)
        };
        if known {
            return self.known_handover(&capture.handover_id).await;
        }
        if capture
            .media
            .as_ref()
            .is_some_and(|m| m.kind != MediaKind::Direct)
        {
            return self.add_media_capture(capture, queue);
        }
        if ask {
            return self.hold_for_confirmation(capture, url, queue);
        }

        let added = self
            .insert(NewEntry {
                file_name: None,
                hint_name: capture.suggested_name.clone(),
                folder: None,
                queue,
                start: true,
                origin: origin_of(&capture),
                handover: Some(capture.handover_id.clone()),
                expect: Some(Expectation {
                    size: capture.size_hint,
                    name: capture.suggested_name.clone(),
                }),
                media: direct_media(&capture),
                category: direct_category(&capture),
                url,
            })
            .await?;
        let row = match added {
            None => return self.known_handover(&capture.handover_id).await,
            Some(row) => row,
        };
        let reply = AddedDownload {
            id: Some(row.id),
            action: capture_action(&row),
        };
        if reply.action == AddAction::Queued {
            // The link has not been tried; the extension decides what that means.
            self.emit(UiEvent::Captured(row));
            return Ok(reply);
        }
        match self.await_answer(row.id, &capture.handover_id).await {
            Ok(()) => {
                let mut state = self.lock();
                state.handovers.decide(&capture.handover_id, reply.clone());
                if let Ok(entry) = state.entry(row.id) {
                    self.emit(UiEvent::Captured(entry.row.clone()));
                }
                Ok(reply)
            }
            Err(reason) => {
                // Answer at once: stopping the attempt can take up to the stop
                // timeout, and the extension does not wait that long. The attempt
                // is hidden now and removed in the background.
                self.lock().handovers.remove(&capture.handover_id);
                self.remove_in_background(vec![row.id]);
                Err(CoreError::NotStarted(reason))
            }
        }
    }

    /// Waits until the server has answered the app with the captured file, the
    /// attempt failed, or the capture timeout ran out. `Err` carries the reason as
    /// whole sentences, for [`CoreError::NotStarted`].
    async fn await_answer(
        &self,
        id: DownloadId,
        handover: &str,
    ) -> std::result::Result<(), String> {
        let tokens = {
            let state = self.lock();
            state
                .entry(id)
                .ok()
                .and_then(|e| e.run.as_ref())
                .map(|run| (run.answered.clone(), run.exited.clone()))
        };
        if let Some((answered, exited)) = tokens {
            let timeout = tokio::time::sleep(self.inner.capture_timeout);
            tokio::select! {
                biased;
                _ = answered.cancelled() => {}
                _ = exited.cancelled() => {}
                _ = timeout => {
                    return Err(format!(
                        "The server did not answer within {} seconds.",
                        self.inner.capture_timeout.as_secs()
                    ));
                }
            }
        }
        let state = self.lock();
        if state.handovers.get(handover).is_none() {
            return Err("The browser took it back.".into());
        }
        match state.entry(id) {
            Err(_) => Err("It was removed.".into()),
            Ok(entry) if entry.server_answered() => Ok(()),
            Ok(entry) => Err(entry.row.error.as_ref().map_or_else(
                || "It stopped before the server answered.".to_string(),
                |e| e.message.clone(),
            )),
        }
    }

    /// The answer for a handover already being handled: the final answer, after
    /// waiting for it if the first call is still waiting for the server.
    async fn known_handover(&self, handover: &str) -> Result<AddedDownload> {
        let deciding = {
            let state = self.lock_open()?;
            match state.handovers.get(handover) {
                None => None,
                Some(known) => match &known.deciding {
                    None => return Ok(known.reply.clone()),
                    Some(deciding) => Some(deciding.clone()),
                },
            }
        };
        if let Some(deciding) = deciding {
            let patience = self.inner.capture_timeout + self.inner.stop_timeout;
            let _ = tokio::time::timeout(patience, deciding.cancelled()).await;
        }
        let state = self.lock_open()?;
        match state.handovers.get(handover) {
            Some(known) if known.deciding.is_none() => Ok(known.reply.clone()),
            _ => Err(CoreError::NotStarted(
                "An earlier attempt to take it over did not work.".into(),
            )),
        }
    }

    /// Undoes a handover the extension gave up waiting for. A capture the app has
    /// not yet replied `Started` to, or that was queued and has not reached the
    /// server, is dropped completely, leaving no download and no files behind.
    /// Once the app has replied `Started` (the server answered), it is the app's.
    pub async fn withdraw_capture(&self, handover_id: &str) -> Result<WithdrawOutcome> {
        let download = {
            let mut state = self.lock_open()?;
            let Some(known) = state.handovers.get(handover_id).cloned() else {
                return Ok(WithdrawOutcome::NotFound);
            };
            match known.target {
                Target::Dismissed => return Ok(WithdrawOutcome::NotFound),
                // The user said yes in the app; the browser no longer needs its copy.
                Target::Confirmed(_) => return Ok(WithdrawOutcome::AlreadyStarted),
                Target::Pending(pending) => {
                    // Other tabs may still be waiting on the same question.
                    if let Some(waiting) = state.pending.get_mut(&pending) {
                        waiting.handovers.retain(|h| h != handover_id);
                        if waiting.handovers.is_empty() {
                            state.pending.remove(&pending);
                            self.emit(UiEvent::ConfirmationClosed { id: pending });
                        }
                    } else {
                        self.withdraw_media_handover(&mut state, pending, handover_id);
                    }
                    state.handovers.remove(handover_id);
                    return Ok(WithdrawOutcome::Withdrawn);
                }
                Target::Download(id) => {
                    let still_deciding = known.deciding.is_some();
                    match state.entry(id).ok() {
                        None => return Ok(WithdrawOutcome::NotFound),
                        Some(entry) if !still_deciding && entry.server_answered() => {
                            return Ok(WithdrawOutcome::AlreadyStarted);
                        }
                        Some(_) => {
                            state.handovers.remove(handover_id);
                            id
                        }
                    }
                }
            }
        };
        // Hidden and never started again from now on; stopping it may take up to
        // the stop timeout, which the extension should not have to wait for.
        self.remove_in_background(vec![download]);
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

    /// Accepts a waiting capture, with the user's changes, and starts it. The new
    /// download arrives with the next `DownloadsChanged` event.
    pub async fn confirm_capture(&self, id: u64, answer: CaptureConfirmation) -> Result<()> {
        let pending = self
            .lock_open()?
            .pending
            .remove(&id)
            .ok_or(CoreError::ConfirmationNotFound(id))?;
        match self.insert_confirmed(&pending, answer).await {
            Ok(row) => {
                let mut state = self.lock();
                for handover in &pending.handovers {
                    state
                        .handovers
                        .retarget(handover, Target::Confirmed(row.id));
                }
                self.emit(UiEvent::ConfirmationClosed { id });
                Ok(())
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
        for handover in &pending.handovers {
            state.handovers.retarget(handover, Target::Dismissed);
        }
        self.emit(UiEvent::ConfirmationClosed { id });
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
        if let Some(existing) = state
            .pending
            .values_mut()
            .find(|p| p.capture.url == url_text)
        {
            if existing.handovers.len() >= MAX_HANDOVERS_PER_CONFIRMATION {
                return Err(CoreError::Invalid(
                    "This link was captured many times and is already waiting for your confirmation. Answer that first.".into(),
                ));
            }
            existing.handovers.push(capture.handover_id.clone());
            let handover = Handover {
                reply: reply.clone(),
                target: Target::Pending(existing.capture.id),
                deciding: None,
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
                deciding: None,
            },
        );
        state.pending.insert(
            id,
            Pending {
                capture: proposal.clone(),
                handovers: vec![capture.handover_id.clone()],
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
        // Only what the user changed is a choice; the proposal was a guess that the
        // server's answer may still improve.
        let folder = answer
            .folder
            .map(|folder| self.inner.folders.resolve(&PortablePath(folder)))
            .transpose()?;
        let request = &pending.request;
        let added = self
            .insert(NewEntry {
                url: parse_url(&proposal.url)?,
                file_name: answer.file_name,
                hint_name: Some(proposal.file_name.clone()),
                folder,
                queue: answer.queue.unwrap_or(proposal.queue),
                start: true,
                origin: origin_of(request),
                handover: None,
                expect: Some(Expectation {
                    size: request.size_hint,
                    name: request.suggested_name.clone(),
                }),
                media: direct_media(request),
                category: direct_category(request),
            })
            .await?;
        match added {
            Some(row) => Ok(row),
            None => Err(CoreError::Internal(
                "a confirmed capture was matched to an earlier handover".into(),
            )),
        }
    }
}

/// The row's media details for a direct video or audio file.
fn direct_media(capture: &NewDownload) -> Option<StoredMedia> {
    let hint = capture
        .media
        .as_ref()
        .filter(|m| m.kind == MediaKind::Direct)?;
    Some(StoredMedia {
        summary: MediaSummary {
            quality: "Original".into(),
            source: hint.kind,
        },
        job: None,
    })
}

/// A direct media file is video, or audio when the browser says so.
fn direct_category(capture: &NewDownload) -> Option<Category> {
    capture
        .media
        .as_ref()
        .filter(|m| m.kind == MediaKind::Direct)?;
    let audio = capture
        .mime
        .as_deref()
        .is_some_and(|m| m.trim().to_ascii_lowercase().starts_with("audio/"));
    Some(if audio {
        Category::Audio
    } else {
        Category::Video
    })
}

/// The headers that make a captured download work outside the browser.
pub(super) fn origin_of(capture: &NewDownload) -> Origin {
    Origin {
        user_agent: capture.user_agent.clone(),
        headers: headers::for_capture(
            capture.referrer.as_deref(),
            capture.cookies.as_deref(),
            &capture.extra_headers,
        ),
    }
}
