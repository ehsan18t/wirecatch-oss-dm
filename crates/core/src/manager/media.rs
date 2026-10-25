//! Media captures: a stream (HLS, DASH) or a page that needs the media add-on
//! becomes a quality choice for the user, and the chosen option a download.
//!
//! A capture is answered `AwaitingConfirmation` at once. The stream or page is
//! then read in the background; the result is a `MediaChoiceRequested` (kept
//! until chosen or dismissed, so a reloaded window gets it back with
//! [`Manager::pending_media_choices`]), or, when nothing can be downloaded, a
//! failed download row whose message says why. A page captured while the media
//! add-on is missing triggers `AddonRequired` and is read as soon as the add-on
//! is installed.
//!
//! When the person chose not to be asked (D50), a video is read without a
//! window and downloads at once with the saved choices, announced with
//! `Captured`; the window opens only when the video does not offer the saved
//! resolution and its fallback is to ask, when the media add-on must be
//! installed first, or when the download cannot be added as chosen.

use std::collections::HashMap;
use std::path::PathBuf;

use tracing::warn;
use wirecatch_addons::Check;
use wirecatch_engine::{CancellationToken, DownloadOptions, PrivateNetwork, ProxyChoice, Url};
use wirecatch_media::MediaTools;
use wirecatch_media::choice::{self, Offer, Plan, Resolved};
use wirecatch_media::{MediaJob, Pick, StoredMedia, describe, output, ytdlp};
use wirecatch_protocol::app::{
    AddonRequestItem, Category, MediaChoice, MediaReading, MediaSelection, MediaSubtitle,
    MediaSummary, OutputFormat, PortablePath, UiEvent, UserError, VideoQuality,
};
use wirecatch_protocol::extension::{AddAction, AddedDownload, MediaKind, NewDownload};
use wirecatch_protocol::{QuestionId, QueueId};
use wirecatch_sites::local_network::ChosenBy;

use super::Manager;
use super::arrival::{Fetch, Kind, NewEntry, Wanted};
use super::captures::origin_of;
use super::downloads::parse_web_url;
use super::handovers::{Handover, Target};
use super::placement::{Candidate, Placed, QueueRequest};
use super::state::State;
use crate::error::{CoreError, Result};
use crate::headers;
use crate::headers::Origin;

/// How long a page waits for the media add-on before it becomes a failed
/// download, so a question is never asked long after the person moved on.
const ADDON_WAIT_MILLIS: i64 = 30 * 60 * 1000;
/// Most media captures that can wait at once.
const MAX_PENDING_MEDIA: usize = 100;
/// Most captures of one link that can share one choice.
const MAX_HANDOVERS: usize = 32;

/// Where a media capture came from.
#[derive(Debug, Clone)]
pub(super) struct MediaSource {
    /// The playlist or manifest, or the page.
    pub fetch: Fetch,
    pub kind: MediaKind,
    pub title: Option<String>,
    pub page_url: Option<String>,
}

/// A media capture that is not a download yet.
pub(super) struct MediaPending {
    /// When it was captured, in Unix milliseconds.
    pub added_at: i64,
    pub source: MediaSource,
    /// Where it goes for now; placed again once its window proposes a name (D72.11).
    pub placed: Placed,
    pub request: QueueRequest,
    /// The extension's handover ids for it.
    pub handovers: Vec<String>,
    pub stage: MediaStage,
    /// The site and id of the video, once its page was read (see
    /// [`video_key`]); another capture of the same video joins this one.
    pub video: Option<String>,
    /// Read without a window, as the person chose not to be asked about videos
    /// (D50): no `MediaAnalysing`, and not in [`Manager::media_being_read`].
    pub quiet: bool,
    /// The folder the person chose when adding the link in the app.
    pub folder: Option<PathBuf>,
    /// Added paused: a download made without asking waits in its queue.
    pub later: bool,
    /// A link added in the app whose server answered with a web page (D51):
    /// when no video is found on it, the page downloads as it is.
    pub page_otherwise: bool,
}

/// How a capture or a link added in the app waits for its quality to be chosen.
pub(super) struct Held {
    /// The extension's handover ids for it; none for a link added in the app.
    pub handovers: Vec<String>,
    /// See [`MediaPending::folder`].
    pub folder: Option<PathBuf>,
    /// See [`MediaPending::later`].
    pub later: bool,
    /// See [`MediaPending::page_otherwise`].
    pub page_otherwise: bool,
    pub request: QueueRequest,
}

impl Held {
    /// A capture from the browser, under its handover id.
    fn capture(handover: String, queue: Option<QueueId>) -> Self {
        Self {
            handovers: vec![handover],
            folder: None,
            later: false,
            page_otherwise: false,
            request: QueueRequest::Browser(queue),
        }
    }
}

/// Refuses one more video while [`MAX_PENDING_MEDIA`] are waiting.
pub(super) fn check_room(state: &State) -> Result<()> {
    if state.media.len() >= MAX_PENDING_MEDIA {
        return Err(CoreError::LimitReached(format!(
            "{MAX_PENDING_MEDIA} videos are already waiting for your choice. Answer some of them first."
        )));
    }
    Ok(())
}

