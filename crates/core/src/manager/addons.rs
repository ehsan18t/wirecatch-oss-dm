//! Installing, updating and removing the media add-on (D20).
//!
//! Every part is downloaded with the engine (in parallel, resumable), checked
//! against its SHA-256 before anything is unpacked, unpacked into a staging
//! folder, and only then swapped into place. The record of what is installed is
//! written last, and the programs are only ever run when every recorded file is
//! there with its recorded size. yt-dlp keeps up with sites through a quiet
//! update check at most once a day; ffmpeg and Deno change only with the app's
//! pinned versions.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use tracing::{info, warn};
use wirecatch_engine::{
    CancellationToken, DownloadOptions, DownloadRequest, Fetcher, Outcome, Progress, RemoteFile,
    Url,
    header::{ACCEPT, HeaderValue},
};
use wirecatch_protocol::app::{AddonInfo, AddonState, UiEvent};

use super::Manager;
use crate::addons::manifest::{self, AddonComponent, MEDIA_ADDON_ID};
use crate::addons::{self, MediaTools, Record, archive, signature};
use crate::error::{CoreError, Result};
use crate::user_error;

/// How often the progress of an install is reported.
const PROGRESS_EVERY: Duration = Duration::from_millis(250);
/// The update check runs at most this often.
const CHECK_EVERY_MILLIS: i64 = 24 * 60 * 60 * 1000;
/// How often the background task looks whether a check is due.
const LOOK_EVERY: Duration = Duration::from_secs(60 * 60);
/// Why a part did not finish when the install was stopped.
const STOPPED: &str = "It was stopped.";
/// How much larger than announced a release file may be. Release sizes are
/// exact byte counts; the margin is only a small allowance.
const SIZE_MARGIN: u64 = 4096;
/// Largest yt-dlp release file accepted from an update (it is about 40 MB).
const MAX_UPDATE_SIZE: u64 = 200 * 1024 * 1024;
/// A new yt-dlp release is installed (quietly, with the Update button, by a
/// repair or a reinstall) only once it is this old, so a broken or tampered
/// release has time to be noticed and withdrawn (D21.1).
const MATURE_AGE_MILLIS: i64 = 48 * 60 * 60 * 1000;
/// A day, for saying when a release that is not old enough came out.
const DAY_MILLIS: i64 = 24 * 60 * 60 * 1000;
/// How many of the newest yt-dlp releases are looked at for one old enough.
const RELEASES_LOOKED_AT: usize = 10;
/// Largest GitHub release description or checksum list read.
const MAX_METADATA: u64 = 4 * 1024 * 1024;
/// Largest signature file read (yt-dlp's is under 1 KB).
const MAX_SIGNATURE: u64 = 64 * 1024;

/// Why looking up the newest yt-dlp gave nothing.
enum Lookup {
    /// The release information could not be fetched (no connection, server error).
    Unreachable(String),
    /// It was fetched, but the release does not meet the rules and is not offered.
    Refused(String),
    /// The release's checksum list is not signed by yt-dlp's pinned key, so it
    /// is not offered; the reason is worded for a person.
    Unsigned(String),
}

/// The add-on as the manager tracks it.
#[derive(Default)]
pub(super) struct AddonSlot {
    pub record: Record,
    /// The programs, when the add-on is installed and intact.
    pub tools: Option<MediaTools>,
    pub op: Option<AddonOp>,
    /// Why the last install the user started failed.
    pub failure: Option<String>,
    /// yt-dlp releases newer than the installed one, found by the update check
    /// and checked (signature, checksum list, size), newest first.
    pub ytdlp_releases: Vec<NewRelease>,
}

/// A yt-dlp release found by the update check.
#[derive(Debug, Clone)]
pub(super) struct NewRelease {
    pub component: AddonComponent,
    /// When it was published, in Unix milliseconds.
    pub published: i64,
}

impl NewRelease {
    /// Whether it is old enough to install at `now`.
    fn mature(&self, now: i64) -> bool {
        now.saturating_sub(self.published) >= MATURE_AGE_MILLIS
    }
}

/// An install or update in progress.
pub(super) struct AddonOp {
    pub cancel: CancellationToken,
    /// Cancelled when it has ended, however it ended.
    pub ended: CancellationToken,
    pub done: u64,
    pub total: u64,
    /// A background update: a failure is logged, not shown.
    pub quiet: bool,
}

impl AddonSlot {
    /// Loads the record and checks the files. Blocks on the disk.
    pub fn load(dir: &Path) -> Self {
        let record = Record::load(dir);
        let tools = addons::tools(dir, &record, false);
        Self {
            record,
            tools,
            ..Self::default()
        }
    }
}

impl Manager {
    /// Every optional add-on and its state. There is one: the media add-on.
    pub fn list_addons(&self) -> Vec<AddonInfo> {
        vec![self.addon_info(&self.lock().addon)]
    }

