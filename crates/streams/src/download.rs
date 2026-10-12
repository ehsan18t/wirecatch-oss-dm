//! Downloading a stream: segments in parallel into the work folder, then one
//! file out of them.

use std::collections::{HashMap, VecDeque};
use std::fs::{self, File};
use std::io::{self, BufWriter};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use tokio::sync::OnceCell;
use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;
use tracing::{debug, info, warn};
use url::Url;
use wirecatch_engine::{Fetcher, naming};

use crate::crypto;
use crate::error::{Error, Result};
use crate::ffmpeg::{self, Container, InputFormat};
use crate::lock;
use crate::model::{Outcome, Stage, StreamProgress, StreamRequest, with_extension};
use crate::plan::{Piece, Plan, Role, Source, mp4_compatible};
use crate::workdir::WorkDir;

/// Longest key accepted (AES-128 keys are 16 bytes).
const KEY_LIMIT: u64 = 1024;

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
    let plan = source
        .plan(&fetcher, &request.video, &request.audio)
        .await?;
    if plan.tracks.len() > 1 && request.ffmpeg.is_none() {
        return Err(Error::NeedsFfmpeg);
    }

    let counts: Vec<usize> = plan.tracks.iter().map(|t| t.pieces.len()).collect();
    let total: usize = counts.iter().sum();
    let work = {
        let (output, fingerprint) = (output.clone(), plan.fingerprint());
        Arc::new(blocking(move || WorkDir::open(&output, fingerprint, &counts)).await?)
    };

    let Queue { jobs, done, bytes } = queue(&plan, &work);
    info!(
        total,
        kept = done,
        tracks = plan.tracks.len(),
        "stream planned"
    );
    progress.start(total, done, bytes);
    progress.set_stage(Stage::Downloading);

    if !jobs.is_empty() {
        let workers = request.options.connections.clamp(1, jobs.len());
        let shared = Arc::new(Shared {
            fetcher,
            work: work.clone(),
            progress: progress.clone(),
            keys: Keys::default(),
            queue: Mutex::new(jobs),
            total,
            stop,
        });
        let result = fetch_all(shared, workers).await;
        if cancel.is_cancelled() {
            return Ok(Outcome::Paused {
                segments_left: total.saturating_sub(progress.segments_done()),
            });
        }
        result?;
    }

    progress.set_stage(Stage::Joining);
    join(&plan, work, request, &output, cancel).await
}

/// The pieces still to fetch, and what earlier runs already saved.
struct Queue {
    jobs: VecDeque<Job>,
    /// Pieces already saved, joined tracks included.
    done: usize,
    /// Their size in bytes.
    bytes: u64,
}

/// Queues every piece of `plan` that `work` does not hold yet.
fn queue(plan: &Plan, work: &WorkDir) -> Queue {
    let mut queue = Queue {
        jobs: VecDeque::new(),
        done: 0,
        bytes: 0,
    };
    let mut number = 0;
    for (track, planned) in plan.tracks.iter().enumerate() {
        if let Some(size) = work.joined.get(&track) {
            queue.done += planned.pieces.len();
            queue.bytes += size;
            number += planned.pieces.len();
            continue;
        }
        for (index, piece) in planned.pieces.iter().enumerate() {
            number += 1;
            if let Some(size) = work.done.get(&(track, index)) {
                queue.done += 1;
                queue.bytes += size;
            } else {
                queue.jobs.push_back(Job {
                    track,
                    index,
                    number,
                    piece: piece.clone(),
                });
            }
        }
    }
    queue
}

/// Fetches the queued pieces with `workers` tasks. The first failure stops
/// the others and is returned.
async fn fetch_all(shared: Arc<Shared>, workers: usize) -> Result<()> {
    let mut set = JoinSet::new();
    for _ in 0..workers {
        set.spawn(worker(shared.clone()));
    }
    let mut first_error = None;
    while let Some(joined) = set.join_next().await {
        if let Err(e) = joined.map_err(|e| Error::task(&e)).and_then(|r| r) {
            warn!(error = %e, "segment download failed; stopping the others");
            shared.stop.cancel();
            first_error.get_or_insert(e);
        }
    }
    first_error.map_or(Ok(()), Err)
}

/// One piece to fetch.
struct Job {
    track: usize,
    index: usize,
    /// Position among all pieces, from 1, for messages.
    number: usize,
    piece: Piece,
}