/// Who picked a video's quality.
enum Picker {
    /// The person, in the quality window: the choices are remembered for next time.
    Person,
    /// The saved choices, without asking (D50); `instead_of` is the saved
    /// resolution the video did not offer.
    Saved { instead_of: Option<String> },
}

/// What reading a stream or page found: its title, the qualities offered and,
/// for a page, which video it is.
pub(super) struct Found {
    pub title: Option<String>,
    pub offers: Vec<Offer>,
    pub video: Option<String>,
    /// The subtitles a page offers, human-written ones first (D24).
    pub subtitles: Vec<MediaSubtitle>,
    /// The page has chapters.
    pub chapters: bool,
    pub duration_ms: Option<u64>,
}

/// Which video a page is, as yt-dlp names it: the extractor and the video's id
/// there, for example `Youtube:dQw4w9WgXcQ`. `None` when yt-dlp did not say.
/// Two links that differ only in extra parameters (`&t=30s`, `&pp=...`) give
/// the same key.
pub(super) fn video_key(extractor: Option<&str>, id: Option<&str>) -> Option<String> {
    let extractor = extractor.map(str::trim).filter(|e| !e.is_empty())?;
    let id = id.map(str::trim).filter(|i| !i.is_empty())?;
    Some(format!("{extractor}:{id}"))
}

impl MediaSource {
    pub(super) fn page(fetch: Fetch) -> Self {
        Self {
            fetch,
            kind: MediaKind::Page,
            title: None,
            page_url: None,
        }
    }

    fn of(capture: &NewDownload, url: Url, kind: MediaKind, chosen_by: ChosenBy) -> Self {
        Self {
            fetch: Fetch {
                url,
                origin: origin_of(capture),
                chosen_by,
            },
            kind,
            title: capture.media.as_ref().and_then(|m| m.title.clone()),
            page_url: capture.page_url.clone(),
        }
    }
}

pub(super) enum MediaStage {
    /// Being read; cancelled when dismissed or withdrawn.
    Analysing(CancellationToken),
    /// A page waiting for the media add-on.
    NeedsAddon,
    /// Waiting for the user's choice.
    Ready {
        choice: Box<MediaChoice>,
        plans: HashMap<String, Plan>,
    },
}

impl Manager {
    /// Takes a captured stream or media page: answers `AwaitingConfirmation` and
    /// reads it in the background.
    pub(super) fn add_media_capture(
        &self,
        capture: &NewDownload,
        placed: Placed,
        chosen_by: ChosenBy,
    ) -> Result<AddedDownload> {
        let url = parse_web_url(&capture.url)?;
        let kind = capture.media.as_ref().map_or(MediaKind::Page, |m| m.kind);
        let mut state = self.lock_open()?;
        if let Some(known) = state.handovers.get(&capture.handover_id) {
            return Ok(known.reply.clone());
        }
        let reply = AddedDownload {
            id: None,
            action: AddAction::AwaitingConfirmation,
        };
        // The same media captured again joins the one already waiting.
        if let Some((&id, existing)) = state
            .media
            .iter_mut()
            .find(|(_, m)| m.source.fetch.url == url && m.source.kind == kind)
        {
            if existing.handovers.len() >= MAX_HANDOVERS {
                return Err(CoreError::LimitReached(
                    "This video was sent many times and is already waiting for your choice. Answer that first.".into(),
                ));
            }
            existing.handovers.push(capture.handover_id.clone());
            let handover = Handover {
                reply: reply.clone(),
                target: Target::Pending(id),
                deciding: None,
            };
            state.handovers.insert(&capture.handover_id, handover);
            return Ok(reply);
        }
        check_room(&state)?;
        let source = MediaSource::of(capture, url, kind, chosen_by);
        let title = capture
            .media
            .as_ref()
            .and_then(|m| m.title.clone())
            .unwrap_or_else(|| {
                wirecatch_logging::redact::url_for_display(source.fetch.url.as_str())
            });
        let held = Held::capture(capture.handover_id.clone(), capture.queue);
        let id = self.hold_media(&mut state, source, placed, held);
        // Read without a window: the person hears it was added now, not seconds later when its
        // download starts (D67).
        if state.media.get(&id).is_some_and(|m| m.quiet) {
            self.emit(UiEvent::MediaCaptured { id, title });
        }
        state.handovers.insert(
            &capture.handover_id,
            Handover {
                reply: reply.clone(),
                target: Target::Pending(id),
                deciding: None,
            },
        );
        Ok(reply)
    }