    /// Installs an add-on in the background; progress and the result arrive as
    /// `AddonsChanged`. Installing one that is installed and up to date, or
    /// already being installed, does nothing. The files are looked at again
    /// first, so a damaged install (a program an antivirus took away, say) is
    /// repaired.
    pub async fn install_addon(&self, id: &str) -> Result<()> {
        check_id(id)?;
        if self.lock_open()?.addon.op.is_some() {
            return Ok(());
        }
        // A repair looks closely: every file's checksum.
        self.rescan_addon(true).await;
        let mut state = self.lock_open()?;
        let plan = self.addon_plan(&state.addon);
        if state.addon.op.is_some() || plan.is_empty() {
            return Ok(());
        }
        self.start_addon_op(&mut state.addon, plan, false);
        Ok(())
    }

    /// Brings an installed add-on up to date in the background, the same way as
    /// [`install_addon`](Self::install_addon) (which it is, for one that is not
    /// installed).
    pub async fn update_addon(&self, id: &str) -> Result<()> {
        self.install_addon(id).await
    }

    /// Stops an install or update in progress, the person's or a quiet one.
    /// Nothing in progress is fine. An update leaves the version that was
    /// installed before in place and working, and discards what it had
    /// downloaded; a first install (or a repair of a damaged add-on) removes
    /// everything it put down, so the add-on is not installed afterwards.
    pub async fn cancel_addon_operation(&self, id: &str) -> Result<()> {
        check_id(id)?;
        let (ended, had_tools) = {
            let state = self.lock_open()?;
            let Some(op) = &state.addon.op else {
                return Ok(());
            };
            op.cancel.cancel();
            (op.ended.clone(), state.addon.tools.is_some())
        };
        ended.cancelled().await;
        let dir = self.inner.addon_dir.clone();
        // A working add-on keeps its files; only the staging folder goes.
        let target = if had_tools { dir.join(".work") } else { dir };
        let discarded = self
            .blocking(move || match std::fs::remove_dir_all(&target) {
                Ok(()) => Ok(()),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
                Err(e) => Err(CoreError::Invalid(format!(
                    "The install was stopped, but some of its files could not be deleted ({e}). Remove the media add-on in Settings, Add-ons to clean up."
                ))),
            })
            .await;
        {
            let mut state = self.lock();
            state.addon.failure = None;
        }
        // Show what is really on disk now.
        self.rescan_addon(false).await;
        self.emit_addons(&self.lock().addon);
        discarded
    }

    /// Removes an add-on and its files, stopping an install in progress first.
    /// Refused while a video is being read or downloaded with it. Whatever
    /// happens, the add-on's state afterwards is what is on disk.
    pub async fn remove_addon(&self, id: &str) -> Result<()> {
        check_id(id)?;
        let ended = {
            let state = self.lock_open()?;
            if addon_in_use(&state) {
                return Err(CoreError::WrongState(
                    "The media add-on is in use by a video that is being read or downloaded. Pause those downloads or wait for them to finish, then remove it.",
                ));
            }
            state.addon.op.as_ref().map(|op| {
                op.cancel.cancel();
                op.ended.clone()
            })
        };
        if let Some(ended) = ended {
            ended.cancelled().await;
        }
        let dir = self.inner.addon_dir.clone();
        let removed = self
            .blocking(move || match std::fs::remove_dir_all(&dir) {
                Ok(()) => Ok(()),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
                Err(e) => Err(CoreError::Invalid(format!(
                    "The media add-on could not be removed completely ({e}). Close any program that may be using its files, then remove it again."
                ))),
            })
            .await;
        {
            let mut state = self.lock();
            state.addon.failure = None;
            state.addon.ytdlp_releases.clear();
        }
        // Part of it may be gone: show what is really left.
        self.rescan_addon(false).await;
        self.emit_addons(&self.lock().addon);
        removed
    }

    /// Looks at the add-on's files again, off the async threads, and updates
    /// what the manager believes is installed; `deep` checks every file's
    /// checksum too. Does nothing while an install owns the folder.
    pub(super) async fn rescan_addon(&self, deep: bool) {
        let dir = self.inner.addon_dir.clone();
        let Ok((record, tools)) = self
            .blocking(move || {
                let record = Record::load(&dir);
                let tools = addons::tools(&dir, &record, deep);
                Ok((record, tools))
            })
            .await
        else {
            return;
        };
        let mut state = self.lock();
        if state.addon.op.is_some() {
            return;
        }
        let changed = state.addon.record != record || state.addon.tools != tools;
        state.addon.record = record;
        state.addon.tools = tools;
        if changed {
            info!("the media add-on's files changed on disk");
            self.emit_addons(&state.addon);
        }
    }

