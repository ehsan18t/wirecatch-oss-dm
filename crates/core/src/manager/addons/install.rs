//! Installing the add-on's parts: each release file downloaded, checked, unpacked and swapped in.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use tracing::warn;
use wirecatch_addons::manifest::{AddonComponent, ComponentId};
use wirecatch_addons::{self as addons, Check, Record, WORK_FOLDER, archive};
use wirecatch_engine::{
    CancellationToken, DownloadOptions, DownloadRequest, Outcome, Progress, RemoteFile,
};
use wirecatch_files::FileError;
use wirecatch_media::MediaTools;
use wirecatch_media::ytdlp;
use wirecatch_protocol::app::UiEvent;
use wirecatch_streams::process::{self, RunError, RunOptions};

use super::{ADDON_NETWORK, InstallFailure, Origin, Part, PartError, Refusal, STOPPED};
use crate::error::CoreError;
use crate::manager::Manager;

/// How often the progress of an install is reported.
pub(super) const PROGRESS_EVERY: Duration = Duration::from_millis(250);

/// How much larger than announced a release file may be. Release sizes are
/// exact byte counts; the margin is only a small allowance.
pub(super) const SIZE_MARGIN: u64 = 4096;

/// How long a new yt-dlp may take to say its version. It answers in about a second, more on its first run while an antivirus looks at it.
pub(super) const VERSION_TIMEOUT: Duration = Duration::from_mins(1);

/// The most of a version a program prints that a message repeats.
pub(super) const MAX_SHOWN_VERSION: usize = 40;

/// A part's release file while it downloads, in the work folder.
pub(super) fn release_file(id: ComponentId, version: &str) -> String {
    format!("{}-{}.download", id.as_str(), file_safe(version))
}

/// Where a part is unpacked before it is swapped in, in the work folder.
pub(super) fn stage_folder(id: ComponentId) -> String {
    format!("stage-{}", id.as_str())
}

/// Where the part it replaces waits until the swap has worked, in the work folder.
pub(super) fn old_folder(id: ComponentId) -> String {
    format!("old-{}", id.as_str())
}

impl Manager {
    /// Downloads, checks and unpacks every part of `plan` in parallel, then swaps
    /// them in and writes the record. `Err` is the reason, in sentences.
    pub(super) async fn install_parts(
        &self,
        plan: Vec<Part>,
        cancel: CancellationToken,
    ) -> std::result::Result<(Record, Option<MediaTools>), InstallFailure> {
        let dir = self.inner.addon_dir.clone();
        let work = dir.join(WORK_FOLDER);
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
    pub(super) async fn fetch_part(
        &self,
        part: &Part,
        work: &Path,
        progress: Arc<Progress>,
        cancel: CancellationToken,
    ) -> std::result::Result<(PathBuf, archive::Installed), PartError> {
        let component = &part.component;
        let id = component.id();
        let release = work.join(release_file(id, component.version()));
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
                        wirecatch_messages::describe(&e, work, wirecatch_messages::Worked::Never)
                            .message
                    )
                    .into());
                }
            }
        }
        let owned = component.clone();
        let stage = work.join(stage_folder(id));
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
    pub(super) fn addon_download_options(&self) -> DownloadOptions {
        let state = self.lock();
        let policy = self.app_policy(&state);
        let mut options = crate::headers::download_options(ADDON_NETWORK, policy.proxy);
        options.connections = policy.connections;
        options.rate_limiters = policy.limiters;
        options.retry_backoff = policy.retry_backoff;
        options
    }

    /// Reports the bytes downloaded so far until stopped.
    pub(super) async fn report_addon_progress(
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
}

/// Asks a yt-dlp the update check found, unpacked in `stage`, which version it is, and installs it only when it answers, successfully and with nothing else, with the tag it was offered as. yt-dlp's checksum list is signed but names no version, so an older release's files and signed list published again under a newer tag would otherwise be installed as the newer release (D38.4). Only a successful answer that is not the tag is evidence of that, a [`PartError::Refused`]; one that cannot be started, does not answer in time or ends with an error is an ordinary failure, tried again later (an antivirus or a full temporary folder can cause it).
pub(super) async fn confirm_version(
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
pub(super) fn fits(total: Option<u64>, cap: u64) -> std::result::Result<(), String> {
    match total {
        Some(total) if total <= cap => Ok(()),
        Some(_) => Err("the file is larger than its release says".into()),
        None => Err("the server did not say how large the file is".into()),
    }
}

/// A version made safe for a file name.
pub(super) fn file_safe(version: &str) -> String {
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
pub(super) fn verify_and_unpack(
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
pub(super) fn swap_in(
    dir: &Path,
    work: &Path,
    mut record: Record,
    staged: Vec<(AddonComponent, (PathBuf, archive::Installed))>,
    mut rename: impl FnMut(&Path, &Path) -> std::io::Result<()>,
) -> std::result::Result<(Record, Option<MediaTools>), InstallFailure> {
    for (component, (stage, files)) in staged {
        let id = component.id();
        let target = dir.join(id.as_str());
        let old = work.join(old_folder(id));
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
