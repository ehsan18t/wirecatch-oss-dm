//! Joining and converting the downloaded parts with ffmpeg into the file to keep.

use std::path::{Path, PathBuf};

use tracing::warn;
use wirecatch_engine::CancellationToken;
use wirecatch_media::MediaProgress;
use wirecatch_media::MediaTools;
use wirecatch_media::convert;
use wirecatch_media::describe;
use wirecatch_media::ffmpeg::{FfmpegError, Finished};
use wirecatch_media::merge;
use wirecatch_media::output::{Container, Inputs, OutputJob, VideoAction};
use wirecatch_protocol::app::{ProcessingStep, UserError};
use wirecatch_streams::Role;

use super::placement::finish;
use super::workdir::Downloaded;
use super::{Job, PageJob, workdir};
use crate::manager::run::MediaDone;

/// Saves what was downloaded into the file the person chose: as it came
/// (joined, if it came as separate video and audio), or converted by ffmpeg,
/// with subtitle files next to it when they do not go inside.
pub(super) async fn save(
    job: &Job,
    page: &PageJob,
    tools: &MediaTools,
    downloaded: &Downloaded,
    work: &Path,
    progress: &MediaProgress,
    cancel: &CancellationToken,
) -> Result<MediaDone, UserError> {
    let files: Vec<PathBuf> = downloaded.media.iter().map(|n| work.join(n)).collect();
    let Some(output) = page.output.as_ref() else {
        return match join_original(
            tools,
            &files,
            page.keep_ext.as_deref(),
            job,
            work,
            progress,
            cancel,
        )
        .await?
        {
            Some(file) => finish(&file, job, work, &[]).await,
            None => Ok(MediaDone::Paused),
        };
    };
    let sidecars = match subtitle_files(tools, output, downloaded, work, cancel).await {
        Ok(Some(sidecars)) => sidecars,
        Ok(None) => return Ok(MediaDone::Paused),
        Err(e) => return Ok(not_converted(&e, output, tools)),
    };
    let file = if output.needs_pass() {
        let inputs = match inputs_for(output, downloaded, &files, work).await? {
            Ok(inputs) => inputs,
            Err(e) => return Ok(not_converted(&e, output, tools)),
        };
        let target = work.join(workdir::output(output.container.ext()));
        let result = convert::convert(tools, output, &inputs, &target, progress, cancel).await;
        progress.set_step(None);
        match result {
            Ok(Finished::Done) => target,
            Ok(Finished::Cancelled) => return Ok(MediaDone::Paused),
            Err(e) => return Ok(not_converted(&e, output, tools)),
        }
    } else {
        match join_original(
            tools,
            &files,
            Some(output.original_ext.as_str()),
            job,
            work,
            progress,
            cancel,
        )
        .await?
        {
            Some(file) => file,
            None => return Ok(MediaDone::Paused),
        }
    };
    finish(&file, job, work, &sidecars).await
}

fn not_converted(error: &FfmpegError, output: &OutputJob, tools: &MediaTools) -> MediaDone {
    MediaDone::NotConverted(describe::conversion(error, output, tools))
}

/// The downloaded file as it came: the one file, or the video and audio joined
/// (copied, never re-encoded) into a file with extension `ext` (the planned
/// file's when `None`). `None` when stopped.
pub(super) async fn join_original(
    tools: &MediaTools,
    files: &[PathBuf],
    ext: Option<&str>,
    job: &Job,
    work: &Path,
    progress: &MediaProgress,
    cancel: &CancellationToken,
) -> Result<Option<PathBuf>, UserError> {
    match files {
        [single] => Ok(Some(single.clone())),
        [video, audio] => {
            let container = joined_container(ext, job);
            let joined = work.join(workdir::joined(container.ext()));
            progress.set_step(Some(ProcessingStep::Joining));
            let result = merge::join(tools, (video, audio), &joined, container, cancel).await;
            progress.set_step(None);
            match result.map_err(|e| describe::join(&e, tools))? {
                Finished::Done => Ok(Some(joined)),
                Finished::Cancelled => Ok(None),
            }
        }
        _ => Err(describe::addon_missing()),
    }
}

/// The container separately downloaded video and audio are joined into: the one `ext` names (the planned file's extension when `None`), always a video one.
pub(super) fn joined_container(ext: Option<&str>, job: &Job) -> Container {
    ext.or_else(|| job.output.extension().and_then(|e| e.to_str()))
        .and_then(Container::from_ext)
        .filter(|c| c.is_video())
        .unwrap_or(Container::Mkv)
}

/// The extension stored with a page job for its downloaded streams when they are not converted, as [`media::output::kept_ext`] chose it.
pub(super) fn stored_kept_ext(page: &PageJob) -> Option<&str> {
    match &page.output {
        Some(output) => Some(output.original_ext.as_str()),
        None => page.keep_ext.as_deref(),
    }
}

/// What ffmpeg reads for `output`: the downloaded media, each read only as the
/// format it was recognised as, and the subtitles, metadata and cover picture.
/// The outer error is the download's state, the inner one a file ffmpeg may not read.
pub(super) async fn inputs_for(
    output: &OutputJob,
    downloaded: &Downloaded,
    files: &[PathBuf],
    work: &Path,
) -> Result<Result<Inputs, FfmpegError>, UserError> {
    let (video, audio) = match (files, output.video) {
        ([video, audio], _) => (Some(video), Some(audio)),
        ([single], VideoAction::None) => (None, Some(single)),
        ([single], _) => (Some(single), None),
        _ => return Err(describe::addon_missing()),
    };
    let video = match merge::optional_input(video, Role::Video).await {
        Ok(video) => video,
        Err(e) => return Ok(Err(e)),
    };
    let audio = match merge::optional_input(audio, Role::Audio).await {
        Ok(audio) => audio,
        Err(e) => return Ok(Err(e)),
    };
    Ok(Ok(Inputs {
        video,
        audio,
        subtitles: downloaded
            .subtitles
            .iter()
            .map(|s| (s.format, work.join(&s.file), s.pick.clone()))
            .collect(),
        metadata: downloaded.metadata.as_ref().map(|m| work.join(m)),
        cover: downloaded
            .cover
            .as_ref()
            .map(|(f, format)| (*format, work.join(f))),
    }))
}

/// The subtitles saved next to the file, as `SubRip` files in `work` with their
/// language, made with ffmpeg; nothing when they go inside the file. One that
/// ffmpeg cannot read is left out. `None` when stopped; an error when ffmpeg does not start.
pub(super) async fn subtitle_files(
    tools: &MediaTools,
    output: &OutputJob,
    downloaded: &Downloaded,
    work: &Path,
    cancel: &CancellationToken,
) -> Result<Option<Vec<(PathBuf, String)>>, FfmpegError> {
    let mut made = Vec::new();
    if !output.sidecar_subtitles() {
        return Ok(Some(made));
    }
    for (n, subtitle) in downloaded.subtitles.iter().enumerate() {
        let target = work.join(workdir::converted_subtitle(n));
        match convert::to_srt(
            tools,
            subtitle.format,
            &work.join(&subtitle.file),
            &target,
            cancel,
        )
        .await
        {
            Ok(Finished::Done) => made.push((target, subtitle.pick.language.clone())),
            Ok(Finished::Cancelled) => return Ok(None),
            Err(e @ FfmpegError::Start(_)) => return Err(e),
            // One subtitle ffmpeg cannot read is left out; the video is saved without it.
            Err(e) => {
                warn!(language = %subtitle.pick.language, error = %e, "could not make a subtitle file");
            }
        }
    }
    Ok(Some(made))
}
