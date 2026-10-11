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

use std::collections::HashMap;
use std::path::PathBuf;

use tracing::warn;
use wirecatch_engine::{CancellationToken, DownloadOptions, Url};
use wirecatch_protocol::QueueId;
use wirecatch_protocol::app::{
    Category, MediaChoice, MediaSelection, MediaSummary, PortablePath, UiEvent, UserError,
};
use wirecatch_protocol::extension::{AddAction, AddedDownload, MediaKind, NewDownload};

use super::Manager;
use super::captures::origin_of;
use super::downloads::{NewEntry, parse_web_url};
use super::handovers::{Handover, Target};
use super::state::State;
use crate::addons::MediaTools;
use crate::error::{CoreError, Result};
use crate::media::choice::{self, Offer, Plan};
use crate::media::{self, MediaJob, Pick, StoredMedia, ytdlp};
use crate::store::Origin;
use crate::{headers, settings};

/// How long a page waits for the media add-on before it becomes a failed
/// download, so a question is never asked long after the person moved on.
const ADDON_WAIT_MILLIS: i64 = 30 * 60 * 1000;
/// Most media captures that can wait at once.
const MAX_PENDING_MEDIA: usize = 100;
/// Most captures of one link that can share one choice.
const MAX_HANDOVERS: usize = 32;

/// A page waiting for an add-on, as [`Manager::pending_addon_requests`] lists it.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct AddonRequestItem {
    /// Drops it when passed to [`Manager::dismiss_media_choice`]; the same id
    /// as a later media choice for it would have.
    pub id: u64,
    /// The add-on it needs.
    pub addon: wirecatch_protocol::app::AddonInfo,
    /// The page.
    pub url: String,
}

/// A captured stream or page still being read for its qualities, as
/// [`Manager::media_being_read`] lists it.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct MediaReading {
    /// The id its quality choice will have; [`Manager::dismiss_media_choice`]
    /// with it stops the reading.
    pub id: u64,
    /// The stream or page, without any password in it.
    pub url: String,
}

/// Where a media capture came from.
#[derive(Debug, Clone)]
pub(super) struct MediaSource {
    /// The playlist or manifest, or the page.
    pub url: Url,
    pub kind: MediaKind,
    pub title: Option<String>,
    pub page_url: Option<String>,
    pub origin: Origin,
}

/// A media capture that is not a download yet.
pub(super) struct MediaPending {
    /// When it was captured, in Unix milliseconds.
    pub added_at: i64,
    pub source: MediaSource,
    pub queue: QueueId,
    /// The extension's handover ids for it.
    pub handovers: Vec<String>,
    pub stage: MediaStage,
    /// The site and id of the video, once its page was read (see
    /// [`video_key`]); another capture of the same video joins this one.
    pub video: Option<String>,
}

