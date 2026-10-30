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
    CancellationToken, DownloadOptions, DownloadRequest, Fetcher, Outcome, PrivateNetwork,
    Progress, RemoteFile, Url,
    header::{ACCEPT, HeaderValue},
};
use wirecatch_files::FileError;
use wirecatch_protocol::app::{AddonId, AddonInfo, AddonState, UiEvent};

use super::Manager;
use crate::error::{CoreError, Result};
use wirecatch_addons::manifest::{self, AddonComponent, ComponentId, ReleaseFile};
use wirecatch_addons::{self as addons, Check, Record, archive, signature};
use wirecatch_engine::units::{KIB, MIB};
use wirecatch_media::MediaTools;
use wirecatch_media::ytdlp;
use wirecatch_streams::process::{self, RunError, RunOptions};

/// How often the progress of an install is reported.
const PROGRESS_EVERY: Duration = Duration::from_millis(250);
/// The update check runs at most this often.
const CHECK_EVERY_MILLIS: i64 = 24 * 60 * 60 * 1000;
/// How often the background task looks whether a check is due.
const LOOK_EVERY: Duration = Duration::from_hours(1);
/// Why a part did not finish when the install was stopped.
const STOPPED: &str = "It was stopped.";
/// How much larger than announced a release file may be. Release sizes are
/// exact byte counts; the margin is only a small allowance.
const SIZE_MARGIN: u64 = 4096;
/// Largest yt-dlp release file accepted from an update (it is about 40 MB).
const MAX_UPDATE_SIZE: u64 = 200 * MIB;
/// A new yt-dlp release is installed (quietly, with the Update button, by a
/// repair or a reinstall) only once it is this old, so a broken or tampered
/// release has time to be noticed and withdrawn (D21.1).
const MATURE_AGE_MILLIS: i64 = 48 * 60 * 60 * 1000;
/// A day, for saying when a release that is not old enough came out.
const DAY_MILLIS: i64 = 24 * 60 * 60 * 1000;
/// How many of the newest yt-dlp releases are looked at for one old enough.
const RELEASES_LOOKED_AT: usize = 10;
/// Largest GitHub release description or checksum list read.
const MAX_METADATA: u64 = 4 * MIB;
/// Largest signature file read (yt-dlp's is under 1 KB).
const MAX_SIGNATURE: u64 = 64 * KIB;
/// How long a new yt-dlp may take to say its version. It answers in about a second, more on its first run while an antivirus looks at it.
const VERSION_TIMEOUT: Duration = Duration::from_mins(1);
/// The most of a version a program prints that a message repeats.
const MAX_SHOWN_VERSION: usize = 40;
/// Every request for the add-on: a release host is never on the local network, so a name that answers with a local address, or a redirect there, is refused (D38.5).
const ADDON_NETWORK: PrivateNetwork = PrivateNetwork::Block;

/// Why installing or updating the media add-on failed.
#[derive(Debug)]
struct InstallFailure {
    /// What went wrong, in sentences.
    reason: String,
    /// A part that was installed is gone: the new one could not be put in place
    /// and the old one could not be put back, so the add-on no longer works.
    part_lost: bool,
    /// A yt-dlp the update check found was not the release it claimed.
    refused: Option<Refusal>,
}

impl From<String> for InstallFailure {
    fn from(reason: String) -> Self {
        Self {
            reason,
            part_lost: false,
            refused: None,
        }
    }
}

/// A yt-dlp release refused because it is not the release it claims (D38.4). It is a warning sign rather than a passing failure: it is not offered, downloaded or run again while the app runs, and the person is told even when the update was a quiet one.
#[derive(Debug, Clone)]
pub(super) struct Refusal {
    /// The release's tag.
    pub version: String,
    /// What the person is told, in sentences.
    pub message: String,
}

/// Why one part was not made ready to put in place.
#[derive(Debug)]
enum PartError {
    /// The install was stopped, or another part failed first.
    Stopped,
    /// It is not the release it claims.
    Refused(Refusal),
    /// Anything else, in sentences.
    Failed(String),
}

impl From<String> for PartError {
    fn from(reason: String) -> Self {
        Self::Failed(reason)
    }
}

/// Where a part to install comes from, which decides how it is checked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Origin {
    /// The app's own manifest, which names its checksum.
    Pinned,
    /// The update check, whose signed checksum list names no version: it must also say it is its tag.
    Found,
}

/// One part to install, and where it comes from.
#[derive(Debug, Clone)]
struct Part {
    component: AddonComponent,
    origin: Origin,
}

/// Why looking up the newest yt-dlp gave nothing.
enum Lookup {
    /// The release information could not be fetched (no connection, server error).
    Unreachable(String),
    /// It was fetched, but the release does not meet the rules and is not offered.
    Refused(String),
    /// The release's checksum list is not signed by yt-dlp's pinned key, so it is not offered.
    Unsigned {
        /// The release.
        version: String,
        /// What is wrong with its signature.
        why: Unsigned,
    },
}