    /// [`rescan_addon`](Self::rescan_addon) in the background.
    pub(super) fn spawn_addon_rescan(&self, deep: bool) {
        let manager = self.clone();
        self.spawn(async move { manager.rescan_addon(deep).await });
    }

    /// Looks for a newer yt-dlp now (sites change often, and an old yt-dlp stops
    /// working with them). With `auto_update_addons` on, anything out of date is
    /// then updated quietly. Does nothing when the add-on is not installed.
    ///
    /// When the release could not be looked up, or its checksum list is not
    /// signed by yt-dlp's developers, the check still counts (so the daily
    /// check does not retry at once) and this fails with the reason, for a
    /// person who asked to check.
    pub async fn check_addon_updates(&self) -> Result<()> {
        let (installed, record) = {
            let state = self.lock_open()?;
            (state.addon.tools.is_some(), state.addon.record.clone())
        };
        if !installed {
            return Ok(());
        }
        let mut problem = None;
        let found = match self.new_ytdlp_releases().await {
            Ok(found) => found,
            // A release that does not meet the rules is not offered; nothing to report.
            Err(Lookup::Refused(reason)) => {
                warn!(%reason, "a newer yt-dlp was refused");
                Vec::new()
            }
            // Refused too, but a person who asked should know why nothing came.
            Err(Lookup::Unsigned(message)) => {
                warn!(%message, "a newer yt-dlp was refused");
                problem = Some(message);
                Vec::new()
            }
            Err(Lookup::Unreachable(reason)) => {
                warn!(%reason, "could not check for a newer yt-dlp");
                problem = Some(format!(
                    "Wirecatch could not check for a newer media add-on ({reason}). Check your internet connection, then try again."
                ));
                Vec::new()
            }
        };
        let now = self.inner.clock.unix_millis();
        let mut saved = record;
        saved.last_update_check = Some(now);
        let dir = self.inner.addon_dir.clone();
        let to_save = saved.clone();
        if let Err(e) = self.blocking(move || Ok(to_save.save(&dir))).await? {
            warn!(error = %e, "could not record the add-on update check");
        }
        let mut state = self.lock_open()?;
        state.addon.record.last_update_check = saved.last_update_check;
        let installed = state.addon.record.version("yt-dlp").map(str::to_string);
        state.addon.ytdlp_releases = found
            .into_iter()
            .filter(|r| {
                installed
                    .as_deref()
                    .is_none_or(|have| addons::newer(&r.component.version, have))
            })
            .collect();
        // A release younger than two days is not installed yet, by any path.
        let plan = self.addon_plan(&state.addon);
        if state.settings.auto_update_addons && !plan.is_empty() && state.addon.op.is_none() {
            info!(parts = plan.len(), "updating the media add-on quietly");
            self.start_addon_op(&mut state.addon, plan, true);
        } else {
            self.emit_addons(&state.addon);
        }
        match problem {
            None => Ok(()),
            Some(message) => Err(CoreError::Invalid(message)),
        }
    }

    /// The add-on as the UI shows it.
    pub(super) fn addon_info(&self, slot: &AddonSlot) -> AddonInfo {
        let plan = self.addon_plan(slot);
        let installed = slot.tools.is_some();
        let state = if let Some(op) = &slot.op {
            AddonState::Installing {
                done: op.done,
                total: Some(op.total),
            }
        } else if let Some(message) = &slot.failure {
            AddonState::Failed {
                message: message.clone(),
            }
        } else if !installed && !slot.record.components.is_empty() {
            AddonState::Failed {
                message: "The media add-on is damaged: some of its files are missing or changed. Install it again to repair it.".into(),
            }
        } else if !installed {
            AddonState::NotInstalled
        } else if plan.is_empty() {
            AddonState::Installed
        } else {
            AddonState::UpdateAvailable
        };
        let wanted = self.wanted_components(slot);
        AddonInfo {
            id: MEDIA_ADDON_ID.into(),
            name: "Media add-on".into(),
            description: "Downloads video and audio from sites such as YouTube (yt-dlp), and joins separate video and audio into one file (ffmpeg).".into(),
            state,
            installed_version: (!slot.record.components.is_empty()).then(|| {
                describe(
                    slot.record
                        .components
                        .iter()
                        .map(|(name, c)| (name.as_str(), c.version.as_str())),
                )
            }),
            latest_version: Some(describe(
                wanted.iter().map(|c| (c.name.as_str(), c.version.as_str())),
            )),
            size: (!plan.is_empty()).then(|| plan.iter().map(|c| c.size).sum()),
            note: self.waiting_note(slot),
        }
    }

    pub(super) fn emit_addons(&self, slot: &AddonSlot) {
        self.emit(UiEvent::AddonsChanged(vec![self.addon_info(slot)]));
    }

