//! Downloading a stream: segments in parallel into the work folder, then one
//! file out of them.

#![forbid(unsafe_code)]

mod fetch;
mod join;

use std::io;
use std::path::Path;
use std::sync::{Arc, Mutex};

use tokio_util::sync::CancellationToken;
use tracing::{info, warn};
use wirecatch_engine::{Fetcher, FileWork};

use crate::error::{Error, Result};
use crate::model::{Outcome, Stage, StreamProgress, StreamRequest};
use crate::source::Source;
use crate::workdir::WorkDir;
use fetch::{Keys, Queue, Shared, download_pieces, queue};
use join::join;

/// Downloads the stream at `request.url` and saves it at `request.output`.
///
/// Reads the playlist or manifest, picks the tracks ([`StreamRequest::video`]
/// and [`StreamRequest::audio`]), and refuses what it cannot download (live
/// broadcasts, DRM, tracks that need ffmpeg to be joined when none is given)
/// before fetching any segment. Segments are then downloaded
/// [`DownloadOptions::connections`](wirecatch_engine::DownloadOptions::connections)
/// at a time into a folder next to the output, decrypted if the stream uses
/// HLS AES-128, and joined: by ffmpeg with stream copy into MP4 (or Matroska
/// when the codecs do not fit MP4) when [`StreamRequest::ffmpeg`] is set,
/// otherwise end to end as the stream's own format (one track only).
///
/// `progress` can be read from any task while this runs. Cancelling `cancel`
/// stops the download and returns [`Outcome::Paused`]; running the same
/// request again downloads only the segments still missing. On success the
/// temporary folder is removed.
///
/// # Errors
///
/// See [`Error`]: every message says what went wrong and how to fix it.
pub async fn download(
    request: StreamRequest,
    progress: Arc<StreamProgress>,
    cancel: CancellationToken,
) -> Result<Outcome> {
    progress.set_stage(Stage::Preparing);
    let result = run(&request, &progress, &cancel).await;
    progress.set_stage(Stage::Finished);
    result
}

async fn run(
    request: &StreamRequest,
    progress: &Arc<StreamProgress>,
    cancel: &CancellationToken,
) -> Result<Outcome> {
    let output = std::path::absolute(&request.output)?;
    // Stopping the segment downloads after an error must not look like the
    // caller's cancellation, so the fetcher gets a child token.
    let stop = cancel.child_token();
    let fetcher = Fetcher::with_private_network(
        &request.url,
        &request.options,
        stop.clone(),
        request.private_network,
    )
    .await?;
    let source = Source::load(&fetcher).await?;
    let mut plan = source
        .plan(&fetcher, &request.video, &request.audio)
        .await?;
    if plan.tracks.len() > 1 && request.ffmpeg.is_none() {
        return Err(Error::NeedsFfmpeg);
    }

    let counts: Vec<usize> = plan.tracks.iter().map(|t| t.pieces.len()).collect();
    let total: usize = counts.iter().sum();
    let work = {
        let (output, fingerprint, counts) = (output.clone(), plan.fingerprint(), counts.clone());
        Arc::new(
            blocking(&request.file_work, move || {
                WorkDir::open(&output, fingerprint, &counts)
            })
            .await?,
        )
    };

    let Queue { jobs, done, bytes } = queue(&mut plan, &work);
    info!(
        total,
        kept = done,
        tracks = plan.tracks.len(),
        "stream planned"
    );
    progress.start(total, done, bytes);
    progress.set_stage(Stage::Downloading);
    let shared = Arc::new(Shared {
        fetcher,
        work: work.clone(),
        progress: progress.clone(),
        keys: Keys::default(),
        queue: Mutex::new(jobs),
        file_work: request.file_work.clone(),
        total,
        stop,
    });
    if let Some(paused) = download_pieces(shared, request.options.connections, cancel).await? {
        return Ok(paused);
    }
    progress.set_stage(Stage::Joining);
    join(&plan, &counts, work, request, &output, cancel).await
}

/// Runs file system work off the async threads, as part of the download's `file_work`.
async fn blocking<T: Send + 'static>(
    file_work: &FileWork,
    f: impl FnOnce() -> io::Result<T> + Send + 'static,
) -> Result<T> {
    Ok(file_work.spawn_blocking(f).await.map_err(Error::Task)??)
}

/// Logs a failure to delete a file the download no longer needs. It stops nothing: the file is in the download's temporary folder, which goes when the download ends.
fn discarded(path: &Path, deleted: io::Result<()>) {
    match deleted {
        Ok(()) => {}
        Err(e) if e.kind() == io::ErrorKind::NotFound => {}
        Err(e) => {
            warn!(path = %path.display(), error = %e, "could not delete a file that is no longer needed");
        }
    }
}