/// What reading a stream or page found: its title, the qualities offered and,
/// for a page, which video it is.
pub(super) struct Found {
    pub title: Option<String>,
    pub offers: Vec<Offer>,
    pub video: Option<String>,
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

pub(super) enum MediaStage {
    /// Being read; cancelled when dismissed or withdrawn.
    Analysing(CancellationToken),
    /// A page waiting for the media add-on.
    NeedsAddon,
    /// Waiting for the user's choice.
    Ready {
        choice: MediaChoice,
        plans: HashMap<String, Plan>,
    },
}

impl Manager {
    /// Takes a captured stream or media page: answers `AwaitingConfirmation` and
    /// reads it in the background.
    pub(super) fn add_media_capture(
        &self,
        capture: NewDownload,
        queue: QueueId,
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
            .find(|(_, m)| m.source.url == url && m.source.kind == kind)
        {
            if existing.handovers.len() >= MAX_HANDOVERS {
                return Err(CoreError::Invalid(
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
        if state.media.len() >= MAX_PENDING_MEDIA {
            return Err(CoreError::Invalid(format!(
                "{MAX_PENDING_MEDIA} videos are already waiting for your choice. Answer some of them first."
            )));
        }
        let id = state.ids.pending();
        let needs_addon = kind == MediaKind::Page && state.addon.tools.is_none();
        let token = CancellationToken::new();
        let stage = if needs_addon {
            MediaStage::NeedsAddon
        } else {
            MediaStage::Analysing(token.clone())
        };
        state.handovers.insert(
            &capture.handover_id,
            Handover {
                reply: reply.clone(),
                target: Target::Pending(id),
                deciding: None,
            },
        );
        let source = MediaSource {
            url: url.clone(),
            kind,
            title: capture.media.as_ref().and_then(|m| m.title.clone()),
            page_url: capture.page_url.clone(),
            origin: origin_of(&capture),
        };
        state.media.insert(
            id,
            MediaPending {
                added_at: self.inner.clock.unix_millis(),
                source,
                queue,
                handovers: vec![capture.handover_id.clone()],
                stage,
                video: None,
            },
        );
        if needs_addon {
            let addon = self.addon_info(&state.addon);
            self.emit(UiEvent::AddonRequired {
                addon,
                url: crate::redact::url_for_display(url.as_str()),
                id,
            });
        } else {
            self.spawn_analysis(id, token);
        }
        Ok(reply)
    }

    /// Media waiting for the user's choice of quality, oldest first. Only
    /// [`dismiss_media_choice`](Self::dismiss_media_choice) and
    /// [`choose_media`](Self::choose_media) take one away (or the browser
    /// withdrawing it), so a window that was closed or reloaded gets them back.
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
    pub fn media_being_read(&self) -> Vec<MediaReading> {
        self.lock()
            .media
            .iter()
            .filter(|(_, m)| matches!(m.stage, MediaStage::Analysing(_)))
            .map(|(id, m)| MediaReading {
                id: *id,
                url: crate::redact::url_for_display(m.source.url.as_str()),
            })
            .collect()
    }

    /// Downloads the option the user chose for a [`MediaChoice`], with the
    /// user's changes, or with `selection.later` adds it to its queue paused.
    /// Returns the new download's id; its row also arrives with the next
    /// `DownloadsChanged`.
    ///
    /// An option whose video and audio must be joined needs the media add-on;
    /// without it this fails with [`CoreError::AddonRequired`], tells the UI with
    /// `AddonRequired`, and keeps the choice so it can be made again.
    pub async fn choose_media(
        &self,
        id: u64,
        selection: MediaSelection,
    ) -> Result<wirecatch_protocol::DownloadId> {
        let (pending, plan, choice) = {
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
            if plan.needs_ffmpeg && state.addon.tools.is_none() {
                let (choice_url, addon) = (source.url.to_string(), self.addon_info(&state.addon));
                self.emit(UiEvent::AddonRequired {
                    addon,
                    url: crate::redact::url_for_display(&choice_url),
                    id,
                });
                return Err(CoreError::AddonRequired);
            }
            let choice = choice.clone();
            let Some(pending) = state.media.remove(&id) else {
                return Err(CoreError::ConfirmationNotFound(id));
            };
            (pending, plan, choice)
        };
        match self
            .insert_chosen(&pending, &plan, &choice, selection)
            .await
        {
            Ok(row) => {
                let mut state = self.lock();
                for handover in &pending.handovers {
                    state.handovers.retarget(handover, Target::Confirmed(row));
                }
                self.emit(UiEvent::ConfirmationClosed { id });
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
    pub fn dismiss_media_choice(&self, id: u64) -> Result<()> {
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
    pub(super) fn withdraw_media_handover(&self, state: &mut State, id: u64, handover: &str) {
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
            for (id, pending) in state.media.iter_mut() {
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
                url: crate::redact::url_for_display(m.source.url.as_str()),
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
        let failed: Vec<(u64, MediaPending)> = {
            let mut state = self.lock();
            let ids: Vec<u64> = state
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

    fn spawn_analysis(&self, id: u64, token: CancellationToken) {
        let manager = self.clone();
        self.spawn(async move { manager.analyse(id, token).await });
    }

    /// Reads a stream or page and offers its qualities.
    async fn analyse(&self, id: u64, token: CancellationToken) {
        let (source, tools, connections) = {
            let state = self.lock();
            let Some(pending) = state.media.get(&id) else {
                return;
            };
            (
                pending.source.clone(),
                state.addon.tools.clone(),
                usize::from(state.settings.connections_per_download),
            )
        };
        self.emit(UiEvent::MediaAnalysing {
            id,
            url: crate::redact::url_for_display(source.url.as_str()),
        });
        let found = match source.kind {
            MediaKind::Page => match &tools {
                Some(tools) => {
                    let found = self.read_page(&source, tools, &token).await;
                    if tools.is_broken() {
                        self.rescan_addon(true).await;
                    }
                    found
                }
                None => Err(addon_missing()),
            },
            _ => {
                self.read_stream(&source, tools.is_some(), connections, &token)
                    .await
            }
        };
        if token.is_cancelled() {
            return;
        }
        if let Some((pending, error)) = self.offer(id, found) {
            warn!(error = %error.message, "media could not be read");
            // The reading is over; the failed download that follows says why.
            self.emit(UiEvent::ConfirmationClosed { id });
            self.record_media_failure(pending, error).await;
        }
    }

    /// Turns what was found into a choice for the user. On failure returns the
    /// pending media, taken out, and why.
    fn offer(
        &self,
        id: u64,
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
                    .unwrap_or_else(|| title_from_url(&pending.source.url));
                let audio_only = offers.iter().all(|(o, _)| o.audio_only);
                let category = if audio_only {
                    Category::Audio
                } else {
                    Category::Video
                };
                let folder = settings::folder_for(&state.settings, category, &self.inner.folders)
                    .unwrap_or_else(|_| self.inner.folders.downloads.clone());
                let page_url = pending.source.page_url.clone().or_else(|| {
                    (pending.source.kind == MediaKind::Page).then(|| pending.source.url.to_string())
                });
                let choice = MediaChoice {
                    id,
                    title,
                    page_url,
                    options: offers.iter().map(|(o, _)| o.clone()).collect(),
                    folder: folder.to_string_lossy().into_owned(),
                    queue: pending.queue,
                };
                let plans = offers.into_iter().map(|(o, p)| (o.id, p)).collect();
                if let Some(pending) = state.media.get_mut(&id) {
                    pending.stage = MediaStage::Ready {
                        choice: choice.clone(),
                        plans,
                    };
                    pending.video = video;
                }
                self.emit(UiEvent::MediaChoiceRequested(choice_for_display(&choice)));
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
        token: &CancellationToken,
    ) -> std::result::Result<Found, UserError> {
        let mut options = media_options(&source.origin);
        options.connections = connections.max(1);
        let info = wirecatch_streams::inspect(&source.url, &options, token.clone())
            .await
            .map_err(|e| super::media_run::stream_error(&e, &self.inner.folders.downloads))?;
        Ok(Found {
            title: None,
            offers: choice::from_stream(&info, ffmpeg)?,
            video: None,
        })
    }

    async fn read_page(
        &self,
        source: &MediaSource,
        tools: &MediaTools,
        token: &CancellationToken,
    ) -> std::result::Result<Found, UserError> {
        let cookies = cookie_header(&source.origin);
        let ctx = ytdlp::Context {
            tools,
            page: &source.url,
            cookies: cookies.as_deref(),
            user_agent: source.origin.user_agent.as_deref(),
            temp_dir: &self.inner.temp_dir,
            timeout: self.inner.media_timeout,
        };
        let info = ytdlp::extract(&ctx, token).await?;
        Ok(Found {
            title: info.title.clone(),
            offers: choice::from_page(&info)?,
            video: video_key(info.extractor_key.as_deref(), info.id.as_deref()),
        })
    }

    /// Folds media `id`, just read and found to be `video`, into another
    /// question about the same video, so one video is never asked about twice
    /// (two links to one video, say `watch?v=ID` and `watch?v=ID&t=30s`). Its
    /// browser captures join the other question and `id` closes. Returns
    /// whether it was folded.
    fn join_same_video(&self, state: &mut State, id: u64, video: &str) -> bool {
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

    /// Media that cannot be offered becomes a failed download, so the reason is
    /// in the list with a Retry button (which downloads the best quality).
    async fn record_media_failure(&self, pending: MediaPending, error: UserError) {
        let source = &pending.source;
        let title = source
            .title
            .clone()
            .unwrap_or_else(|| title_from_url(&source.url));
        let job = match source.kind {
            MediaKind::Page => MediaJob::Page {
                format: "bv*+ba/b".into(),
                video: None,
                audio: None,
            },
            _ => MediaJob::Stream {
                video: Pick::Auto,
                audio: Pick::Auto,
            },
        };
        let added = self
            .insert(NewEntry {
                url: source.url.clone(),
                file_name: Some(media::file_name(&title, "mp4")),
                hint_name: None,
                folder: None,
                queue: pending.queue,
                start: false,
                origin: source.origin.clone(),
                handover: None,
                expect: None,
                media: Some(StoredMedia {
                    summary: MediaSummary {
                        quality: "Best available".into(),
                        source: source.kind,
                        video_key: pending.video.clone(),
                    },
                    job: Some(job),
                }),
                category: Some(Category::Video),
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
        choice: &MediaChoice,
        selection: MediaSelection,
    ) -> Result<wirecatch_protocol::DownloadId> {
        let folder = match selection.folder {
            Some(folder) => self.inner.folders.resolve(&PortablePath(folder))?,
            None => PathBuf::from(&choice.folder),
        };
        let name = selection
            .file_name
            .filter(|n| !n.trim().is_empty())
            .unwrap_or_else(|| media::file_name(&choice.title, &plan.ext));
        let added = self
            .insert(NewEntry {
                url: pending.source.url.clone(),
                file_name: Some(name),
                hint_name: None,
                folder: Some(folder),
                queue: selection.queue.unwrap_or(choice.queue),
                start: !selection.later,
                origin: pending.source.origin.clone(),
                handover: None,
                expect: None,
                media: Some(StoredMedia {
                    summary: MediaSummary {
                        quality: plan.quality.clone(),
                        source: pending.source.kind,
                        video_key: pending.video.clone(),
                    },
                    job: Some(plan.job.clone()),
                }),
                category: Some(if plan.audio_only {
                    Category::Audio
                } else {
                    Category::Video
                }),
            })
            .await?;
        added.map(|row| row.id).ok_or_else(|| {
            CoreError::Internal("a chosen media download was matched to a handover".into())
        })
    }
}

/// Request options carrying the browser's headers for the stream or page.
/// A quality choice as the UI shows it: no password in the page's link.
fn choice_for_display(choice: &MediaChoice) -> MediaChoice {
    MediaChoice {
        page_url: crate::redact::optional_url_for_display(choice.page_url.as_deref()),
        ..choice.clone()
    }
}

pub(super) fn media_options(origin: &Origin) -> DownloadOptions {
    let mut options = DownloadOptions::default();
    options.headers = headers::to_map(&origin.headers);
    if let Some(agent) = &origin.user_agent {
        options.user_agent = agent.clone();
    }
    options
}

/// The browser's `Cookie` header, if it sent one.
pub(super) fn cookie_header(origin: &Origin) -> Option<String> {
    origin
        .headers
        .iter()
        .find(|h| h.name.trim().eq_ignore_ascii_case("cookie"))
        .map(|h| h.value.clone())
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