    /// Says so when the newest yt-dlp is not installed yet because it is
    /// younger than two days.
    fn waiting_note(&self, slot: &AddonSlot) -> Option<String> {
        let now = self.inner.clock.unix_millis();
        let newest = slot.ytdlp_releases.first()?;
        if newest.mature(now) {
            return None;
        }
        let when = if now.saturating_sub(newest.published) < DAY_MILLIS {
            "today"
        } else {
            "yesterday"
        };
        Some(format!(
            "A newer version came out {when}; Wirecatch installs it once it is two days old."
        ))
    }

    /// The parts as they should be: the pinned ones, with the newest yt-dlp
    /// the update check found that is at least two days old. Every install
    /// path (quiet update, Update button, repair, reinstall) uses this.
    fn wanted_components(&self, slot: &AddonSlot) -> Vec<AddonComponent> {
        let sources = &self.inner.addon_sources;
        let now = self.inner.clock.unix_millis();
        let ytdlp = slot
            .ytdlp_releases
            .iter()
            .find(|r| r.mature(now) && addons::newer(&r.component.version, &sources.ytdlp.version))
            .map_or_else(|| sources.ytdlp.clone(), |r| r.component.clone());
        vec![ytdlp, sources.ffmpeg.clone(), sources.deno.clone()]
    }

    /// What must be downloaded: every part for a missing or damaged add-on,
    /// otherwise the parts that are out of date.
    fn addon_plan(&self, slot: &AddonSlot) -> Vec<AddonComponent> {
        let wanted = self.wanted_components(slot);
        if slot.tools.is_none() {
            return wanted;
        }
        addons::outdated(&slot.record, &wanted)
    }

    fn start_addon_op(&self, slot: &mut AddonSlot, plan: Vec<AddonComponent>, quiet: bool) {
        let cancel = self.inner.stopped.child_token();
        let ended = CancellationToken::new();
        slot.op = Some(AddonOp {
            cancel: cancel.clone(),
            ended: ended.clone(),
            done: 0,
            total: plan.iter().map(|c| c.size).sum(),
            quiet,
        });
        if !quiet {
            slot.failure = None;
        }
        self.emit_addons(slot);
        let manager = self.clone();
        self.spawn(async move {
            let result = manager.install_parts(plan, cancel.clone()).await;
            manager.end_addon_op(result, cancel.is_cancelled());
            ended.cancel();
        });
    }

    /// Records how an install ended and tells everyone.
    fn end_addon_op(
        &self,
        result: std::result::Result<(Record, Option<MediaTools>), String>,
        cancelled: bool,
    ) {
        let mut state = self.lock();
        let quiet = state.addon.op.take().is_some_and(|op| op.quiet);
        // Some parts may have been swapped in before a failure.
        let rescan = result.is_err() && !cancelled;
        match result {
            Ok((record, tools)) => {
                state.addon.record = record;
                state.addon.tools = tools;
                state.addon.failure = None;
                // Releases no newer than what is now installed are done with.
                let installed = state.addon.record.version("yt-dlp").map(str::to_string);
                if let Some(have) = installed {
                    state
                        .addon
                        .ytdlp_releases
                        .retain(|r| addons::newer(&r.component.version, &have));
                }
                info!("the media add-on is installed");
            }
            Err(_) if cancelled => {}
            Err(reason) if quiet => warn!(%reason, "a quiet update of the media add-on failed"),
            Err(reason) => {
                warn!(%reason, "installing the media add-on failed");
                state.addon.failure = Some(format!("Installing the media add-on failed. {reason}"));
            }
        }
        self.emit_addons(&state.addon);
        let ready = state.addon.tools.is_some();
        let failed = !ready && !cancelled && !quiet;
        drop(state);
        if rescan {
            self.spawn_addon_rescan(false);
        }
        if ready {
            self.media_addon_ready();
        } else if failed {
            // The pages waiting for it would otherwise wait for good.
            self.media_addon_failed();
        }
    }