    /// Holds `source` until its quality is chosen: reads it at once, or a page
    /// once the media add-on is installed, which the person is asked for.
    /// Returns its question id.
    pub(super) fn hold_media(
        &self,
        state: &mut State,
        source: MediaSource,
        placed: Placed,
        held: Held,
    ) -> QuestionId {
        let id = state.ids.pending();
        let needs_addon = source.kind == MediaKind::Page && state.addon.tools.is_none();
        // A window for the add-on opens anyway, and then shows the reading too.
        let quiet = !state.settings.media.ask_each_time && !needs_addon;
        let token = CancellationToken::new();
        let first_stage = if needs_addon {
            MediaStage::NeedsAddon
        } else {
            MediaStage::Analysing(token.clone())
        };
        let shown = wirecatch_logging::redact::url_for_display(source.fetch.url.as_str());
        state.media.insert(
            id,
            MediaPending {
                added_at: self.inner.clock.unix_millis(),
                source,
                placed,
                request: held.request,
                handovers: held.handovers,
                stage: first_stage,
                video: None,
                quiet,
                folder: held.folder,
                later: held.later,
                page_otherwise: held.page_otherwise,
            },
        );
        if needs_addon {
            let addon = self.addon_info(&state.addon);
            self.emit(UiEvent::AddonRequired {
                addon,
                url: shown,
                id,
            });
        } else {
            self.spawn_analysis(id, token);
        }
        id
    }

    /// Media waiting for the user's choice of quality, oldest first. Only
    /// [`dismiss_media_choice`](Self::dismiss_media_choice) and
    /// [`choose_media`](Self::choose_media) take one away (or the browser
    /// withdrawing it), so a window that was closed or reloaded gets them back.
    #[must_use]
    pub fn pending_media_choices(&self) -> Vec<MediaChoice> {
        self.lock()
            .media
            .values()
            .filter_map(|m| match &m.stage {
                MediaStage::Ready { choice, .. } => Some(choice_for_display(choice)),
                _ => None,
            })
            .collect()
    }

    /// Captured streams and pages being read for their qualities right now, oldest
    /// first, so a window opened later can say so. Each ends with a
    /// `MediaChoiceRequested` or a `ConfirmationClosed` with its id.
    #[must_use]
    pub fn media_being_read(&self) -> Vec<MediaReading> {
        self.lock()
            .media
            .iter()
            .filter(|(_, m)| !m.quiet && matches!(m.stage, MediaStage::Analysing(_)))
            .map(|(id, m)| MediaReading {
                id: *id,
                url: wirecatch_logging::redact::url_for_display(m.source.fetch.url.as_str()),
            })
            .collect()
    }

    /// Downloads the option the user chose for a [`MediaChoice`], with the
    /// user's changes, or with `selection.later` adds it to its queue paused.
    /// Returns the new download's id; its row also arrives with the next
    /// `DownloadsChanged`.
    ///
    /// The selection may save it in another format, with subtitles and chapters
    /// (D24); what it asks for must be among what the choice offered. The
    /// choices are remembered in the settings (separately for videos and for
    /// audio only), so the next window starts from them, with the resolution
    /// unless the video did not offer the saved one, and whether to ask for
    /// the next video ([`MediaSelection::every_video`], D50).
    ///
    /// An option whose video and audio must be joined, or that is converted,
    /// needs the media add-on; without it this fails with
    /// [`CoreError::AddonRequired`], tells the UI with `AddonRequired`, and
    /// keeps the choice so it can be made again.
    ///
    /// # Errors
    ///
    /// [`CoreError::ConfirmationNotFound`] when the choice was already made, dismissed
    /// or withdrawn, or is not ready; [`CoreError::Invalid`] when the selection asks
    /// for something the choice did not offer; [`CoreError::AddonRequired`] as
    /// described above; errors adding the download (its name, folder or queue), in
    /// which case the choice stays open; and [`CoreError::Closed`] once the manager has
    /// shut down.
    pub async fn choose_media(
        &self,
        id: QuestionId,
        selection: MediaSelection,
    ) -> Result<wirecatch_protocol::DownloadId> {
        self.choose(id, selection, Picker::Person).await
    }

    /// Downloads what the saved choices pick for a video, without asking (D50),
    /// and announces it as a capture. When it cannot be added as chosen (its
    /// folder is gone, say), the quality window opens, so the person can fix it.
    async fn choose_without_asking(
        &self,
        id: QuestionId,
        selection: MediaSelection,
        instead_of: Option<String>,
    ) {
        match self
            .choose(id, selection, Picker::Saved { instead_of })
            .await
        {
            Ok(row) => {
                let state = self.lock();
                if let Ok(entry) = state.entry(row) {
                    self.emit(UiEvent::Captured(entry.summary()));
                }
            }
            Err(e) => {
                warn!(error = %e, "a video could not be downloaded with the saved choices");
                let state = self.lock();
                if let Some(MediaPending {
                    stage: MediaStage::Ready { choice, .. },
                    ..
                }) = state.media.get(&id)
                {
                    self.emit(UiEvent::MediaChoiceRequested(choice_for_display(choice)));
                }
            }
        }
    }

    fn spawn_choice_without_asking(
        &self,
        id: QuestionId,
        selection: MediaSelection,
        instead_of: Option<String>,
    ) {
        let manager = self.clone();
        self.spawn(async move {
            manager
                .choose_without_asking(id, selection, instead_of)
                .await;
        });
    }