struct Shared {
    fetcher: Fetcher,
    work: Arc<WorkDir>,
    progress: Arc<StreamProgress>,
    keys: Keys,
    queue: Mutex<VecDeque<Job>>,
    total: usize,
    stop: CancellationToken,
}

async fn worker(shared: Arc<Shared>) -> Result<()> {
    loop {
        if shared.stop.is_cancelled() {
            return Ok(());
        }
        let Some(job) = lock(&shared.queue).pop_front() else {
            return Ok(());
        };
        match fetch_piece(&shared, &job).await {
            Ok(()) => shared.progress.segment_done(),
            // Cancelled, or stopped because another segment failed.
            Err(_) if shared.stop.is_cancelled() => return Ok(()),
            Err(e) => return Err(e),
        }
    }
}

/// Fetches one piece into its `.part` file, decrypts it if needed, moves it
/// to its final name and notes it in the log.
async fn fetch_piece(shared: &Shared, job: &Job) -> Result<()> {
    let key = match &job.piece.encryption {
        Some(encryption) => Some((
            shared
                .keys
                .get(&shared.fetcher, &encryption.key_url)
                .await?,
            encryption.iv,
        )),
        None => None,
    };
    let part = shared.work.part_path(job.track, job.index);
    let progress = shared.progress.clone();
    shared
        .fetcher
        .save(&job.piece.url, job.piece.range.clone(), &part, move |n| {
            progress.add_bytes(n);
        })
        .await
        .map_err(|e| match e {
            wirecatch_engine::Error::Cancelled => Error::Cancelled,
            wirecatch_engine::Error::PrivateNetwork { .. } => Error::PrivateNetwork,
            source => Error::Segment {
                number: job.number,
                total: shared.total,
                source,
            },
        })?;

    let work = shared.work.clone();
    let (track, index) = (job.track, job.index);
    let decrypted = blocking(move || {
        let path = work.segment_path(track, index);
        // Each segment is on disk before it gets its final name and its line
        // in the log, so a power cut can never leave a logged segment whose
        // file is empty or short.
        let size = match key {
            None => {
                // Write access: Windows refuses to flush a read-only handle.
                let file = fs::OpenOptions::new().write(true).open(&part)?;
                file.sync_all()?;
                let size = file.metadata()?.len();
                drop(file);
                fs::rename(&part, &path)?;
                size
            }
            Some((key, iv)) => {
                // Streamed file to file: memory use does not grow with the
                // segment's size.
                let mut input = File::open(&part)?;
                let mut output = BufWriter::with_capacity(256 * 1024, File::create(&path)?);
                let Some(len) = crypto::decrypt(&mut input, &mut output, &key, &iv)? else {
                    drop(output);
                    let _ = fs::remove_file(&path);
                    return Ok(false);
                };
                output
                    .into_inner()
                    .map_err(io::IntoInnerError::into_error)?
                    .sync_all()?;
                drop(input);
                fs::remove_file(&part)?;
                len
            }
        };
        work.record_segment(track, index, size)?;
        Ok(true)
    })
    .await?;
    if !decrypted {
        return Err(Error::Decrypt {
            number: job.number,
            total: shared.total,
        });
    }
    debug!(track, index, "segment done");
    Ok(())
}

/// Decryption keys, each fetched once however many segments use it.
#[derive(Default)]
struct Keys {
    cells: Mutex<HashMap<Url, Arc<OnceCell<[u8; 16]>>>>,
}

impl Keys {
    async fn get(&self, fetcher: &Fetcher, url: &Url) -> Result<[u8; 16]> {
        let cell = lock(&self.cells).entry(url.clone()).or_default().clone();
        let key = cell
            .get_or_try_init(|| async {
                let response = fetcher
                    .get(url, None, KEY_LIMIT)
                    .await
                    .map_err(|e| match e {
                        wirecatch_engine::Error::Cancelled => Error::Cancelled,
                        wirecatch_engine::Error::PrivateNetwork { .. } => Error::PrivateNetwork,
                        e => Error::Key {
                            url: url.to_string(),
                            reason: e.to_string(),
                        },
                    })?;
                <[u8; 16]>::try_from(&response.body[..]).map_err(|_| Error::Key {
                    url: url.to_string(),
                    reason: format!("it is {} bytes long instead of 16", response.body.len()),
                })
            })
            .await?;
        Ok(*key)
    }
}