    /// Downloads, checks and unpacks every part of `plan` in parallel, then swaps
    /// them in and writes the record. `Err` is the reason, in sentences.
    async fn install_parts(
        &self,
        plan: Vec<AddonComponent>,
        cancel: CancellationToken,
    ) -> std::result::Result<(Record, Option<MediaTools>), String> {
        let dir = self.inner.addon_dir.clone();
        let work = dir.join(".work");
        let made = work.clone();
        self.blocking(move || {
            std::fs::create_dir_all(&made).map_err(|e| CoreError::File {
                path: made.display().to_string(),
                source: e,
            })
        })
        .await
        .map_err(|e| e.to_string())?;

        let progress: Vec<Arc<Progress>> = plan.iter().map(|_| Arc::default()).collect();
        let reporter = self.spawn(self.clone().report_addon_progress(
            progress.clone(),
            plan.iter().map(|c| c.size).collect(),
            cancel.clone(),
        ));
        // One part failing stops the others, without counting as a stop asked for.
        let parts_cancel = cancel.child_token();
        let mut tasks = Vec::new();
        for (component, progress) in plan.iter().cloned().zip(progress) {
            let manager = self.clone();
            let (work, cancel) = (work.clone(), parts_cancel.clone());
            tasks.push(self.spawn(async move {
                let result = manager
                    .fetch_part(&component, &work, progress, cancel.clone())
                    .await;
                if result.is_err() {
                    // One part failing ends the install; stop the others.
                    cancel.cancel();
                }
                result.map(|staged| (component, staged))
            }));
        }
        let mut staged = Vec::new();
        let mut failure = None;
        for task in tasks {
            match task.await {
                Ok(Ok(part)) => staged.push(part),
                // A part stopped because another failed says less than that failure.
                Ok(Err(reason)) if reason == STOPPED => {}
                Ok(Err(reason)) => {
                    failure.get_or_insert(reason);
                }
                Err(e) => {
                    failure.get_or_insert(format!("Something went wrong inside Wirecatch ({e})."));
                }
            }
        }
        reporter.abort();
        if let Some(reason) = failure {
            return Err(reason);
        }
        if cancel.is_cancelled() || staged.len() != plan.len() {
            return Err(STOPPED.into());
        }
        let base = self.lock().addon.record.clone();
        self.blocking(move || Ok(swap_in(&dir, &work, base, staged)))
            .await
            .map_err(|e| e.to_string())?
    }

    /// Downloads one part (continuing an earlier attempt), checks it and unpacks
    /// it into its staging folder.
    async fn fetch_part(
        &self,
        component: &AddonComponent,
        work: &Path,
        progress: Arc<Progress>,
        cancel: CancellationToken,
    ) -> std::result::Result<(PathBuf, archive::Installed), String> {
        let release = work.join(format!(
            "{}-{}.download",
            component.name,
            file_safe(&component.version)
        ));
        if component.size == 0 {
            return Err(format!(
                "The size of {} is not known, so it was not downloaded.",
                component.name
            ));
        }
        let cap = component.size.saturating_add(SIZE_MARGIN);
        let expected = component.sha256.clone();
        let known = release.clone();
        let already = self
            .blocking(move || {
                Ok(known.is_file()
                    && std::fs::metadata(&known).is_ok_and(|m| m.len() <= cap)
                    && archive::sha256_file(&known).is_ok_and(|h| h == expected))
            })
            .await
            .map_err(|e| e.to_string())?;
        if !already {
            let url = Url::parse(&component.url)
                .map_err(|e| format!("The address of {} is not valid ({e}).", component.name))?;
            let mut request = DownloadRequest::new(url, work);
            // The server must announce the size, and no more than expected, before
            // anything is written; a larger file is never downloaded.
            let target = release.clone();
            request.resolve_path = Some(Arc::new(move |remote: &RemoteFile| {
                fits(remote.total, cap).map(|()| target.clone())
            }));
            request.options = self.addon_download_options();
            match wirecatch_engine::download(request, progress, cancel).await {
                Ok(Outcome::Completed { .. }) => {}
                Ok(_) | Err(wirecatch_engine::Error::Cancelled) => {
                    return Err(STOPPED.into());
                }
                Err(e) => {
                    return Err(format!(
                        "{} could not be downloaded: {}",
                        component.name,
                        user_error::describe(&e, work, false).message
                    ));
                }
            }
        }
        let component = component.clone();
        let stage = work.join(format!("stage-{}", component.name));
        self.blocking(move || Ok(verify_and_unpack(&component, &release, &stage, cap)))
            .await
            .map_err(|e| e.to_string())?
    }

    fn addon_download_options(&self) -> DownloadOptions {
        let state = self.lock();
        let mut options = DownloadOptions::default();
        options.connections = usize::from(state.settings.connections_per_download);
        options.rate_limiters = vec![self.inner.global_limiter.clone()];
        options
    }

    /// Reports the bytes downloaded so far until stopped.
    async fn report_addon_progress(
        self,
        parts: Vec<Arc<Progress>>,
        sizes: Vec<u64>,
        cancel: CancellationToken,
    ) {
        loop {
            {
                let mut state = self.lock();
                let slot = &mut state.addon;
                if let Some(op) = &mut slot.op {
                    let done: u64 = parts
                        .iter()
                        .zip(&sizes)
                        .map(|(p, size)| p.done().min(*size))
                        .sum();
                    if done != op.done {
                        op.done = done;
                        let info = self.addon_info(slot);
                        self.emit(UiEvent::AddonsChanged(vec![info]));
                    }
                }
            }
            tokio::select! {
                _ = tokio::time::sleep(PROGRESS_EVERY) => {}
                _ = cancel.cancelled() => return,
            }
        }
    }