    async fn choose(
        &self,
        id: QuestionId,
        selection: MediaSelection,
        picker: Picker,
    ) -> Result<wirecatch_protocol::DownloadId> {
        let (pending, plan, choice, resolved) = {
            let mut state = self.lock_open()?;
            let Some(MediaPending {
                stage: MediaStage::Ready { plans, choice },
                source,
                ..
            }) = state.media.get(&id)
            else {
                return Err(CoreError::ConfirmationNotFound(id));
            };
            let plan = plans.get(&selection.option_id).cloned().ok_or_else(|| {
                CoreError::Invalid(
                    "That quality is not offered for this video. Choose one of the listed options."
                        .into(),
                )
            })?;
            let resolved = plan
                .resolve(choice, &selection)
                .map_err(|e| CoreError::Invalid(e.to_string()))?;
            if resolved.needs_ffmpeg && state.addon.tools.is_none() {
                let (choice_url, addon) =
                    (source.fetch.url.to_string(), self.addon_info(&state.addon));
                self.emit(UiEvent::AddonRequired {
                    addon,
                    url: wirecatch_logging::redact::url_for_display(&choice_url),
                    id,
                });
                return Err(CoreError::AddonRequired);
            }
            let choice = choice.clone();
            let Some(pending) = state.media.remove(&id) else {
                return Err(CoreError::ConfirmationNotFound(id));
            };
            (pending, plan, choice, resolved)
        };
        let instead_of = match &picker {
            Picker::Person => None,
            Picker::Saved { instead_of } => instead_of.clone(),
        };
        match self
            .insert_chosen(
                &pending,
                &plan,
                &resolved,
                &choice,
                selection.clone(),
                instead_of,
            )
            .await
        {
            Ok(row) => {
                {
                    let mut state = self.lock();
                    for handover in &pending.handovers {
                        state.handovers.retarget(handover, Target::Confirmed(row));
                    }
                }
                self.emit(UiEvent::ConfirmationClosed { id });
                if matches!(picker, Picker::Person) {
                    self.remember_media_choice(&choice, &selection);
                }
                Ok(row)
            }
            Err(e) => {
                // Kept, so the user can fix the problem and choose again.
                if let Ok(mut state) = self.lock_open() {
                    state.media.insert(id, pending);
                }
                Err(e)
            }
        }
    }

    /// Drops a media choice (or a capture still being read or waiting for the
    /// add-on); nothing is downloaded.
    ///
    /// # Errors
    ///
    /// [`CoreError::ConfirmationNotFound`] when there is no such choice, and
    /// [`CoreError::Closed`] once the manager has shut down.
    pub fn dismiss_media_choice(&self, id: QuestionId) -> Result<()> {
        let mut state = self.lock_open()?;
        let pending = state
            .media
            .remove(&id)
            .ok_or(CoreError::ConfirmationNotFound(id))?;
        if let MediaStage::Analysing(token) = &pending.stage {
            token.cancel();
        }
        for handover in &pending.handovers {
            state.handovers.retarget(handover, Target::Dismissed);
        }
        self.emit(UiEvent::ConfirmationClosed { id });
        Ok(())
    }

    /// The browser took back one capture of media `id`; when none is left, the
    /// media is dropped.
    pub(super) fn withdraw_media_handover(
        &self,
        state: &mut State,
        id: QuestionId,
        handover: &str,
    ) {
        let Some(pending) = state.media.get_mut(&id) else {
            return;
        };
        pending.handovers.retain(|h| h != handover);
        if pending.handovers.is_empty()
            && let Some(pending) = state.media.remove(&id)
        {
            if let MediaStage::Analysing(token) = &pending.stage {
                token.cancel();
            }
            self.emit(UiEvent::ConfirmationClosed { id });
        }
    }

    /// The media add-on was installed: read the pages that waited for it.
    pub(super) fn media_addon_ready(&self) {
        // A page that waited too long becomes a failed download, not a question
        // the person no longer expects.
        self.expire_addon_waits();
        let mut started = Vec::new();
        {
            let mut state = self.lock();
            for (id, pending) in &mut state.media {
                if matches!(pending.stage, MediaStage::NeedsAddon) {
                    let token = CancellationToken::new();
                    pending.stage = MediaStage::Analysing(token.clone());
                    started.push((*id, token));
                }
            }
        }
        for (id, token) in started {
            self.spawn_analysis(id, token);
        }
    }

    /// Pages waiting for the media add-on, oldest first, so a window opened
    /// later can offer the install again. [`dismiss_media_choice`] with an
    /// item's id drops it. A page waits at most 30 minutes; then it becomes a
    /// failed download whose Retry works once the add-on is installed.
    ///
    /// [`dismiss_media_choice`]: Self::dismiss_media_choice
    #[must_use]
    pub fn pending_addon_requests(&self) -> Vec<AddonRequestItem> {
        self.expire_addon_waits();
        let state = self.lock();
        let addon = self.addon_info(&state.addon);
        state
            .media
            .iter()
            .filter(|(_, m)| matches!(m.stage, MediaStage::NeedsAddon))
            .map(|(id, m)| AddonRequestItem {
                id: *id,
                addon: addon.clone(),
                url: wirecatch_logging::redact::url_for_display(m.source.fetch.url.as_str()),
            })
            .collect()
    }