/// Why a release's checksum list counts as unsigned.
#[derive(Debug, thiserror::Error)]
enum Unsigned {
    #[error("the release has no signature file")]
    NoSignature,
    #[error("the signature file is not in yt-dlp's release folder")]
    Elsewhere,
    #[error(transparent)]
    Invalid(signature::SignatureError),
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
    /// The record could not be read when the files were last looked at, so what is installed is unknown.
    pub unreadable: bool,
    /// yt-dlp releases refused while the app runs for not being what they claim, newest refusal last.
    pub refused: Vec<Refusal>,
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
        match Record::load(dir) {
            Ok(record) => {
                let tools = addons::tools(dir, &record, Check::Sizes).map(MediaTools::from);
                Self {
                    record,
                    tools,
                    ..Self::default()
                }
            }
            Err(e) => {
                warn!(error = %e, "could not read the media add-on record; its state is unknown until the files are looked at again");
                Self {
                    unreadable: true,
                    ..Self::default()
                }
            }
        }
    }
}

impl Manager {
    /// Every optional add-on and its state. There is one: the media add-on.
    #[must_use]
    pub fn list_addons(&self) -> Vec<AddonInfo> {
        vec![self.addon_info(&self.lock().addon)]
    }

    /// Installs an add-on in the background; progress and the result arrive as `AddonsChanged`. Installing one that is installed and up to date, or already being installed, does nothing. The files are looked at again first, so a damaged install (a program an antivirus took away, say) is repaired.
    ///
    /// # Errors
    ///
    /// [`CoreError::UnknownAddon`] when `id` names no add-on Wirecatch knows, and [`CoreError::Closed`] once the manager has shut down. A failed install is not an error here: it arrives as `AddonsChanged`.
    pub async fn install_addon(&self, id: AddonId) -> Result<()> {
        check_id(id)?;
        if self.lock_open()?.addon.op.is_some() {
            return Ok(());
        }
        // A repair looks closely: every file's checksum.
        self.rescan_addon(Check::Hashes).await;
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
    ///
    /// # Errors
    ///
    /// The errors of [`install_addon`](Self::install_addon).
    pub async fn update_addon(&self, id: AddonId) -> Result<()> {
        self.install_addon(id).await
    }

    /// Stops an install or update in progress, the person's or a quiet one. Nothing in progress is fine. An update leaves the version that was installed before in place and working, and discards what it had downloaded; a first install (or a repair of a damaged add-on) removes everything it put down, so the add-on is not installed afterwards.
    ///
    /// # Errors
    ///
    /// [`CoreError::UnknownAddon`] when `id` names no add-on Wirecatch knows, [`CoreError::AddonFilesLeft`] when the install stopped but some of its files could not be deleted, and [`CoreError::Closed`] once the manager has shut down.
    pub async fn cancel_addon_operation(&self, id: AddonId) -> Result<()> {
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
                Err(source) => Err(CoreError::AddonFilesLeft { source }),
            })
            .await;
        {
            let mut state = self.lock();
            state.addon.failure = None;
        }
        // Show what is really on disk now.
        self.rescan_addon(Check::Sizes).await;
        self.emit_addons(&self.lock().addon);
        discarded
    }

    /// Removes an add-on and its files, stopping an install in progress first. Refused while a video is being read or downloaded with it. Whatever happens, the add-on's state afterwards is what is on disk.
    ///
    /// # Errors
    ///
    /// [`CoreError::UnknownAddon`] when `id` names no add-on Wirecatch knows, [`CoreError::AddonNotRemoved`] when its files could not all be deleted, [`CoreError::WrongState`] while a video is read or downloaded with it, and [`CoreError::Closed`] once the manager has shut down.
    pub async fn remove_addon(&self, id: AddonId) -> Result<()> {
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
                Err(source) => Err(CoreError::AddonNotRemoved { source }),
            })
            .await;
        {
            let mut state = self.lock();
            state.addon.failure = None;
            state.addon.ytdlp_releases.clear();
        }
        // Part of it may be gone: show what is really left.
        self.rescan_addon(Check::Sizes).await;
        self.emit_addons(&self.lock().addon);
        removed
    }

    /// Looks at the add-on's files again, off the async threads, and updates what the manager believes is installed; `check` says how closely. Does nothing while an install owns the folder.
    pub(super) async fn rescan_addon(&self, check: Check) {
        let dir = self.inner.addon_dir.clone();
        let scanned = self
            .blocking(move || {
                let record = Record::load(&dir).map_err(|source| {
                    CoreError::File(FileError::new(&dir.join(addons::RECORD_FILE), source))
                })?;
                let tools = addons::tools(&dir, &record, check).map(MediaTools::from);
                Ok((record, tools))
            })
            .await;
        let (record, tools) = match scanned {
            Ok(scanned) => scanned,
            // A record that cannot be read now (held by an antivirus, say) keeps what was known.
            Err(e) => {
                warn!(error = %e, "could not look at the media add-on's files; keeping what was known");
                return;
            }
        };
        let mut state = self.lock();
        if state.addon.op.is_some() {
            return;
        }
        let changed =
            state.addon.record != record || state.addon.tools != tools || state.addon.unreadable;
        state.addon.record = record;
        state.addon.tools = tools;
        state.addon.unreadable = false;
        if changed {
            info!("the media add-on's files changed on disk");
            self.emit_addons(&state.addon);
        }
    }

    /// [`rescan_addon`](Self::rescan_addon) in the background.
    pub(super) fn spawn_addon_rescan(&self, check: Check) {
        let manager = self.clone();
        self.spawn(async move { manager.rescan_addon(check).await });
    }

    /// Looks for a newer yt-dlp now (sites change often, and an old yt-dlp stops
    /// working with them). With `auto_update_addons` on, anything out of date is
    /// then updated quietly. Does nothing when the add-on is not installed.
    ///
    /// When the release could not be looked up, or its checksum list is not
    /// signed by yt-dlp's developers, the check still counts (so the daily
    /// check does not retry at once) and this fails with the reason, for a
    /// person who asked to check.
    ///
    /// # Errors
    ///
    /// [`CoreError::UpdateCheckFailed`], with a message for the person who asked, when the
    /// releases could not be looked up or a newer release's checksum list is not signed
    /// by yt-dlp's developers; and [`CoreError::Closed`] once the manager has shut
    /// down.
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
            Err(Lookup::Unsigned { version, why }) => {
                warn!(%version, reason = %why, "a newer yt-dlp was refused");
                problem = Some(format!(
                    "yt-dlp {version} was not installed because its checksum list does not carry a valid signature from yt-dlp's developers ({why}). The installed version keeps working; if this keeps happening, please report it."
                ));
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
        let installed = state
            .addon
            .record
            .version(ComponentId::YtDlp)
            .map(str::to_string);
        let refused: Vec<String> = state
            .addon
            .refused
            .iter()
            .map(|r| r.version.clone())
            .collect();
        state.addon.ytdlp_releases = found
            .into_iter()
            .filter(|r| {
                installed
                    .as_deref()
                    .is_none_or(|have| addons::newer(r.component.version(), have))
                    // Refused for not being what it claims: never downloaded or run again while the app runs.
                    && !refused.iter().any(|v| v == r.component.version())
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
            Some(message) => Err(CoreError::UpdateCheckFailed(message)),
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
        } else if slot.unreadable {
            AddonState::Failed {
                message: "The media add-on's list of installed files could not be read, so Wirecatch cannot tell whether the add-on works. Restart Wirecatch, or install the add-on again to repair it.".into(),
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
            id: AddonId::Media,
            name: "Media add-on".into(),
            description: "Downloads video and audio from sites such as YouTube (yt-dlp), and merges separate video and audio into one file (ffmpeg).".into(),
            state,
            installed_version: (!slot.record.components.is_empty()).then(|| {
                describe(
                    slot.record
                        .components
                        .iter()
                        .map(|(id, c)| (*id, c.version.as_str())),
                )
            }),
            latest_version: Some(describe(
                wanted
                    .iter()
                    .map(|p| (p.component.id(), p.component.version())),
            )),
            size: (!plan.is_empty())
                .then(|| plan.iter().map(|p| p.component.release().size()).sum()),
            note: self.addon_notice(slot),
        }
    }

    pub(super) fn emit_addons(&self, slot: &AddonSlot) {
        self.emit(UiEvent::AddonsChanged(vec![self.addon_info(slot)]));
    }

    /// What the add-on says besides its state: the latest refusal while it still concerns the installed yt-dlp (no newer one is installed), and a newer release that waits.
    fn addon_notice(&self, slot: &AddonSlot) -> Option<String> {
        let installed = slot.record.version(ComponentId::YtDlp);
        let refusal = slot
            .refused
            .iter()
            .rev()
            .find(|r| installed.is_none_or(|have| !addons::newer(have, &r.version)))
            .map(|r| r.message.clone());
        let notes: Vec<String> = refusal
            .into_iter()
            .chain(self.waiting_notice(slot))
            .collect();
        (!notes.is_empty()).then(|| notes.join(" "))
    }

    /// Says so when the newest yt-dlp is not installed yet because it is
    /// younger than two days.
    fn waiting_notice(&self, slot: &AddonSlot) -> Option<String> {
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
            "A newer version was released {when}; Wirecatch installs it after two days."
        ))
    }

    /// The parts as they should be: the pinned ones, with the newest yt-dlp
    /// the update check found that is at least two days old. Every install
    /// path (quiet update, Update button, repair, reinstall) uses this.
    fn wanted_components(&self, slot: &AddonSlot) -> Vec<Part> {
        let sources = &self.inner.addon_sources;
        let now = self.inner.clock.unix_millis();
        let pinned = |component: &AddonComponent| Part {
            component: component.clone(),
            origin: Origin::Pinned,
        };
        let ytdlp = slot
            .ytdlp_releases
            .iter()
            .find(|r| {
                r.mature(now) && addons::newer(r.component.version(), sources.ytdlp.version())
            })
            .map_or_else(
                || pinned(&sources.ytdlp),
                |r| Part {
                    component: r.component.clone(),
                    origin: Origin::Found,
                },
            );
        vec![ytdlp, pinned(&sources.ffmpeg), pinned(&sources.deno)]
    }

    /// What must be downloaded: every part for a missing or damaged add-on,
    /// otherwise the parts that are out of date.
    fn addon_plan(&self, slot: &AddonSlot) -> Vec<Part> {
        let wanted = self.wanted_components(slot);
        if slot.tools.is_none() {
            return wanted;
        }
        wanted
            .into_iter()
            .filter(|p| addons::outdated(&slot.record, &p.component))
            .collect()
    }

    fn start_addon_op(&self, slot: &mut AddonSlot, plan: Vec<Part>, quiet: bool) {
        let cancel = self.inner.stopped.child_token();
        let ended = CancellationToken::new();
        slot.op = Some(AddonOp {
            cancel: cancel.clone(),
            ended: ended.clone(),
            done: 0,
            total: plan.iter().map(|p| p.component.release().size()).sum(),
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
        result: std::result::Result<(Record, Option<MediaTools>), InstallFailure>,
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
                state.addon.unreadable = false;
                state.addon.failure = None;
                // Releases no newer than what is now installed are done with.
                let installed = state
                    .addon
                    .record
                    .version(ComponentId::YtDlp)
                    .map(str::to_string);
                if let Some(have) = installed {
                    state
                        .addon
                        .ytdlp_releases
                        .retain(|r| addons::newer(r.component.version(), &have));
                }
                info!("the media add-on is installed");
            }
            Err(InstallFailure {
                reason,
                part_lost,
                refused,
            }) => {
                // A release that is not what it claims is never offered again while the app runs, and is shown however the install was started.
                let was_refused = refused.is_some();
                if let Some(refusal) = refused {
                    warn!(version = %refusal.version, "a yt-dlp release was refused for not being what it claims");
                    state
                        .addon
                        .ytdlp_releases
                        .retain(|r| r.component.version() != refusal.version);
                    state.addon.refused.push(refusal);
                }
                // A stopped or quiet install stays quiet unless it broke what was installed; a refusal is shown in the note, not twice.
                if part_lost || !(cancelled || quiet || was_refused) {
                    warn!(%reason, "installing the media add-on failed");
                    state.addon.failure =
                        Some(format!("Installing the media add-on failed. {reason}"));
                } else if quiet && !cancelled && !was_refused {
                    warn!(%reason, "a quiet update of the media add-on failed");
                }
            }
        }
        self.emit_addons(&state.addon);
        let ready = state.addon.tools.is_some();
        let failed = !ready && !cancelled && !quiet;
        drop(state);
        if rescan {
            self.spawn_addon_rescan(Check::Sizes);
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
        plan: Vec<Part>,
        cancel: CancellationToken,
    ) -> std::result::Result<(Record, Option<MediaTools>), InstallFailure> {
        let dir = self.inner.addon_dir.clone();
        let work = dir.join(".work");
        let made = work.clone();
        self.blocking(move || {
            std::fs::create_dir_all(&made).map_err(|e| CoreError::File(FileError::new(&made, e)))
        })
        .await
        .map_err(|e| e.to_string())?;

        let progress: Vec<Arc<Progress>> = plan.iter().map(|_| Arc::default()).collect();
        let reporter = self.spawn(self.clone().report_addon_progress(
            progress.clone(),
            plan.iter().map(|p| p.component.release().size()).collect(),
            cancel.clone(),
        ));
        // One part failing stops the others, without counting as a stop asked for.
        let parts_cancel = cancel.child_token();
        let mut tasks = Vec::new();
        for (part, progress) in plan.iter().cloned().zip(progress) {
            let manager = self.clone();
            let (work, cancel) = (work.clone(), parts_cancel.clone());
            tasks.push(self.spawn(async move {
                let result = manager
                    .fetch_part(&part, &work, progress, cancel.clone())
                    .await;
                if result.is_err() {
                    // One part failing ends the install; stop the others.
                    cancel.cancel();
                }
                result.map(|staged| (part.component, staged))
            }));
        }
        let mut staged = Vec::new();
        let mut failure: Option<InstallFailure> = None;
        for task in tasks {
            match task.await {
                Ok(Ok(part)) => staged.push(part),
                // A part stopped because another failed says less than that failure.
                Ok(Err(PartError::Stopped)) => {}
                Ok(Err(PartError::Refused(refusal))) => {
                    failure.get_or_insert(InstallFailure {
                        reason: refusal.message.clone(),
                        part_lost: false,
                        refused: Some(refusal),
                    });
                }
                Ok(Err(PartError::Failed(reason))) => {
                    failure.get_or_insert(reason.into());
                }
                Err(e) => {
                    failure.get_or_insert(
                        format!("Something went wrong inside Wirecatch ({e}).").into(),
                    );
                }
            }
        }
        reporter.abort();
        if let Some(failure) = failure {
            return Err(failure);
        }
        if cancel.is_cancelled() || staged.len() != plan.len() {
            return Err(STOPPED.to_string().into());
        }
        let base = self.lock().addon.record.clone();
        self.blocking(move || {
            Ok(swap_in(&dir, &work, base, staged, |from, to| {
                std::fs::rename(from, to)
            }))
        })
        .await
        .map_err(|e| e.to_string())?
    }

    /// Downloads one part (continuing an earlier attempt), checks it and unpacks it into its staging folder. A yt-dlp the update check found must also say it is the release it was offered as; one that does not is deleted, download and all.
    async fn fetch_part(
        &self,
        part: &Part,
        work: &Path,
        progress: Arc<Progress>,
        cancel: CancellationToken,
    ) -> std::result::Result<(PathBuf, archive::Installed), PartError> {
        let component = &part.component;
        let id = component.id();
        let release = work.join(format!(
            "{}-{}.download",
            id.as_str(),
            file_safe(component.version())
        ));
        let cap = component.release().size().saturating_add(SIZE_MARGIN);
        let expected = component.release().sha256();
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
            let mut request = DownloadRequest::new(component.release().url().clone(), work);
            // The server must announce the size, and no more than expected, before
            // anything is written; a larger file is never downloaded.
            let target = release.clone();
            request.resolve_path = Some(Arc::new(move |remote: &RemoteFile| {
                fits(remote.total, cap)
                    .map(|()| target.clone())
                    .map_err(wirecatch_engine::PathRejected::new)
            }));
            request.options = self.addon_download_options();
            match wirecatch_engine::download(request, progress, cancel.clone()).await {
                Ok(Outcome::Completed { .. }) => {}
                Ok(_) | Err(wirecatch_engine::Error::Cancelled) => {
                    return Err(PartError::Stopped);
                }
                Err(e) => {
                    return Err(format!(
                        "{id} could not be downloaded: {}",
                        wirecatch_messages::describe(&e, work, false).message
                    )
                    .into());
                }
            }
        }
        let owned = component.clone();
        let stage = work.join(format!("stage-{}", id.as_str()));
        let downloaded = release.clone();
        let staged = self
            .blocking(move || Ok(verify_and_unpack(&owned, &downloaded, &stage, cap)))
            .await
            .map_err(|e| e.to_string())??;
        if part.origin == Origin::Found {
            let confirmed = confirm_version(component, &staged.0, &cancel).await;
            if let Err(error) = &confirmed {
                // A refused release is never wanted again; anything else may be tried again from the same download.
                let refused = matches!(error, PartError::Refused(_));
                let stage = staged.0.clone();
                self.blocking(move || {
                    wirecatch_files::discard::dir(&stage);
                    if refused {
                        wirecatch_files::discard::file(&release);
                    }
                    Ok(())
                })
                .await
                .map_err(|e| e.to_string())?;
            }
            confirmed?;
        }
        Ok(staged)
    }

    /// Options for every request for the add-on: the app's own identity, and no request to the local network, since a release host never is on it (D38.5).
    fn addon_download_options(&self) -> DownloadOptions {
        let state = self.lock();
        let policy = self.app_policy(&state);
        let mut options = crate::headers::download_options(ADDON_NETWORK, policy.proxy);
        options.connections = policy.connections;
        options.rate_limiters = policy.limiters;
        options.retry_backoff = policy.retry_backoff;
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
                () = tokio::time::sleep(PROGRESS_EVERY) => {}
                () = cancel.cancelled() => return,
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
        let cancel = self.inner.stopped.child_token();
        let proxy = self.lock().app_proxy();
        let mut options = crate::headers::download_options(ADDON_NETWORK, proxy);
        options.headers.insert(
            ACCEPT,
            HeaderValue::from_static("application/vnd.github+json"),
        );
        let fetcher = Fetcher::with_private_network(api, &options, cancel, ADDON_NETWORK)
            .await
            .map_err(|e| Lookup::Unreachable(e.to_string()))?;
        let body = fetcher
            .get(api, None, MAX_METADATA)
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
            .filter(|r| addons::newer(r.tag_name.trim(), sources.ytdlp.version()))
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
        // `newer` let only a plain dotted version through, so the tag is one folder name.
        let mut downloads = sources.ytdlp_release_downloads.clone();
        if !downloads.path().ends_with('/') {
            let path = format!("{}/", downloads.path());
            downloads.set_path(&path);
        }
        let folder = downloads
            .join(&format!("{version}/"))
            .map_err(|e| Lookup::Refused(e.to_string()))?;
        // A file of the release is taken only from exactly `<folder><name>`.
        let in_folder = |asset: &addons::Asset| -> Option<Url> {
            folder
                .join(&asset.name)
                .ok()
                .filter(|url| url.as_str() == asset.browser_download_url)
        };
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
        let (Some(program_url), Some(sums_url)) = (in_folder(program), in_folder(sums)) else {
            return Err(Lookup::Refused(format!(
                "release {version} points {} or its checksums somewhere other than yt-dlp's releases",
                program.name
            )));
        };
        if program.size == 0 || program.size > MAX_UPDATE_SIZE {
            return Err(Lookup::Refused(format!(
                "release {version} gives {} an unusable size ({} bytes)",
                program.name, program.size
            )));
        }
        let list = fetcher
            .get(&sums_url, None, MAX_METADATA)
            .await
            .map_err(|e| Lookup::Unreachable(e.to_string()))?
            .body;
        // The checksum list counts only with yt-dlp's signature on it.
        let unsigned = |why: Unsigned| Lookup::Unsigned {
            version: version.clone(),
            why,
        };
        let sig_name = format!("{}.sig", sources.ytdlp_checksums_asset);
        let sig = asset(&sig_name).ok_or_else(|| unsigned(Unsigned::NoSignature))?;
        let sig_url = in_folder(sig).ok_or_else(|| unsigned(Unsigned::Elsewhere))?;
        let signature = fetcher
            .get(&sig_url, None, MAX_SIGNATURE)
            .await
            .map_err(|e| Lookup::Unreachable(e.to_string()))?
            .body;
        let key = sources.ytdlp_signing_key.clone();
        let signed = list.clone();
        // The check itself not running (the app closing) says nothing about the release: it is tried again later.
        self.blocking(move || Ok(signature::verify_detached(&key, &signed, &signature)))
            .await
            .map_err(|e| Lookup::Unreachable(e.to_string()))?
            .map_err(|why| unsigned(Unsigned::Invalid(why)))?;
        let list = String::from_utf8_lossy(&list);
        let sha256 = addons::checksum_for(&list, &sources.ytdlp_asset).ok_or_else(|| {
            Lookup::Refused(format!(
                "release {version} has no checksum for {}",
                sources.ytdlp_asset
            ))
        })?;
        Ok(sources.ytdlp.for_release(
            version,
            ReleaseFile::new(program_url, sha256, program.size)
                .map_err(|e| Lookup::Refused(e.to_string()))?,
        ))
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
                () = tokio::time::sleep(LOOK_EVERY) => {}
                () = stopped.cancelled() => return,
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

fn check_id(id: AddonId) -> Result<()> {
    match id {
        AddonId::Media => Ok(()),
        AddonId::Unknown => Err(CoreError::UnknownAddon),
    }
}

/// "yt-dlp 2026.08.19, ffmpeg 9.0.2, Deno 2.9.7".
fn describe<'a>(parts: impl Iterator<Item = (ComponentId, &'a str)>) -> String {
    let mut parts: Vec<(ComponentId, &str)> = parts.collect();
    parts.sort_by_key(|(id, _)| *id);
    parts
        .into_iter()
        .map(|(id, version)| match id {
            ComponentId::Ffmpeg => format!(
                "{id} {}",
                version
                    .trim_start_matches('n')
                    .split('-')
                    .next()
                    .unwrap_or(version)
            ),
            _ => format!("{id} {version}"),
        })
        .collect::<Vec<_>>()
        .join(", ")
}