    /// The yt-dlp releases worth installing, newest first, each newer than the
    /// pinned one and checked: the newest release and, when that one is younger
    /// than two days, the newest older than that, so the add-on can move to it
    /// meanwhile (D21.1). Read from yt-dlp's list of releases; drafts,
    /// pre-releases, tags that are not plain versions and releases that do not
    /// say when they came out are skipped.
    async fn new_ytdlp_releases(&self) -> std::result::Result<Vec<NewRelease>, Lookup> {
        let sources = self.inner.addon_sources.clone();
        let Some(api) = &sources.ytdlp_releases else {
            return Ok(Vec::new());
        };
        let api = Url::parse(api).map_err(|e| Lookup::Refused(e.to_string()))?;
        let cancel = self.inner.stopped.child_token();
        let mut options = DownloadOptions::default();
        options.headers.insert(
            ACCEPT,
            HeaderValue::from_static("application/vnd.github+json"),
        );
        let fetcher = Fetcher::new(&api, &options, cancel)
            .await
            .map_err(|e| Lookup::Unreachable(e.to_string()))?;
        let body = fetcher
            .get(&api, None, MAX_METADATA)
            .await
            .map_err(|e| Lookup::Unreachable(e.to_string()))?
            .body;
        let releases: Vec<addons::Release> = serde_json::from_slice(&body)
            .map_err(|e| Lookup::Refused(format!("unreadable list of releases ({e})")))?;
        let now = self.inner.clock.unix_millis();
        let mut candidates: Vec<(addons::Release, i64)> = releases
            .into_iter()
            .take(RELEASES_LOOKED_AT)
            .filter(|r| !r.draft && !r.prerelease)
            // `newer` only reads plain dotted numbers, so the tag is safe in a path.
            .filter(|r| addons::newer(r.tag_name.trim(), &sources.ytdlp.version))
            .filter_map(|r| {
                let published = r
                    .published_at
                    .as_deref()
                    .and_then(|at| chrono::DateTime::parse_from_rfc3339(at).ok())?
                    .timestamp_millis();
                Some((r, published))
            })
            .collect();
        // Newest version first, whatever order the list came in.
        candidates.sort_by(|(a, _), (b, _)| {
            if addons::newer(a.tag_name.trim(), b.tag_name.trim()) {
                std::cmp::Ordering::Less
            } else if addons::newer(b.tag_name.trim(), a.tag_name.trim()) {
                std::cmp::Ordering::Greater
            } else {
                std::cmp::Ordering::Equal
            }
        });
        let mut found = Vec::new();
        for (release, published) in candidates {
            let mature = now.saturating_sub(published) >= MATURE_AGE_MILLIS;
            // Past the newest, only a release old enough to install is of use.
            if !found.is_empty() && !mature {
                continue;
            }
            let component = self.checked_release(&fetcher, &sources, &release).await?;
            found.push(NewRelease {
                component,
                published,
            });
            if mature {
                break;
            }
        }
        Ok(found)
    }