    /// Turns pages that waited longer than [`ADDON_WAIT_MILLIS`] into failed
    /// downloads.
    pub(super) fn expire_addon_waits(&self) {
        let now = self.inner.clock.unix_millis();
        self.fail_addon_waits(
            |m| now.saturating_sub(m.added_at) >= ADDON_WAIT_MILLIS,
            "This video waited for the media add-on for a long time, so Wirecatch stopped waiting. Install the media add-on in Settings, Add-ons, then try again.",
        );
    }

    /// An install the person started failed: the pages waiting for it become
    /// failed downloads that say so.
    pub(super) fn media_addon_failed(&self) {
        self.fail_addon_waits(
            |_| true,
            "This video needs the media add-on, which could not be installed. Install the media add-on in Settings, Add-ons, then try again.",
        );
    }

    fn fail_addon_waits(&self, which: impl Fn(&MediaPending) -> bool, message: &str) {
        let failed: Vec<(QuestionId, MediaPending)> = {
            let mut state = self.lock();
            let ids: Vec<QuestionId> = state
                .media
                .iter()
                .filter(|(_, m)| matches!(m.stage, MediaStage::NeedsAddon) && which(m))
                .map(|(id, _)| *id)
                .collect();
            ids.into_iter()
                .filter_map(|id| state.media.remove(&id).map(|m| (id, m)))
                .collect()
        };
        for (id, pending) in failed {
            self.emit(UiEvent::ConfirmationClosed { id });
            let manager = self.clone();
            let error = UserError {
                message: message.to_string(),
                fix: Some(wirecatch_protocol::app::FixAction::Retry),
            };
            self.spawn(async move { manager.record_media_failure(pending, error).await });
        }
    }

    fn spawn_analysis(&self, id: QuestionId, token: CancellationToken) {
        let manager = self.clone();
        self.spawn(async move { manager.analyse(id, token).await });
    }

    /// Reads a stream or page and offers its qualities.
    async fn analyse(&self, id: QuestionId, token: CancellationToken) {
        let (source, tools, policy, quiet) = {
            let state = self.lock();
            let Some(pending) = state.media.get(&id) else {
                return;
            };
            (
                pending.source.clone(),
                state.addon.tools.clone(),
                self.run_policy(&state, pending.placed.queue),
                pending.quiet,
            )
        };
        if !quiet {
            self.emit(UiEvent::MediaAnalysing {
                id,
                url: wirecatch_logging::redact::url_for_display(source.fetch.url.as_str()),
            });
        }
        let found = match source.kind {
            MediaKind::Page => match &tools {
                Some(tools) => {
                    let found = self.read_page(&source, tools, policy.proxy, &token).await;
                    if tools.is_broken() {
                        self.rescan_addon(Check::Hashes).await;
                    }
                    found
                }
                None => Err(addon_missing()),
            },
            _ => {
                self.read_stream(
                    &source,
                    tools.is_some(),
                    policy.connections,
                    policy.proxy,
                    &token,
                )
                .await
            }
        };
        if token.is_cancelled() {
            return;
        }
        if let Some((pending, error)) = self.offer(id, found) {
            warn!(error = %error.message, "media could not be read");
            // The reading is over; the failed download (or the page) that follows says why.
            self.emit(UiEvent::ConfirmationClosed { id });
            if pending.page_otherwise {
                self.add_page_as_file(pending).await;
            } else {
                self.record_media_failure(pending, error).await;
            }
        }
    }

    /// Turns what was found into a choice for the user, or, when the person
    /// chose not to be asked and nothing needs them (D50), into a download with
    /// the saved choices. On failure returns the pending media, taken out, and
    /// why.
    fn offer(
        &self,
        id: QuestionId,
        found: std::result::Result<Found, UserError>,
    ) -> Option<(MediaPending, UserError)> {
        let mut state = self.lock();
        let pending = state.media.get(&id)?;
        if !matches!(pending.stage, MediaStage::Analysing(_)) {
            return None;
        }
        match found {
            Ok(Found {
                title,
                offers,
                video,
                subtitles,
                chapters,
                duration_ms,
            }) => {
                if let Some(video) = &video
                    && self.join_same_video(&mut state, id, video)
                {
                    return None;
                }
                let pending = state.media.get(&id)?;
                let title = title
                    .or_else(|| pending.source.title.clone())
                    .filter(|t| !t.trim().is_empty())
                    .unwrap_or_else(|| title_from_url(&pending.source.fetch.url));
                let audio_only = offers.iter().all(|(o, _)| o.audio_only);
                let category = if audio_only {
                    Category::Audio
                } else {
                    Category::Video
                };
                let folder = self.folder_for(&state, pending, category);
                let placed = placed_by_title(&state, pending, &title, category);
                let later = pending.later;
                let page_url = pending.source.page_url.clone().or_else(|| {
                    (pending.source.kind == MediaKind::Page)
                        .then(|| pending.source.fetch.url.to_string())
                });
                let options: Vec<_> = offers.iter().map(|(o, _)| o.clone()).collect();
                let start = state.settings.media.start(&options, &subtitles, chapters);
                let choice = MediaChoice {
                    id,
                    title,
                    page_url,
                    options,
                    folder: folder.to_string_lossy().into_owned(),
                    queue: placed.queue,
                    queue_reason: placed.reason.clone(),
                    subtitles,
                    chapters,
                    duration_ms,
                    start: start.selection,
                    missing_quality: start.missing,
                };
                let plans: HashMap<String, Plan> =
                    offers.into_iter().map(|(o, p)| (o.id, p)).collect();
                // The media add-on is installed only when the person says so.
                let ready = plans
                    .get(&choice.start.option_id)
                    .and_then(|plan| plan.resolve(&choice, &choice.start).ok())
                    .is_some_and(|r| !r.needs_ffmpeg || state.addon.tools.is_some());
                let without_asking = !start.asks && ready;
                if let Some(pending) = state.media.get_mut(&id) {
                    pending.stage = MediaStage::Ready {
                        choice: Box::new(choice.clone()),
                        plans,
                    };
                    pending.video = video;
                    pending.placed = placed;
                }
                if without_asking {
                    let (mut selection, instead_of) = (choice.start, choice.missing_quality);
                    selection.later = later;
                    self.spawn_choice_without_asking(id, selection, instead_of);
                } else {
                    self.emit(UiEvent::MediaChoiceRequested(choice_for_display(&choice)));
                }
                None
            }
            Err(error) => state.media.remove(&id).map(|pending| (pending, error)),
        }
    }