/// Asks a yt-dlp the update check found, unpacked in `stage`, which version it is, and installs it only when it answers, successfully and with nothing else, with the tag it was offered as. yt-dlp's checksum list is signed but names no version, so an older release's files and signed list published again under a newer tag would otherwise be installed as the newer release (D38.4). Only a successful answer that is not the tag is evidence of that, a [`PartError::Refused`]; one that cannot be started, does not answer in time or ends with an error is an ordinary failure, tried again later (an antivirus or a full temporary folder can cause it).
async fn confirm_version(
    component: &AddonComponent,
    stage: &Path,
    cancel: &CancellationToken,
) -> std::result::Result<(), PartError> {
    let id = component.id();
    let version = component.version();
    let program = archive::safe_join(stage, component.program()).map_err(|e| e.to_string())?;
    let options = RunOptions::collect().with_timeout(VERSION_TIMEOUT);
    let args = ytdlp::version_args();
    let ran = match process::run(&program, &args, stage, options, cancel, |_, _| {}).await {
        Ok(ran) => ran,
        Err(RunError::Cancelled) => return Err(PartError::Stopped),
        Err(e) => {
            return Err(format!(
                "The downloaded {id} {version} could not be started to confirm its version ({e}), so it was not installed. Try again later."
            )
            .into());
        }
    };
    // A run that did not succeed says nothing about which release this is: a full temporary folder or a security product can end a genuine one.
    if !ran.success() {
        return Err(format!(
            "The downloaded {id} {version} ended with an error when asked which version it is, so it was not installed. Wirecatch tries again later."
        )
        .into());
    }
    let said = String::from_utf8_lossy(&ran.stdout);
    let said = said.trim();
    if !ran.stdout_truncated && said == version {
        return Ok(());
    }
    let shown: String = said
        .chars()
        .filter(|c| !c.is_control())
        .take(MAX_SHOWN_VERSION)
        .collect();
    let answered = if ran.stdout_truncated {
        "with far more than a version".to_string()
    } else if shown.is_empty() {
        "nothing".to_string()
    } else {
        format!("\"{shown}\"")
    };
    warn!(part = id.as_str(), offered = version, said = %shown, truncated = ran.stdout_truncated, "a new release says it is another version");
    Err(PartError::Refused(Refusal {
        version: version.to_string(),
        message: format!(
            "{id} {version} was not installed: asked which version it is, it answered {answered}, so it may be an older release published again under a newer name. The installed version keeps working. If this keeps happening, please report it."
        ),
    }))
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
        |e: std::io::Error| format!("The downloaded {} could not be read ({e}).", component.id());
    // One open file is checked and unpacked, so nothing can swap it in between.
    let mut file = archive::open_release(release).map_err(unreadable)?;
    // Never hash (or unpack) more than was announced.
    let len = file.metadata().map_err(unreadable)?.len();
    if len > cap {
        drop(file);
        wirecatch_files::discard::file(release);
        return Err(format!(
            "The downloaded {} is larger than its release says, so it was not installed. Try again later; if it keeps happening, please report it.",
            component.id()
        ));
    }
    let actual = archive::sha256_of(&mut file).map_err(unreadable)?;
    if actual != component.release().sha256() {
        warn!(part = component.id().as_str(), expected = %component.release().sha256(), %actual, "checksum mismatch");
        drop(file);
        wirecatch_files::discard::file(release);
        return Err(format!(
            "The downloaded {} did not match its published checksum, so it was not installed. Try again later; if it keeps happening, please report it.",
            component.id()
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
    let installed = archive::unpack(component, &mut file, stage).map_err(|e| {
        wirecatch_files::discard::dir(stage);
        e.to_string()
    })?;
    Ok((stage.to_path_buf(), installed))
}

/// Puts staged parts in place of the old ones and writes the record, then
/// deletes the release files. Folders are moved with `rename` (a parameter so
/// tests can make a move fail). Blocks on the disk.
fn swap_in(
    dir: &Path,
    work: &Path,
    mut record: Record,
    staged: Vec<(AddonComponent, (PathBuf, archive::Installed))>,
    mut rename: impl FnMut(&Path, &Path) -> std::io::Result<()>,
) -> std::result::Result<(Record, Option<MediaTools>), InstallFailure> {
    for (component, (stage, files)) in staged {
        let id = component.id();
        let target = dir.join(id.as_str());
        let old = work.join(format!("old-{}", id.as_str()));
        wirecatch_files::discard::dir(&old);
        let had_old = target.exists();
        if had_old {
            rename(&target, &old).map_err(|e| {
                format!(
                    "The old {} could not be replaced ({e}). Wait until no video is being processed, then try again.",
                    component.id()
                )
            })?;
        }
        if let Err(e) = rename(&stage, &target) {
            // Put the old part back, so what was installed keeps working.
            if had_old && let Err(restore) = rename(&old, &target) {
                warn!(part = id.as_str(), from = %old.display(), to = %target.display(), error = %restore, "could not put the old add-on part back");
                // The part is gone from its place: the record must not claim it,
                // so the add-on shows as needing an install and nothing runs it.
                record.components.remove(&id);
                if let Err(error) = record.save(dir) {
                    warn!(%error, "could not record that an add-on part is missing");
                }
                return Err(InstallFailure {
                    reason: format!(
                        "{} could not be put in place ({e}), and the version that was installed could not be put back ({restore}). Install the media add-on again from Settings > Media.",
                        component.id()
                    ),
                    part_lost: true,
                    refused: None,
                });
            }
            return Err(format!("{id} could not be put in place ({e}).").into());
        }
        wirecatch_files::discard::dir(&old);
        record.components.insert(
            id,
            addons::InstalledComponent {
                version: component.version().to_string(),
                program: component.program().to_string(),
                files,
            },
        );
        // Recorded at once: if a later part cannot be swapped in, the record
        // still says exactly what is on disk, so the next install repairs only
        // what is missing and nothing is run that does not match it.
        record
            .save(dir)
            .map_err(|e| format!("The add-on could not record what it installed ({e})."))?;
        wirecatch_files::discard::file(&work.join(format!(
            "{}-{}.download",
            id.as_str(),
            file_safe(component.version())
        )));
        // What another version cached (player code, signatures) is a known cause of failed reads.
        if id == ComponentId::YtDlp {
            wirecatch_files::discard::dir(&addons::ytdlp_cache(dir));
        }
    }
    let tools = addons::tools(dir, &record, Check::Sizes).map(MediaTools::from);
    Ok((record, tools))
}

#[cfg(test)]
mod tests {
    use super::*;
    use wirecatch_addons::InstalledComponent;
    use wirecatch_addons::manifest::{ArchiveKind, Sha256Digest};

    /// Add-on downloads name the app, never the engine library's default.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn add_on_downloads_name_the_app() {
        let dir = tempfile::tempdir().unwrap();
        let folders = crate::KnownFolders::under_home(dir.path().join("home"));
        let config = crate::ManagerConfig::new(dir.path().join("data"), folders);
        let manager = Manager::open(config).await.unwrap();
        let agent = manager.addon_download_options().user_agent;
        assert_eq!(
            agent,
            crate::headers::download_options(
                PrivateNetwork::Allow,
                wirecatch_engine::ProxyChoice::System
            )
            .user_agent
        );
        assert_ne!(agent, DownloadOptions::default().user_agent);
    }

    /// A quiet or stopped install is not shown when it fails, unless it lost a
    /// part: the add-on then no longer works, and the person must hear why.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn an_install_that_loses_a_part_says_so_even_quiet_or_stopped() {
        let dir = tempfile::tempdir().unwrap();
        let folders = crate::KnownFolders::under_home(dir.path().join("home"));
        let config = crate::ManagerConfig::new(dir.path().join("data"), folders);
        let manager = Manager::open(config).await.unwrap();
        let end_with = |part_lost: bool, cancelled: bool| {
            manager.lock().addon.op = Some(AddonOp {
                cancel: CancellationToken::new(),
                ended: CancellationToken::new(),
                done: 0,
                total: 0,
                quiet: true,
            });
            let failure = InstallFailure {
                reason: "ffmpeg could not be put in place.".into(),
                part_lost,
                refused: None,
            };
            manager.end_addon_op(Err(failure), cancelled);
            manager.lock().addon.failure.take()
        };
        assert_eq!(end_with(false, false), None, "a quiet failure was shown");
        assert_eq!(end_with(false, true), None, "a stopped install was shown");
        for cancelled in [false, true] {
            let shown = end_with(true, cancelled).expect("the lost part was not shown");
            assert!(
                shown.contains("ffmpeg could not be put in place."),
                "{shown}"
            );
        }
    }

    /// A part with one program file, `tool`, from release file `sha256`.
    fn part(id: ComponentId, sha256: Sha256Digest) -> AddonComponent {
        AddonComponent::new(
            id,
            "2",
            ReleaseFile::new(Url::parse("https://example.com/tool").unwrap(), sha256, 4).unwrap(),
            ArchiveKind::Plain,
            vec![manifest::AddonFile {
                from: String::new(),
                to: "tool".into(),
                executable: true,
            }],
            "tool",
        )
        .unwrap()
    }

    /// A new part that cannot be put in place, when the old one cannot be put
    /// back either, leaves the add-on without that part: the record must stop
    /// claiming it, and the error must say to install the add-on again.
    #[test]
    fn a_part_that_cannot_be_put_back_is_no_longer_recorded() {
        let dir = tempfile::tempdir().unwrap();
        let work = dir.path().join(".work");
        std::fs::create_dir_all(&work).unwrap();
        let target = dir.path().join("ffmpeg");
        std::fs::create_dir_all(&target).unwrap();
        let mut record = Record::default();
        record.components.insert(
            ComponentId::Ffmpeg,
            InstalledComponent {
                version: "1".into(),
                program: "ffmpeg".into(),
                files: Vec::new(),
            },
        );
        record.save(dir.path()).unwrap();
        let component = part(ComponentId::Ffmpeg, Sha256Digest::from_bytes([0; 32]));
        let staged = vec![(component, (work.join("stage-ffmpeg"), Vec::new()))];
        let mut calls = 0;
        let failure = swap_in(dir.path(), &work, record, staged, |from, to| {
            calls += 1;
            // Moving the old part aside works; putting the new one in place
            // and the old one back both fail.
            if calls == 1 {
                std::fs::rename(from, to)
            } else {
                Err(std::io::ErrorKind::PermissionDenied.into())
            }
        })
        .unwrap_err();
        assert_eq!(calls, 3);
        assert!(failure.part_lost);
        let reason = failure.reason;
        assert!(
            reason.contains("Install the media add-on again"),
            "{reason}"
        );
        assert!(
            !Record::load(dir.path())
                .unwrap()
                .components
                .contains_key(&ComponentId::Ffmpeg),
            "the record still claims the part"
        );
    }

    /// A checksum is a value, not its spelling: upper-case hex names the same file.
    #[test]
    fn a_checksum_given_in_upper_case_still_matches() {
        let dir = tempfile::tempdir().unwrap();
        let release = dir.path().join("release");
        std::fs::write(&release, b"tool").unwrap();
        let upper = archive::sha256_file(&release)
            .unwrap()
            .to_string()
            .to_ascii_uppercase();
        let component = part(ComponentId::Deno, Sha256Digest::parse(&upper).unwrap());
        let (_, installed) =
            verify_and_unpack(&component, &release, &dir.path().join("stage"), 100).unwrap();
        assert_eq!(installed.len(), 1);
    }

    /// A part's folder is named by the part, so nothing a caller names can move or delete a folder outside the add-on's: each part lands in its own folder inside it.
    #[test]
    fn a_part_cannot_be_put_outside_the_add_on_folder() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("media");
        let work = dir.join(".work");
        let mut staged = Vec::new();
        for id in ComponentId::ALL {
            let stage = work.join(format!("stage-{}", id.as_str()));
            std::fs::create_dir_all(&stage).unwrap();
            std::fs::write(stage.join("tool"), b"tool").unwrap();
            let files = vec![addons::InstalledFile {
                path: "tool".into(),
                size: 4,
                sha256: archive::sha256_file(&stage.join("tool")).unwrap(),
            }];
            staged.push((part(id, Sha256Digest::from_bytes([0; 32])), (stage, files)));
        }
        let (_, tools) = swap_in(&dir, &work, Record::default(), staged, |from, to| {
            std::fs::rename(from, to)
        })
        .unwrap();
        let outside: Vec<_> = std::fs::read_dir(root.path())
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(outside, ["media"]);
        for id in ComponentId::ALL {
            assert!(dir.join(id.as_str()).join("tool").is_file(), "{id}");
        }
        assert!(tools.is_some());
    }

    #[test]
    fn a_release_file_must_announce_a_size_within_the_cap() {
        assert!(fits(Some(100), 100).is_ok());
        assert!(fits(Some(101), 100).is_err());
        assert!(fits(None, 100).is_err());
    }

    #[test]
    fn versions_read_plainly() {
        let parts = [
            (ComponentId::Deno, "2.9.7"),
            (ComponentId::Ffmpeg, "n9.0.2-25-g67b60c310b"),
            (ComponentId::YtDlp, "2026.08.19"),
        ];
        assert_eq!(
            describe(parts.into_iter()),
            "yt-dlp 2026.08.19, ffmpeg 9.0.2, Deno 2.9.7"
        );
        assert_eq!(file_safe("v1/../2"), "v1_.._2");
    }
}
