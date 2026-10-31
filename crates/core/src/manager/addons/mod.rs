//! Installing, updating and removing the media add-on (D20).
//!
//! Every part is downloaded with the engine (in parallel, resumable), checked
//! against its SHA-256 before anything is unpacked, unpacked into a staging
//! folder, and only then swapped into place. The record of what is installed is
//! written last, and the programs are only ever run when every recorded file is
//! there with its recorded size. yt-dlp keeps up with sites through a quiet
//! update check at most once a day; ffmpeg and Deno change only with the app's
//! pinned versions.

mod install;
#[cfg(test)]
mod tests;
mod updates;

use std::path::Path;
use std::time::Duration;

use tracing::{info, warn};
use wirecatch_addons::manifest::{AddonComponent, ComponentId};
use wirecatch_addons::{self as addons, Check, Record, WORK_FOLDER, signature};
use wirecatch_engine::{CancellationToken, PrivateNetwork};
use wirecatch_files::FileError;
use wirecatch_media::MediaTools;
use wirecatch_protocol::UnixMillis;
use wirecatch_protocol::app::{AddonId, AddonInfo, AddonState, UiEvent};

use super::Manager;
use crate::error::{CoreError, Result};

/// The update check runs at most this often.
const CHECK_EVERY: Duration = Duration::from_hours(24);

/// How often the background task looks whether a check is due.
const LOOK_EVERY: Duration = Duration::from_hours(1);

/// Why a part did not finish when the install was stopped.
const STOPPED: &str = "It was stopped.";

/// A new yt-dlp release is installed (quietly, with the Update button, by a
/// repair or a reinstall) only once it is this old, so a broken or tampered
/// release has time to be noticed and withdrawn (D21.1).
const MATURE_AGE: Duration = Duration::from_hours(48);

/// A day, for saying when a release that is not old enough came out.
const DAY: Duration = Duration::from_hours(24);

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
    pub published: UnixMillis,
}

impl NewRelease {
    /// Whether it is old enough to install at `now`.
    fn mature(&self, now: UnixMillis) -> bool {
        now.since(self.published) >= MATURE_AGE
    }
}

/// An install or update in progress.
pub(super) struct AddonOp {
    pub cancel: CancellationToken,
    /// Cancelled when it has ended, however it ended.
    pub ended: CancellationToken,
    pub done: u64,
    pub total: u64,
    pub kind: OpKind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum OpKind {
    /// Started by the person: a failure is shown.
    Visible,
    /// A background update: a failure is logged, not shown.
    Quiet,
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
        self.start_addon_op(&mut state.addon, plan, OpKind::Visible);
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
        let target = if had_tools {
            dir.join(WORK_FOLDER)
        } else {
            dir
        };
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
        let when = if now.since(newest.published) < DAY {
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

    fn start_addon_op(&self, slot: &mut AddonSlot, plan: Vec<Part>, kind: OpKind) {
        let cancel = self.inner.stopped.child_token();
        let ended = CancellationToken::new();
        slot.op = Some(AddonOp {
            cancel: cancel.clone(),
            ended: ended.clone(),
            done: 0,
            total: plan.iter().map(|p| p.component.release().size()).sum(),
            kind,
        });
        if kind == OpKind::Visible {
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
        let quiet = state
            .addon
            .op
            .take()
            .is_some_and(|op| op.kind == OpKind::Quiet);
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