    async fn read_stream(
        &self,
        source: &MediaSource,
        ffmpeg: bool,
        connections: usize,
        proxy: ProxyChoice,
        token: &CancellationToken,
    ) -> std::result::Result<Found, UserError> {
        let mut options = media_options(
            &source.fetch.origin,
            source.fetch.chosen_by.private_network(),
            proxy,
        );
        options.connections = connections;
        let info = wirecatch_streams::inspect(
            &source.fetch.url,
            &options,
            source.fetch.chosen_by.private_network(),
            token.clone(),
        )
        .await
        .map_err(|e| {
            describe::stream(
                &e,
                &self.inner.folders.downloads,
                None,
                source.fetch.chosen_by,
            )
        })?;
        Ok(Found {
            title: None,
            offers: choice::from_stream(
                &info,
                if ffmpeg {
                    choice::Ffmpeg::Installed
                } else {
                    choice::Ffmpeg::Missing
                },
            )?,
            video: None,
            subtitles: Vec::new(),
            chapters: false,
            duration_ms: None,
        })
    }

    async fn read_page(
        &self,
        source: &MediaSource,
        tools: &MediaTools,
        proxy: ProxyChoice,
        token: &CancellationToken,
    ) -> std::result::Result<Found, UserError> {
        let cookies = cookie_header(&source.fetch.origin);
        let proxy = wirecatch_settings::proxy::ytdlp_proxy(&proxy);
        let ctx = ytdlp::Context {
            tools,
            page: &source.fetch.url,
            cookies: cookies.as_deref(),
            user_agent: source.fetch.origin.user_agent.as_deref(),
            temp_dir: &self.inner.temp_dir,
            timeout: self.inner.media_timeout,
            proxy: proxy.as_deref(),
        };
        let info = ytdlp::extract(&ctx, token)
            .await
            .map_err(|e| describe::ytdlp(&e, tools))?;
        Ok(Found {
            title: info.title.clone(),
            offers: choice::from_page(&info)?,
            video: video_key(info.extractor_key.as_deref(), info.id.as_deref()),
            subtitles: output::subtitles(&info)
                .into_iter()
                .map(|(subtitle, _)| subtitle)
                .collect(),
            chapters: output::has_chapters(&info.chapters),
            duration_ms: choice::duration_ms(&info),
        })
    }

    /// Folds media `id`, just read and found to be `video`, into another
    /// question about the same video, so one video is never asked about twice
    /// (two links to one video, say `watch?v=ID` and `watch?v=ID&t=30s`). Its
    /// browser captures join the other question and `id` closes. Returns
    /// whether it was folded.
    fn join_same_video(&self, state: &mut State, id: QuestionId, video: &str) -> bool {
        let Some(&other) = state
            .media
            .iter()
            .find(|(other, m)| **other != id && m.video.as_deref() == Some(video))
            .map(|(other, _)| other)
        else {
            return false;
        };
        let room = state
            .media
            .get(&other)
            .map_or(0, |m| MAX_HANDOVERS.saturating_sub(m.handovers.len()));
        let joining = state.media.get(&id).map_or(0, |m| m.handovers.len());
        if joining > room {
            return false;
        }
        let Some(pending) = state.media.remove(&id) else {
            return false;
        };
        for handover in &pending.handovers {
            state.handovers.retarget(handover, Target::Pending(other));
        }
        if let Some(existing) = state.media.get_mut(&other) {
            existing.handovers.extend(pending.handovers);
        }
        self.emit(UiEvent::ConfirmationClosed { id });
        true
    }