    /// `release` as a component checked against its own checksum list.
    ///
    /// Only yt-dlp's own release folder is trusted for the files, the file's
    /// size must be known and reasonable, and its checksum comes from the
    /// release's checksum list, which must carry a valid OpenPGP signature by
    /// yt-dlp's pinned release key (D21.1).
    async fn checked_release(
        &self,
        fetcher: &Fetcher,
        sources: &manifest::MediaAddonSources,
        release: &addons::Release,
    ) -> std::result::Result<AddonComponent, Lookup> {
        let version = release.tag_name.trim().to_string();
        let folder = format!("{}{version}/", sources.ytdlp_release_downloads);
        let asset = |name: &str| release.assets.iter().find(|a| a.name == name);
        let (Some(program), Some(sums)) = (
            asset(&sources.ytdlp_asset),
            asset(&sources.ytdlp_checksums_asset),
        ) else {
            return Err(Lookup::Refused(format!(
                "release {version} lacks {} or its checksums",
                sources.ytdlp_asset
            )));
        };
        for asset in [program, sums] {
            if asset.browser_download_url != format!("{folder}{}", asset.name) {
                return Err(Lookup::Refused(format!(
                    "release {version} points {} somewhere other than yt-dlp's releases",
                    asset.name
                )));
            }
        }
        if program.size == 0 || program.size > MAX_UPDATE_SIZE {
            return Err(Lookup::Refused(format!(
                "release {version} gives {} an unusable size ({} bytes)",
                program.name, program.size
            )));
        }
        let sums_url =
            Url::parse(&sums.browser_download_url).map_err(|e| Lookup::Refused(e.to_string()))?;
        let list = fetcher
            .get(&sums_url, None, MAX_METADATA)
            .await
            .map_err(|e| Lookup::Unreachable(e.to_string()))?
            .body;
        // The checksum list counts only with yt-dlp's signature on it.
        let unsigned = |why: &str| {
            Lookup::Unsigned(format!(
                "yt-dlp {version} was not installed because its checksum list does not carry a valid signature from yt-dlp's developers ({why}). The installed version keeps working; if this keeps happening, please report it."
            ))
        };
        let sig_name = format!("{}.sig", sources.ytdlp_checksums_asset);
        let sig = asset(&sig_name).ok_or_else(|| unsigned("the release has no signature file"))?;
        if sig.browser_download_url != format!("{folder}{}", sig.name) {
            return Err(unsigned(
                "the signature file is not in yt-dlp's release folder",
            ));
        }
        let sig_url =
            Url::parse(&sig.browser_download_url).map_err(|e| Lookup::Refused(e.to_string()))?;
        let signature = fetcher
            .get(&sig_url, None, MAX_SIGNATURE)
            .await
            .map_err(|e| Lookup::Unreachable(e.to_string()))?
            .body;
        let key = sources.ytdlp_signing_key.clone();
        let signed = list.clone();
        self.blocking(move || Ok(signature::verify_detached(&key, &signed, &signature)))
            .await
            .map_err(|e| Lookup::Refused(e.to_string()))?
            .map_err(|why| unsigned(&why))?;
        let list = String::from_utf8_lossy(&list);
        let sha256 = addons::checksum_for(&list, &sources.ytdlp_asset).ok_or_else(|| {
            Lookup::Refused(format!(
                "release {version} has no checksum for {}",
                sources.ytdlp_asset
            ))
        })?;
        let mut component = manifest::ytdlp_component(
            &version,
            program.browser_download_url.clone(),
            &sha256,
            program.size,
        );
        component.files.clone_from(&sources.ytdlp.files);
        component.program.clone_from(&sources.ytdlp.program);
        Ok(component)
    }

    /// Runs the update check whenever it is due, while the app runs.
    pub(super) async fn run_addon_updates(
        weak: std::sync::Weak<super::Inner>,
        stopped: CancellationToken,
    ) {
        loop {
            let Some(manager) = Manager::from_weak(&weak) else {
                return;
            };
            let due = {
                let state = manager.lock();
                let last = state.addon.record.last_update_check.unwrap_or(i64::MIN);
                state.settings.auto_update_addons
                    && state.addon.tools.is_some()
                    && manager.inner.addon_sources.ytdlp_releases.is_some()
                    && manager.inner.clock.unix_millis().saturating_sub(last) >= CHECK_EVERY_MILLIS
            };
            if due && let Err(e) = manager.check_addon_updates().await {
                warn!(error = %e, "the add-on update check failed");
            }
            manager.expire_addon_waits();
            drop(manager);
            tokio::select! {
                _ = tokio::time::sleep(LOOK_EVERY) => {}
                _ = stopped.cancelled() => return,
            }
        }
    }
}

/// Whether a video is being read with yt-dlp or downloaded as media (which may
/// run yt-dlp or ffmpeg) right now.
fn addon_in_use(state: &super::state::State) -> bool {
    let reading = state.media.values().any(|m| {
        m.source.kind == wirecatch_protocol::extension::MediaKind::Page
            && matches!(m.stage, super::media::MediaStage::Analysing(_))
    });
    let downloading = state.running.iter().any(|id| {
        state
            .downloads
            .get(id)
            .is_some_and(|e| e.media_job.is_some())
    });
    reading || downloading
}

fn check_id(id: &str) -> Result<()> {
    if id == MEDIA_ADDON_ID {
        Ok(())
    } else {
        Err(CoreError::Invalid(format!(
            "There is no add-on called \"{id}\". Update Wirecatch if this add-on is new."
        )))
    }
}

/// "yt-dlp 2026.08.19, ffmpeg 9.0.2, Deno 2.9.7".
fn describe<'a>(parts: impl Iterator<Item = (&'a str, &'a str)>) -> String {
    let mut parts: Vec<(&str, &str)> = parts.collect();
    let order = |name: &str| ["yt-dlp", "ffmpeg", "deno"].iter().position(|n| *n == name);
    parts.sort_by_key(|(name, _)| order(name));
    parts
        .into_iter()
        .map(|(name, version)| match name {
            "ffmpeg" => format!(
                "ffmpeg {}",
                version
                    .trim_start_matches('n')
                    .split('-')
                    .next()
                    .unwrap_or(version)
            ),
            "deno" => format!("Deno {version}"),
            other => format!("{other} {version}"),
        })
        .collect::<Vec<_>>()
        .join(", ")
}

