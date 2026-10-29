//! Downloads handed over by the browser extension, and the confirmation flow for
//! `ask_before_download` (D4, D23).
//!
//! Every handover carries an id chosen by the extension. A repeated add with the
//! same id gets the first answer and adds nothing, and a withdrawal undoes a
//! handover the app has not acted on yet, so the browser and the app never both
//! download the same file.

use std::time::Duration;

use tokio::time::Instant;
use wirecatch_engine::{CancellationToken, Url};
use wirecatch_media::StoredMedia;
use wirecatch_protocol::app::{
    CaptureConfirmation, Category, DownloadStatus, DownloadSummary, MediaSummary, PendingCapture,
    PortablePath, Repeated, UiEvent,
};
use wirecatch_protocol::extension::{
    AddAction, AddedDownload, MediaKind, NewDownload, WithdrawOutcome,
};
use wirecatch_protocol::{DownloadId, QuestionId, Secret};
use wirecatch_sites::local_network::{self, ChosenBy};

use super::Manager;
use super::arrival::{Fetch, Kind, NewEntry, Wanted};
use super::downloads::{capture_action, parse_url};
use super::handovers::{self, Handover, Target};
use super::placement::{Candidate, Placed, QueueRequest};
use super::state::{Expectation, Pending, State};
use crate::error::{CoreError, Result};
use crate::headers::Origin;
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
    ///
    /// # Errors
    ///
    /// [`CoreError::NotStarted`] when the app could not take the download over (the
    /// server did not answer in time, refused it, or the browser took it back), so the
    /// browser keeps it; [`CoreError::Invalid`] for a handover id that is empty or too
    /// long, or a name that cannot be used; [`CoreError::InvalidUrl`] for a link that
    /// is not http, https or FTP; [`CoreError::Path`] when the folder for its category
    /// does not resolve; and [`CoreError::Closed`] once the manager has shut down.
    pub async fn add_capture(&self, capture: NewDownload) -> Result<AddedDownload> {
        // The extension waits a fixed time for the answer from when it sent
        // the capture, the check of the link included.
        self.capture_arrived(capture, Instant::now()).await
    }

    /// [`add_capture`](Self::add_capture) for a capture that `arrived` at
    /// that moment, which the capture timeout counts from.
    pub(super) async fn capture_arrived(
        &self,
        capture: NewDownload,
        arrived: Instant,
    ) -> Result<AddedDownload> {
        handovers::check_id(&capture.handover_id)?;
        let url = parse_url(&capture.url)?;
        let Arrival {
            known,
            ask,
            placed,
            chosen_by,
        } = self.arrival(&capture, &url)?;
        if known {
            return self.known_handover(&capture.handover_id).await;
        }
        // Before anything is answered: once the browser hears the app has it
        // (waiting for the person, or queued), the browser's copy is gone.
        if local_network::refuses(chosen_by, &url).await {
            return Err(CoreError::NotStarted(
                wirecatch_messages::CAPTURE_TO_OWN_NETWORK.into(),
            ));
        }
        if let Some(joined) = self.join_unfinished(&capture, &url)? {
            return Ok(joined);
        }
        if capture
            .media
            .as_ref()
            .is_some_and(|m| m.kind != MediaKind::Direct)
        {
            return self.add_media_capture(&capture, placed, chosen_by);
        }
        if ask {
            return self.hold_for_confirmation(capture, &url, placed, chosen_by);
        }

        let added = self
            .insert(NewEntry {
                fetch: Fetch {
                    url,
                    origin: origin_of(&capture),
                    chosen_by,
                },
                wanted: wanted_by(&capture),
                placed,
                start: true,
                kind: captured(&capture, Some(capture.handover_id.clone())),
            })
            .await?;
        let Some(row) = added else {
            return self.known_handover(&capture.handover_id).await;
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
        match self
            .await_answer(row.id, &capture.handover_id, arrived)
            .await
        {
            Ok(()) => {
                let mut state = self.lock();
                state.handovers.decide(&capture.handover_id, reply.clone());
                if let Ok(entry) = state.entry(row.id) {
                    self.emit(UiEvent::Captured(entry.summary()));
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
    /// attempt failed, or the capture timeout, counted from when the capture
    /// `arrived`, ran out. `Err` carries the reason as whole sentences, for
    /// [`CoreError::NotStarted`].
    async fn await_answer(
        &self,
        id: DownloadId,
        handover: &str,
        arrived: Instant,
    ) -> std::result::Result<(), String> {
        let tokens = {
            let state = self.lock();
            state
                .entry(id)
                .ok()
                .and_then(|e| e.run.as_ref())
                .map(|run| (run.answered.clone(), run.exited.clone()))
        };
        if let Some((answered, exited)) = tokens
            && !ended_in_time(&answered, &exited, arrived, self.inner.capture_timeout).await
        {
            return Err(format!(
                "The server did not answer within {} seconds.",
                self.inner.capture_timeout.as_secs()
            ));
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
            if tokio::time::timeout(patience, deciding.cancelled())
                .await
                .is_err()
            {
                tracing::warn!(
                    handover,
                    ?patience,
                    "the first attempt to take this download over is still undecided; answering that it did not start"
                );
            }
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
    ///
    /// # Errors
    ///
    /// [`CoreError::Closed`] once the manager has shut down. A handover that is unknown
    /// or already started is an outcome, not an error.
    pub fn withdraw_capture(&self, handover_id: &str) -> Result<WithdrawOutcome> {
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
    #[must_use]
    pub fn pending_confirmations(&self) -> Vec<PendingCapture> {
        self.lock()
            .pending
            .values()
            .map(|p| capture_for_display(&p.capture))
            .collect()
    }

    /// Accepts a waiting capture, with the user's changes, and starts it, or with
    /// `answer.later` adds it to its queue paused. Returns the new download's id; its
    /// row also arrives with the next `DownloadsChanged` event.
    ///
    /// # Errors
    ///
    /// [`CoreError::ConfirmationNotFound`] when the question was already answered or
    /// withdrawn; [`CoreError::InvalidUrl`], [`CoreError::Invalid`],
    /// [`CoreError::Path`] or [`CoreError::QueueNotFound`] when the answer's link,
    /// name, folder or queue cannot be used, in which case the question stays open; and
    /// [`CoreError::Closed`] once the manager has shut down.
    pub async fn confirm_capture(
        &self,
        id: QuestionId,
        answer: CaptureConfirmation,
    ) -> Result<DownloadId> {
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
                Ok(row.id)
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
    ///
    /// # Errors
    ///
    /// [`CoreError::ConfirmationNotFound`] when the question was already answered or
    /// withdrawn, and [`CoreError::Closed`] once the manager has shut down.
    pub fn reject_capture(&self, id: QuestionId) -> Result<()> {
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

    /// The newest download in the list that the open question `id` (a waiting
    /// capture or media choice) is about again, so the question can say so: the
    /// same address, or for a video the same page, or the same video read from
    /// another link to it. `None` when there is none, or no such question.
    #[must_use]
    pub fn earlier_download(&self, id: QuestionId) -> Option<DownloadSummary> {
        let state = self.lock();
        let (links, video) = if let Some(waiting) = state.pending.get(&id) {
            (vec![waiting.capture.url.clone()], None)
        } else {
            let media = state.media.get(&id)?;
            let mut links = vec![media.source.fetch.url.to_string()];
            links.extend(media.source.page_url.clone());
            (links, media.video.clone())
        };
        state
            .visible()
            .filter(|entry| {
                let row = &entry.row;
                let same_video = video.is_some()
                    && row.media.as_ref().and_then(|m| m.video_key.as_ref()) == video.as_ref();
                same_video || links.iter().any(|link| same_address(link, &row.url))
            })
            .max_by_key(|entry| (entry.row.added_at, entry.row.id))
            .map(super::state::Entry::summary)
    }

    /// A capture of a link that is already a download not finished yet (queued, running or
    /// paused) joins that download instead of starting another, and its window comes to the
    /// front (D89). `None` when there is no such download.
    fn join_unfinished(&self, capture: &NewDownload, url: &Url) -> Result<Option<AddedDownload>> {
        let mut state = self.lock_open()?;
        let Some(row) = state
            .visible()
            .map(|entry| &entry.row)
            .find(|row| unfinished(row.status) && same_address(&row.url, url.as_str()))
            .cloned()
        else {
            return Ok(None);
        };
        let reply = AddedDownload {
            id: Some(row.id),
            action: capture_action(&row),
        };
        // Confirmed, not the capture's own: withdrawing this handover never takes the download back.
        let handover = Handover {
            reply: reply.clone(),
            target: Target::Confirmed(row.id),
            deciding: None,
        };
        state.handovers.insert(&capture.handover_id, handover);
        self.emit(UiEvent::CaptureRepeated(Repeated::Download(row.id)));
        Ok(Some(reply))
    }

    /// How a capture arrives, read from the state in one go.
    fn arrival(&self, capture: &NewDownload, url: &Url) -> Result<Arrival> {
        let state = self.lock_open()?;
        let placed = placed_capture(&state, capture, url)?;
        Ok(Arrival {
            known: state.handovers.get(&capture.handover_id).is_some(),
            ask: state.settings.extension.ask_before_download,
            placed,
            chosen_by: ChosenBy::of_capture(capture, &state.settings.local_sites, &state.suffixes),
        })
    }

    fn hold_for_confirmation(
        &self,
        capture: NewDownload,
        url: &Url,
        placed: Placed,
        chosen_by: ChosenBy,
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
                return Err(CoreError::LimitReached(
                    "This link was captured many times and is already waiting for your confirmation. Answer that first.".into(),
                ));
            }
            existing.handovers.push(capture.handover_id.clone());
            let id = existing.capture.id;
            let handover = Handover {
                reply: reply.clone(),
                target: Target::Pending(id),
                deciding: None,
            };
            state.handovers.insert(&capture.handover_id, handover);
            self.emit(UiEvent::CaptureRepeated(Repeated::Question(id)));
            return Ok(reply);
        }
        if state.pending.len() >= MAX_PENDING {
            return Err(CoreError::LimitReached(format!(
                "{MAX_PENDING} downloads are already waiting for your confirmation. Answer some of them first."
            )));
        }
        let file_name = naming::file_name(capture.suggested_name.as_deref(), url)?;
        let category = wirecatch_settings::category_of(&state.settings, &file_name);
        let folder =
            wirecatch_settings::folder_for(&state.settings, category, &self.inner.folders)?;
        let id = state.ids.pending();
        let proposal = PendingCapture {
            id,
            url: url_text,
            file_name,
            folder: folder.to_string_lossy().into_owned(),
            category,
            size: capture.size_hint,
            queue: placed.queue,
            queue_reason: placed.reason,
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
                chosen_by,
            },
        );
        self.emit(UiEvent::ConfirmationRequested(capture_for_display(
            &proposal,
        )));
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
                fetch: Fetch {
                    url: parse_url(&proposal.url)?,
                    origin: origin_of(request),
                    chosen_by: pending.chosen_by,
                },
                wanted: Wanted {
                    file_name: answer.file_name,
                    hint_name: Some(proposal.file_name.clone()),
                    folder,
                    ..wanted_by(request)
                },
                placed: Placed::answered(
                    Placed {
                        queue: proposal.queue,
                        reason: proposal.queue_reason.clone(),
                    },
                    answer.queue,
                ),
                start: !answer.later,
                kind: captured(request, None),
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

/// Whether a download in `status` is still to finish: queued, running or paused.
fn unfinished(status: DownloadStatus) -> bool {
    matches!(
        status,
        DownloadStatus::Queued
            | DownloadStatus::Connecting
            | DownloadStatus::Downloading
            | DownloadStatus::Paused
            | DownloadStatus::Processing
    )
}

/// Whether two links are the same address, ignoring a `#fragment` (which never
/// reaches the server).
fn same_address(a: &str, b: &str) -> bool {
    let strip = |link: &str| -> String {
        match Url::parse(link.trim()) {
            Ok(mut url) => {
                url.set_fragment(None);
                url.to_string()
            }
            Err(_) => link.trim().to_string(),
        }
    };
    strip(a) == strip(b)
}

fn wanted_by(capture: &NewDownload) -> Wanted {
    Wanted {
        hint_name: capture.suggested_name.clone(),
        category: direct_category(capture),
        ..Wanted::default()
    }
}

/// `handover`: set while the extension waits for the answer.
fn captured(capture: &NewDownload, handover: Option<String>) -> Kind {
    Kind::Captured {
        handover,
        expect: Expectation {
            size: capture.size_hint,
            name: capture.suggested_name.clone(),
        },
        media: direct_media(capture),
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
            video_key: None,
            instead_of: None,
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

/// A capture waiting for an answer as the UI shows it: no password in its
/// links. The manager keeps the real ones.
fn capture_for_display(capture: &PendingCapture) -> PendingCapture {
    PendingCapture {
        url: wirecatch_logging::redact::url_for_display(&capture.url),
        page_url: wirecatch_logging::redact::optional_url_for_display(capture.page_url.as_deref()),
        ..capture.clone()
    }
}

/// Where a capture goes (D72.3): a file by the browser's name for it, a stream or video page by its page's address and title (D72.11).
fn placed_capture(state: &State, capture: &NewDownload, url: &Url) -> Result<Placed> {
    let request = QueueRequest::Browser(capture.queue);
    if let Some(media) = capture
        .media
        .as_ref()
        .filter(|m| m.kind != MediaKind::Direct)
    {
        let page = capture.page_url.as_deref().and_then(|p| Url::parse(p).ok());
        let title = media.title.as_deref().unwrap_or_default();
        let name = wirecatch_media::file_name(title, "mp4");
        return state.place(
            request,
            &Candidate::video(page.as_ref().unwrap_or(url), &name),
        );
    }
    let name = naming::file_name(capture.suggested_name.as_deref(), url).unwrap_or_default();
    let candidate = Candidate {
        url,
        file_name: &name,
        category: None,
    };
    state.place(request, &candidate)
}

/// How a capture arrives: whether its handover is known already, whether the
/// person is asked first, the queue it goes to and why, and who chose its link.
struct Arrival {
    known: bool,
    ask: bool,
    placed: Placed,
    chosen_by: ChosenBy,
}

/// What makes a captured download work outside the browser: the browser's
/// user agent, and its referrer, cookies and other headers the extension sent.
pub(super) fn origin_of(capture: &NewDownload) -> Origin {
    Origin {
        user_agent: capture.user_agent.clone(),
        headers: headers::for_capture(
            capture.referrer.as_deref(),
            capture.cookies.as_ref().map(Secret::expose),
            &capture.extra_headers,
        ),
        forgot_sign_in: false,
    }
}

/// Waits until the server answered a captured link (`answered`) or its attempt
/// ended (`exited`), but no longer than `timeout` after the capture `arrived`:
/// the browser waits a fixed time from when it sent the capture, so the time
/// the check before this wait took counts too. False when the time ran out.
async fn ended_in_time(
    answered: &CancellationToken,
    exited: &CancellationToken,
    arrived: Instant,
    timeout: Duration,
) -> bool {
    tokio::select! {
        biased;
        () = answered.cancelled() => true,
        () = exited.cancelled() => true,
        () = tokio::time::sleep_until(arrived + timeout) => false,
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use tokio::time::Instant;
    use wirecatch_engine::CancellationToken;

    use super::{ended_in_time, same_address};

    /// The capture timeout counts from the capture's arrival, the check of its
    /// link included, so the answer never comes after the browser stopped
    /// waiting for it.
    #[tokio::test(start_paused = true)]
    async fn the_wait_for_the_server_counts_from_the_captures_arrival() {
        let timeout = Duration::from_secs(8);
        let arrived = Instant::now();
        // The check of the link took most of the time.
        tokio::time::advance(Duration::from_secs(5)).await;
        let never = CancellationToken::new();
        let waiting = Instant::now();
        assert!(!ended_in_time(&never, &never, arrived, timeout).await);
        assert_eq!(waiting.elapsed(), Duration::from_secs(3));

        let done = CancellationToken::new();
        done.cancel();
        assert!(ended_in_time(&done, &never, arrived, timeout).await);
        assert!(ended_in_time(&never, &done, arrived, timeout).await);
        assert_eq!(waiting.elapsed(), Duration::from_secs(3));
    }

    #[test]
    fn the_same_address_ignores_only_the_fragment() {
        assert!(same_address(
            "https://example.com/a.zip#part",
            "https://example.com/a.zip"
        ));
        assert!(same_address(
            "https://EXAMPLE.com/a.zip",
            "https://example.com/a.zip"
        ));
        assert!(!same_address(
            "https://example.com/a.zip?v=2",
            "https://example.com/a.zip"
        ));
        assert!(!same_address(
            "https://example.com/a.zip",
            "https://example.com/b.zip"
        ));
    }
}