    /// Where a video waiting as `pending` is saved: the folder chosen when it
    /// was added, else the folder for `category`.
    fn folder_for(&self, state: &State, pending: &MediaPending, category: Category) -> PathBuf {
        pending.folder.clone().unwrap_or_else(|| {
            wirecatch_settings::folder_for(&state.settings, category, &self.inner.folders)
                .unwrap_or_else(|_| self.inner.folders.downloads.clone())
        })
    }

    /// Media that cannot be offered becomes a failed download, so the reason is
    /// in the list with a Retry button (which downloads the best quality).
    async fn record_media_failure(&self, pending: MediaPending, error: UserError) {
        let source = &pending.source;
        let title = source
            .title
            .clone()
            .unwrap_or_else(|| title_from_url(&source.fetch.url));
        let job = match source.kind {
            MediaKind::Page => MediaJob::Page {
                video: None,
                audio: None,
                output: None,
                keep_ext: None,
            },
            _ => MediaJob::Stream {
                video: Pick::Auto,
                audio: Pick::Auto,
            },
        };
        let added = self
            .insert(NewEntry {
                fetch: source.fetch.clone(),
                wanted: Wanted {
                    file_name: Some(wirecatch_media::file_name(&title, "mp4")),
                    category: Some(Category::Video),
                    ..Wanted::default()
                },
                placed: pending.placed.clone(),
                start: false,
                kind: Kind::Media(StoredMedia {
                    summary: MediaSummary {
                        quality: "Best available".into(),
                        source: source.kind,
                        video_key: pending.video.clone(),
                        instead_of: None,
                    },
                    job: Some(job),
                }),
            })
            .await;
        let row = match added {
            Ok(Some(row)) => row,
            Ok(None) => return,
            Err(e) => {
                warn!(error = %e, "could not record media that failed");
                return;
            }
        };
        let mut state = self.lock();
        for handover in &pending.handovers {
            state
                .handovers
                .retarget(handover, Target::Confirmed(row.id));
        }
        if let Some(entry) = state.downloads.get_mut(&row.id) {
            entry.row.status = wirecatch_protocol::app::DownloadStatus::Failed;
            entry.row.error = Some(super::sign_in::explain(&entry.origin, error));
        }
        self.save(&mut state, row.id);
        self.reconcile(&mut state);
    }

    async fn insert_chosen(
        &self,
        pending: &MediaPending,
        plan: &Plan,
        resolved: &Resolved,
        choice: &MediaChoice,
        selection: MediaSelection,
        instead_of: Option<String>,
    ) -> Result<wirecatch_protocol::DownloadId> {
        let folder = match selection.folder {
            Some(folder) => self.inner.folders.resolve(&PortablePath(folder))?,
            None => PathBuf::from(&choice.folder),
        };
        // A typed name keeps its words; the finished file gets the extension
        // of the format it is in (see `media_run::final_path`).
        let name = selection
            .file_name
            .filter(|n| !n.trim().is_empty())
            .unwrap_or_else(|| wirecatch_media::file_name(&choice.title, &resolved.ext));
        let added = self
            .insert(NewEntry {
                fetch: pending.source.fetch.clone(),
                wanted: Wanted {
                    file_name: Some(name),
                    folder: Some(folder),
                    category: Some(if plan.audio_only {
                        Category::Audio
                    } else {
                        Category::Video
                    }),
                    hint_name: None,
                },
                placed: Placed::answered(
                    Placed {
                        queue: choice.queue,
                        reason: choice.queue_reason.clone(),
                    },
                    selection.queue,
                ),
                start: !selection.later,
                kind: Kind::Media(StoredMedia {
                    summary: MediaSummary {
                        quality: plan.quality.clone(),
                        source: pending.source.kind,
                        video_key: pending.video.clone(),
                        instead_of,
                    },
                    job: Some(resolved.job.clone()),
                }),
            })
            .await?;
        added.map(|row| row.id).ok_or_else(|| {
            CoreError::Internal("a chosen media download was matched to a handover".into())
        })
    }
}

impl Manager {
    /// Saves a download whose conversion failed (its error offers
    /// [`FixAction::KeepOriginal`](wirecatch_protocol::app::FixAction::KeepOriginal))
    /// as it was downloaded instead (D24): the conversion, subtitles and
    /// chapters are dropped, and it finishes from the parts already downloaded,
    /// in their own format.
    ///
    /// # Errors
    ///
    /// [`CoreError::WrongState`] when the download is not a failed conversion offering
    /// this, [`CoreError::DownloadNotFound`] when there is no download `id`, and
    /// [`CoreError::Closed`] once the manager has shut down.
    pub fn keep_original(&self, id: wirecatch_protocol::DownloadId) -> Result<()> {
        {
            let mut state = self.lock_open()?;
            let entry = state.entry_mut(id)?;
            let offered = entry.row.status == wirecatch_protocol::app::DownloadStatus::Failed
                && entry.row.error.as_ref().and_then(|e| e.fix)
                    == Some(wirecatch_protocol::app::FixAction::KeepOriginal);
            let Some(MediaJob::Page {
                output: output @ Some(_),
                keep_ext,
                ..
            }) = entry.media_job.as_mut().filter(|_| offered)
            else {
                return Err(CoreError::WrongState(
                    "This download is not waiting to be kept as it was downloaded.",
                ));
            };
            *keep_ext = output.take().map(|o| o.original_ext);
            self.save(&mut state, id);
        }
        self.retry(id)
    }

