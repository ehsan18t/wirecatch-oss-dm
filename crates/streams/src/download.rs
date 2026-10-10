//! Downloading a stream: segments in parallel into the work folder, then one
//! file out of them.

use std::collections::{HashMap, VecDeque};
use std::fs::{self, File};
use std::io::{self, BufWriter, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};

use tokio::sync::OnceCell;
use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;
use tracing::{debug, info, warn};
use url::Url;
use wirecatch_engine::{Fetcher, naming};

use crate::crypto;
use crate::error::{Error, Result};
use crate::ffmpeg::{self, Container};
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

    let mut jobs = VecDeque::new();
    let (mut done, mut bytes) = (0, 0);
    let mut number = 0;
    for (track, planned) in plan.tracks.iter().enumerate() {
        if let Some(size) = work.joined.get(&track) {
            done += planned.pieces.len();
            bytes += size;
            number += planned.pieces.len();
            continue;
        }
        for (index, piece) in planned.pieces.iter().enumerate() {
            number += 1;
            match work.done.get(&(track, index)) {
                Some(size) => {
                    done += 1;
                    bytes += size;
                }
                None => jobs.push_back(Job {
                    track,
                    index,
                    number,
                    piece: piece.clone(),
                }),
            }
        }
    }
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
            stop: stop.clone(),
        });
        let mut set = JoinSet::new();
        for _ in 0..workers {
            set.spawn(worker(shared.clone()));
        }
        let mut first_error = None;
        while let Some(joined) = set.join_next().await {
            if let Err(e) = joined.map_err(Error::task).and_then(|r| r) {
                warn!(error = %e, "segment download failed; stopping the others");
                stop.cancel();
                first_error.get_or_insert(e);
            }
        }
        if cancel.is_cancelled() {
            return Ok(Outcome::Paused {
                segments_left: total.saturating_sub(progress.segments_done()),
            });
        }
        if let Some(error) = first_error {
            return Err(error);
        }
    }

    progress.set_stage(Stage::Joining);
    join(&plan, work, request, &output, cancel).await
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
            progress.add_bytes(n)
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
        let size = match key {
            None => {
                fs::rename(&part, &path)?;
                fs::metadata(&path)?.len()
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
                output.flush()?;
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
                let fetched = fetcher
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
                <[u8; 16]>::try_from(&fetched.body[..]).map_err(|_| Error::Key {
                    url: url.to_string(),
                    reason: format!("it is {} bytes long instead of 16", fetched.body.len()),
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

    let audio_only = plan.tracks.iter().all(|t| t.role == Role::Audio);
    let saved = match &request.ffmpeg {
        Some(program) => {
            let first = container_for(plan);
            let containers: &[Container] = match first {
                // Codecs the stream did not name may not fit MP4.
                Container::Mp4 => &[Container::Mp4, Container::Matroska],
                Container::Matroska => &[Container::Matroska],
            };
            let inputs: Vec<(PathBuf, Role)> = plan
                .tracks
                .iter()
                .enumerate()
                .map(|(i, t)| (work.track_path(i), t.role))
                .collect();
            let inputs: Vec<(&Path, Role)> =
                inputs.iter().map(|(p, r)| (p.as_path(), *r)).collect();
            let mut saved = None;
            // What ffmpeg said for each container tried, in order.
            let mut failures: Vec<(Container, String)> = Vec::new();
            for container in containers {
                let extension = container.extension(audio_only);
                let muxed = work.muxed_path(extension);
                let args = ffmpeg::arguments(&inputs, &muxed, *container);
                match ffmpeg::run(program, &args, cancel).await {
                    Ok(true) => {
                        let (from, output) = (muxed.clone(), output.to_path_buf());
                        let target = blocking(move || {
                            let target = free_target(&output, extension)?;
                            fs::rename(from, &target)?;
                            Ok(target)
                        })
                        .await?;
                        saved = Some(target);
                        break;
                    }
                    Ok(false) => return Ok(Outcome::Paused { segments_left: 0 }),
                    Err(e @ Error::FfmpegMissing(_)) => return Err(e),
                    Err(Error::Ffmpeg(message)) => {
                        warn!(%message, ?container, "ffmpeg failed");
                        let _ = fs::remove_file(&muxed);
                        failures.push((*container, message));
                    }
                    Err(e) => return Err(e),
                }
            }
            match saved {
                Some(saved) => saved,
                None => return Err(Error::Ffmpeg(failure_summary(&failures))),
            }
        }
        None => {
            let (from, output) = (work.track_path(0), output.to_path_buf());
            blocking(move || {
                let target = free_target(&output, sniff(&from)?)?;
                fs::rename(&from, &target)?;
                Ok(target)
            })
            .await?
        }
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

/// The extension for a track saved as is, from its first bytes.
fn sniff(path: &Path) -> io::Result<&'static str> {
    let mut head = [0u8; 512];
    let mut file = File::open(path)?;
    let mut len = 0;
    while len < head.len() {
        match file.read(&mut head[len..])? {
            0 => break,
            n => len += n,
        }
    }
    let head = &head[..len];
    Ok(if head.first() == Some(&0x47) {
        "ts"
    } else if head
        .get(4..8)
        .is_some_and(|b| matches!(b, b"ftyp" | b"styp" | b"moov" | b"moof" | b"sidx" | b"free"))
    {
        "mp4"
    } else if head.starts_with(&[0x1A, 0x45, 0xDF, 0xA3]) {
        if head.windows(4).any(|w| w == b"webm") {
            "webm"
        } else {
            "mkv"
        }
    } else if head.starts_with(b"ID3")
        || (head.len() > 1 && head[0] == 0xFF && head[1] & 0xF6 == 0xF0)
    {
        "aac"
    } else {
        "ts"
    })
}

/// Runs file system work off the async threads.
async fn blocking<T: Send + 'static>(
    f: impl FnOnce() -> io::Result<T> + Send + 'static,
) -> Result<T> {
    Ok(tokio::task::spawn_blocking(f)
        .await
        .map_err(Error::task)??)
}

/// Locks a mutex; one poisoned by a panic elsewhere is still usable here.
fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
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

    #[test]
    fn sniffing_names_the_format() {
        let dir = std::env::temp_dir().join(format!("wirecatch-sniff-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        for (bytes, ext) in [
            (&[0x47u8, 0x40, 0x11][..], "ts"),
            (b"\0\0\0\x18ftypisom", "mp4"),
            (
                &[
                    0x1A, 0x45, 0xDF, 0xA3, 0x42, 0x82, 0x84, b'w', b'e', b'b', b'm',
                ],
                "webm",
            ),
            (b"ID3\x04", "aac"),
            (&[0xFF, 0xF1, 0x50], "aac"),
            (b"", "ts"),
        ] {
            let path = dir.join("track");
            fs::write(&path, bytes).unwrap();
            assert_eq!(sniff(&path).unwrap(), ext, "{bytes:?}");
        }
        let _ = fs::remove_dir_all(&dir);
    }
}