/// Whether a release file the server announces as `total` bytes may be
/// downloaded under the cap: its size must be announced, and no larger.
fn fits(total: Option<u64>, cap: u64) -> std::result::Result<(), String> {
    match total {
        Some(total) if total <= cap => Ok(()),
        Some(_) => Err("the file is larger than its release says".into()),
        None => Err("the server did not say how large the file is".into()),
    }
}

/// A version made safe for a file name.
fn file_safe(version: &str) -> String {
    version
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_') {
                c
            } else {
                '_'
            }
        })
        .collect()
}

/// Checks a downloaded release against its checksum and, only if it matches,
/// unpacks it. A file that does not match is deleted, so the next try
/// downloads it again. Blocks on the disk.
fn verify_and_unpack(
    component: &AddonComponent,
    release: &Path,
    stage: &Path,
    cap: u64,
) -> std::result::Result<(PathBuf, archive::Installed), String> {
    let unreadable =
        |e: std::io::Error| format!("The downloaded {} could not be read ({e}).", component.name);
    // One open file is checked and unpacked, so nothing can swap it in between.
    let mut file = archive::open_release(release).map_err(unreadable)?;
    // Never hash (or unpack) more than was announced.
    let len = file.metadata().map_err(unreadable)?.len();
    if len > cap {
        drop(file);
        let _ = std::fs::remove_file(release);
        return Err(format!(
            "The downloaded {} is larger than its release says, so it was not installed. Try again later; if it keeps happening, please report it.",
            component.name
        ));
    }
    let actual = archive::sha256_of(&mut file).map_err(unreadable)?;
    if actual != component.sha256 {
        warn!(component = %component.name, expected = %component.sha256, %actual, "checksum mismatch");
        drop(file);
        let _ = std::fs::remove_file(release);
        return Err(format!(
            "The downloaded {} did not match its published checksum, so it was not installed. Try again later; if it keeps happening, please report it.",
            component.name
        ));
    }
    match std::fs::remove_dir_all(stage) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => {
            return Err(format!(
                "The add-on's work folder could not be cleaned ({e})."
            ));
        }
    }
    std::fs::create_dir_all(stage)
        .map_err(|e| format!("The add-on's work folder could not be created ({e})."))?;
    let installed = archive::unpack(component, &mut file, stage).inspect_err(|_| {
        let _ = std::fs::remove_dir_all(stage);
    })?;
    Ok((stage.to_path_buf(), installed))
}

/// Puts staged parts in place of the old ones and writes the record, then
/// deletes the release files. Blocks on the disk.
fn swap_in(
    dir: &Path,
    work: &Path,
    mut record: Record,
    staged: Vec<(AddonComponent, (PathBuf, archive::Installed))>,
) -> std::result::Result<(Record, Option<MediaTools>), String> {
    for (component, (stage, files)) in staged {
        let target = dir.join(&component.name);
        let old = work.join(format!("old-{}", component.name));
        let _ = std::fs::remove_dir_all(&old);
        if target.exists() {
            std::fs::rename(&target, &old).map_err(|e| {
                format!(
                    "The old {} could not be replaced ({e}). Wait until no video is being processed, then try again.",
                    component.name
                )
            })?;
        }
        if let Err(e) = std::fs::rename(&stage, &target) {
            // Put the old part back, so what was installed keeps working.
            let _ = std::fs::rename(&old, &target);
            return Err(format!(
                "{} could not be put in place ({e}).",
                component.name
            ));
        }
        let _ = std::fs::remove_dir_all(&old);
        record.components.insert(
            component.name.clone(),
            addons::InstalledComponent {
                version: component.version.clone(),
                program: component.program.clone(),
                files,
            },
        );
        // Recorded at once: if a later part cannot be swapped in, the record
        // still says exactly what is on disk, so the next install repairs only
        // what is missing and nothing is run that does not match it.
        record
            .save(dir)
            .map_err(|e| format!("The add-on could not record what it installed ({e})."))?;
        let _ = std::fs::remove_file(work.join(format!(
            "{}-{}.download",
            component.name,
            file_safe(&component.version)
        )));
    }
    let tools = addons::tools(dir, &record, false);
    Ok((record, tools))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_release_file_must_announce_a_size_within_the_cap() {
        assert!(fits(Some(100), 100).is_ok());
        assert!(fits(Some(101), 100).is_err());
        assert!(fits(None, 100).is_err());
    }

    #[test]
    fn versions_read_plainly() {
        let parts = [
            ("deno", "2.9.7"),
            ("ffmpeg", "n9.0.2-25-g67b60c310b"),
            ("yt-dlp", "2026.08.19"),
        ];
        assert_eq!(
            describe(parts.into_iter()),
            "yt-dlp 2026.08.19, ffmpeg 9.0.2, Deno 2.9.7"
        );
        assert_eq!(file_safe("v1/../2"), "v1_.._2");
    }
}