/// Joins each track's segments into one file, then muxes the tracks with
/// ffmpeg (or, without ffmpeg, moves the single track into place).
async fn join(
    plan: &Plan,
    work: Arc<WorkDir>,
    request: &StreamRequest,
    output: &Path,
    cancel: &CancellationToken,
) -> Result<Outcome> {
    for (track, planned) in plan.tracks.iter().enumerate() {
        if work.joined.contains_key(&track) {
            continue;
        }
        let (work, cancel, count) = (work.clone(), cancel.clone(), planned.pieces.len());
        if !blocking(move || concat(&work, track, count, &cancel)).await? {
            return Ok(Outcome::Paused { segments_left: 0 });
        }
    }

    let saved = match &request.ffmpeg {
        Some(program) => match mux(plan, &work, program, output, cancel).await? {
            Some(saved) => saved,
            None => return Ok(Outcome::Paused { segments_left: 0 }),
        },
        None => move_single_track(&work, output).await?,
    };

    let bytes = {
        let (saved, work) = (saved.clone(), work.clone());
        blocking(move || {
            let bytes = fs::metadata(&saved)?.len();
            work.remove()?;
            Ok(bytes)
        })
        .await?
    };
    info!(path = %saved.display(), bytes, "stream saved");
    Ok(Outcome::Completed { path: saved, bytes })
}

/// Muxes the joined tracks with `program` into the first container that takes
/// them and moves the result to a free name beside `output`. `None` when
/// `cancel` stopped ffmpeg.
async fn mux(
    plan: &Plan,
    work: &WorkDir,
    program: &Path,
    output: &Path,
    cancel: &CancellationToken,
) -> Result<Option<PathBuf>> {
    let audio_only = plan.tracks.iter().all(|t| t.role == Role::Audio);
    let containers: &[Container] = match container_for(plan) {
        // Codecs the stream did not name may not fit MP4.
        Container::Mp4 => &[Container::Mp4, Container::Matroska],
        Container::Matroska => &[Container::Matroska],
    };
    let tracks = track_formats(plan, work).await?;
    let mut inputs = Vec::with_capacity(tracks.len());
    for (path, role, format) in &tracks {
        let Some(format) = format else {
            return Err(Error::UnknownFormat { role: role.name() });
        };
        inputs.push((path.as_path(), *role, *format));
    }
    // What ffmpeg said for each container tried, in order.
    let mut failures: Vec<(Container, String)> = Vec::new();
    for container in containers {
        let extension = container.extension(audio_only);
        let muxed = work.muxed_path(extension);
        let args = ffmpeg::arguments(&inputs, &muxed, *container);
        match ffmpeg::run(program, &args, cancel).await {
            Ok(true) => {
                let output = output.to_path_buf();
                let target = blocking(move || {
                    let target = free_target(&output, extension)?;
                    fs::rename(muxed, &target)?;
                    Ok(target)
                })
                .await?;
                return Ok(Some(target));
            }
            Ok(false) => return Ok(None),
            Err(Error::Ffmpeg(message)) => {
                warn!(%message, ?container, "ffmpeg failed");
                let _ = fs::remove_file(&muxed);
                failures.push((*container, message));
            }
            Err(e) => return Err(e),
        }
    }
    Err(Error::Ffmpeg(failure_summary(&failures)))
}

/// Each joined track's file, role and format. ffmpeg is told each track's
/// format instead of guessing it from bytes a website chose, so an unknown
/// one (`None`) is never handed over.
async fn track_formats(
    plan: &Plan,
    work: &WorkDir,
) -> Result<Vec<(PathBuf, Role, Option<InputFormat>)>> {
    let tracks: Vec<(PathBuf, Role)> = plan
        .tracks
        .iter()
        .enumerate()
        .map(|(i, t)| (work.track_path(i), t.role))
        .collect();
    blocking(move || {
        tracks
            .into_iter()
            .map(|(path, role)| {
                let format = InputFormat::detect(&path)?;
                Ok((path, role, format))
            })
            .collect::<io::Result<Vec<_>>>()
    })
    .await
}