    /// Keeps what was chosen for a video (or for audio only) as the next
    /// window's starting point, in the settings (D24), and whether the next
    /// video asks at all (D50). The resolution is kept when the video offered
    /// the saved one and another was picked; a quality offered in its original
    /// format only (a stream's) keeps the saved format, subtitles and chapters.
    fn remember_media_choice(&self, choice: &MediaChoice, selection: &MediaSelection) {
        let Some(option) = choice.options.iter().find(|o| o.id == selection.option_id) else {
            return;
        };
        let Ok(mut state) = self.lock_open() else {
            return;
        };
        let mut new = state.settings.clone();
        if let Some(every_video) = selection.every_video {
            new.media.ask_each_time = !every_video;
        }
        if choice.missing_quality.is_none() && selection.option_id != choice.start.option_id {
            if option.audio_only {
                new.media.quality = VideoQuality::AudioOnly;
            } else if let Some(height) = option.height {
                new.media.quality = VideoQuality::Height(height);
            }
        }
        if option.outputs.len() > 1 {
            remember_output(choice, selection, &mut new.media, option.audio_only);
        }
        match wirecatch_settings::validate(
            new,
            &state.queue_list(),
            &self.inner.folders,
            &state.suffixes,
        ) {
            Ok(new) => self.apply_settings(&mut state, new),
            Err(e) => warn!(error = %e, "could not remember the media choices"),
        }
    }
}

/// Where a video goes once its window proposes a name from `title` (D72.11); where it was going when the queue it would join is gone.
fn placed_by_title(
    state: &State,
    pending: &MediaPending,
    title: &str,
    category: Category,
) -> Placed {
    let page = pending
        .source
        .page_url
        .as_deref()
        .and_then(|p| Url::parse(p).ok());
    let name = wirecatch_media::file_name(title, "mp4");
    let candidate = Candidate {
        category: Some(category),
        ..Candidate::video(page.as_ref().unwrap_or(&pending.source.fetch.url), &name)
    };
    state
        .place(pending.request, &candidate)
        .unwrap_or_else(|_| pending.placed.clone())
}

/// Keeps the format, audio quality, subtitles and chapters chosen as the
/// starting point for videos, or for audio only.
fn remember_output(
    choice: &MediaChoice,
    selection: &MediaSelection,
    media: &mut wirecatch_protocol::app::MediaDefaults,
    audio_only: bool,
) {
    let defaults = if audio_only {
        &mut media.audio
    } else {
        &mut media.video
    };
    defaults.format = selection.output;
    if selection.output != OutputFormat::Original && !selection.output.is_lossless() {
        defaults.audio_kbps = selection.audio_kbps;
    }
    if let Some((languages, auto)) =
        output::remembered_subtitles(&choice.subtitles, &selection.subtitles)
    {
        defaults.subtitle_languages = languages;
        defaults.auto_subtitles |= auto;
        if !audio_only {
            defaults.subtitle_files = selection.subtitle_files;
        }
    }
    if choice.chapters {
        defaults.chapters = selection.chapters;
    }
}

/// A quality choice as the UI shows it: no password in the page's link.
fn choice_for_display(choice: &MediaChoice) -> MediaChoice {
    MediaChoice {
        page_url: wirecatch_logging::redact::optional_url_for_display(choice.page_url.as_deref()),
        ..choice.clone()
    }
}

/// Request options carrying the browser's headers for the stream or page,
/// under `private_network`, through `proxy`.
pub(super) fn media_options(
    origin: &Origin,
    private_network: PrivateNetwork,
    proxy: ProxyChoice,
) -> DownloadOptions {
    let mut options = headers::download_options(private_network, proxy);
    options.headers = headers::to_map(&origin.headers);
    if let Some(agent) = &origin.user_agent {
        options.user_agent.clone_from(agent);
    }
    options
}

/// The browser's `Cookie` header, if it sent one.
pub(super) fn cookie_header(origin: &Origin) -> Option<String> {
    origin
        .headers
        .iter()
        .find(|h| h.name.trim().eq_ignore_ascii_case("cookie"))
        .map(|h| h.value.expose().to_owned())
}

pub(super) fn addon_missing() -> UserError {
    UserError {
        message: "This needs the media add-on, which is not installed. Install it from Settings, Add-ons, then try again.".into(),
        fix: Some(wirecatch_protocol::app::FixAction::Retry),
    }
}

/// A title for media whose source gave none: the last part of its address.
fn title_from_url(url: &Url) -> String {
    let last = url
        .path_segments()
        .and_then(|mut s| s.next_back().map(str::to_string))
        .unwrap_or_default();
    let stem = last.rsplit_once('.').map_or(last.as_str(), |(s, _)| s);
    if stem.is_empty() {
        url.host_str().unwrap_or("video").to_string()
    } else {
        stem.to_string()
    }
}