/// Moves the only track, joined without ffmpeg, to a free name beside `output`.
async fn move_single_track(work: &WorkDir, output: &Path) -> Result<PathBuf> {
    let (from, output) = (work.track_path(0), output.to_path_buf());
    blocking(move || {
        // Saved as is, never read by ffmpeg: an unknown format only needs an
        // extension, and `.ts` is the most likely one.
        let extension = InputFormat::detect(&from)?.map_or("ts", InputFormat::extension);
        let target = free_target(&output, extension)?;
        fs::rename(&from, &target)?;
        Ok(target)
    })
    .await
}

/// ffmpeg's messages for every container tried: one alone as is, several
/// each named, so the first (MP4) failure is never hidden by the fallback's.
fn failure_summary(failures: &[(Container, String)]) -> String {
    match failures {
        [] => "it produced no file".into(),
        [(_, message)] => message.clone(),
        several => several
            .iter()
            .map(|(container, message)| format!("as {}: {message}", container.name()))
            .collect::<Vec<_>>()
            .join("; "),
    }
}

/// `output` with `extension`. Never an unrelated file: when the extension
/// changed the name and a file by the new name exists, the first free name
/// such as `video (1).mkv` instead. Blocks on the file system.
fn free_target(output: &Path, extension: &str) -> io::Result<PathBuf> {
    let target = with_extension(output, extension);
    if target == output || !target.exists() {
        return Ok(target);
    }
    let (Some(dir), Some(name)) = (target.parent(), target.file_name().and_then(|n| n.to_str()))
    else {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            format!("{} already exists", target.display()),
        ));
    };
    Ok(naming::unique_path(dir, name))
}

/// MP4 when every codec the stream names fits it (or none is named), else
/// Matroska.
fn container_for(plan: &Plan) -> Container {
    let fits = plan
        .tracks
        .iter()
        .filter_map(|t| t.codecs.as_deref())
        .flat_map(|c| c.split(','))
        .map(str::trim)
        .filter(|c| !c.is_empty())
        .all(mp4_compatible);
    if fits {
        Container::Mp4
    } else {
        Container::Matroska
    }
}

/// Appends a track's segments into one file, notes it, and removes the
/// segments. Returns `false` if cancelled first. Blocks on the file system.
fn concat(
    work: &WorkDir,
    track: usize,
    count: usize,
    cancel: &CancellationToken,
) -> io::Result<bool> {
    let path = work.track_path(track);
    let mut out = BufWriter::with_capacity(1024 * 1024, File::create(&path)?);
    for index in 0..count {
        if cancel.is_cancelled() {
            return Ok(false);
        }
        let mut segment = File::open(work.segment_path(track, index))?;
        io::copy(&mut segment, &mut out)?;
    }
    let file = out.into_inner().map_err(io::IntoInnerError::into_error)?;
    // On disk before the segments it replaces are removed.
    file.sync_all()?;
    let size = file.metadata()?.len();
    drop(file);
    work.record_joined(track, size)?;
    work.remove_segments(track)?;
    Ok(true)
}

/// Runs file system work off the async threads.
async fn blocking<T: Send + 'static>(
    f: impl FnOnce() -> io::Result<T> + Send + 'static,
) -> Result<T> {
    Ok(tokio::task::spawn_blocking(f)
        .await
        .map_err(|e| Error::task(&e))??)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plan::Track;

    #[test]
    fn container_follows_the_codecs() {
        let plan = |codecs: &[Option<&str>]| Plan {
            tracks: codecs
                .iter()
                .map(|c| Track {
                    role: Role::Video,
                    codecs: c.map(str::to_string),
                    pieces: Vec::new(),
                })
                .collect(),
        };
        assert_eq!(
            container_for(&plan(&[Some("avc1.64001f,mp4a.40.2")])),
            Container::Mp4
        );
        assert_eq!(container_for(&plan(&[None])), Container::Mp4);
        assert_eq!(
            container_for(&plan(&[Some("vp8"), Some("vorbis")])),
            Container::Matroska
        );
        assert_eq!(
            container_for(&plan(&[Some("avc1.4d401f"), Some("opus")])),
            Container::Matroska
        );
    }

    #[test]
    fn both_container_failures_are_reported() {
        let both = failure_summary(&[
            (
                Container::Mp4,
                "codec not currently supported in container".into(),
            ),
            (Container::Matroska, "something else".into()),
        ]);
        assert_eq!(
            both,
            "as MP4: codec not currently supported in container; as Matroska: something else"
        );
        assert_eq!(failure_summary(&[(Container::Matroska, "x".into())]), "x");
    }
}
