//! Joining the downloaded segments: each track appended into one file, then the tracks copied into one container by ffmpeg, or the single track moved into place.

#![forbid(unsafe_code)]

use std::fs::{self, File};
use std::io::{self, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use tokio_util::sync::CancellationToken;
use tracing::{info, warn};
use wirecatch_engine::naming;
use wirecatch_engine::units::MIB;

use super::{blocking, discarded};
use crate::codec::Container;
use crate::error::{Error, Result};
use crate::ffmpeg::{self, CopyInput, InputFormat};
use crate::model::{FALLBACK_FORMAT, Outcome, Role, StreamRequest, with_extension};
use crate::plan::Plan;
use crate::workdir::{Appended, WorkDir};

/// How much of a track is appended to its file before the appended segments are removed. Larger batches sync the disk less often; smaller ones need less room twice.
const CONCAT_BATCH: u64 = 64 * MIB;

/// Joins each track's segments into one file, then muxes the tracks with
/// ffmpeg (or, without ffmpeg, moves the single track into place).
pub(super) async fn join(
    plan: &Plan,
    counts: &[usize],
    work: Arc<WorkDir>,
    request: &StreamRequest,
    output: &Path,
    cancel: &CancellationToken,
) -> Result<Outcome> {
    for (track, &count) in counts.iter().enumerate() {
        if work.joined.contains_key(&track) {
            continue;
        }
        let (work, cancel) = (work.clone(), cancel.clone());
        if !blocking(move || concat(&work, track, count, CONCAT_BATCH, &cancel)).await? {
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
    let first = container_for(plan);
    // MP4 can still refuse codecs the stream did not name, or named wrongly.
    let containers = [Some(first), first.fallback()];
    let tracks = track_formats(plan, work).await?;
    let mut inputs = Vec::with_capacity(tracks.len());
    for (path, role, format) in &tracks {
        let Some(format) = format else {
            return Err(Error::UnknownFormat { role: *role });
        };
        inputs.push(CopyInput::new(path, *role, *format));
    }
    // What ffmpeg said for each container tried, in order.
    let mut failures: Vec<(Container, String)> = Vec::new();
    for container in containers.into_iter().flatten() {
        let extension = container.ext();
        let muxed = work.muxed_path(extension);
        let args = ffmpeg::copy_arguments(&inputs, &muxed, container)?;
        match ffmpeg::run(program, &args, &muxed, cancel).await {
            Ok(true) => {
                let output = output.to_path_buf();
                let target =
                    blocking(move || move_to_free_name(&muxed, &output, extension)).await?;
                return Ok(Some(target));
            }
            Ok(false) => return Ok(None),
            Err(Error::Ffmpeg(message)) => {
                warn!(%message, ?container, "ffmpeg failed");
                discarded(&muxed, tokio::fs::remove_file(&muxed).await);
                failures.push((container, message));
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
        let extension = InputFormat::detect(&from)?
            .unwrap_or(FALLBACK_FORMAT)
            .extension();
        move_to_free_name(&from, &output, extension)
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
            .map(|(container, message)| format!("as {}: {message}", container.label()))
            .collect::<Vec<_>>()
            .join("; "),
    }
}

/// Moves `from` to `output` with `extension`, and returns where it went. Onto
/// `output` itself when the extension leaves it unchanged, since that is the
/// caller's own path. Otherwise never onto another file: onto the first free
/// name such as `video (1).mkv` ([`naming::move_to_free_name`]). Blocks on the
/// file system.
fn move_to_free_name(from: &Path, output: &Path, extension: &str) -> io::Result<PathBuf> {
    let target = with_extension(output, extension);
    if target == output {
        fs::rename(from, &target)?;
        return Ok(target);
    }
    let (Some(dir), Some(name)) = (target.parent(), target.file_name().and_then(|n| n.to_str()))
    else {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("{} names no file in a folder", target.display()),
        ));
    };
    naming::move_to_free_name(from, dir, name)
}

/// The container the tracks of `plan` are copied into (see [`Container::for_copy`]).
fn container_for(plan: &Plan) -> Container {
    let audio_only = plan.tracks.iter().all(|t| t.role == Role::Audio);
    let codecs = plan.tracks.iter().flat_map(|t| t.codecs.iter().copied());
    Container::for_copy(codecs, audio_only)
}

/// Appends a track's segments into one file, notes it, and removes the segments as they are appended, so the disk never holds a segment twice for longer than one batch of `batch_size` bytes ([`CONCAT_BATCH`]). An interrupted run continues after the last batch noted, cutting the file back to it. Returns `false` if cancelled first. Blocks on the file system.
fn concat(
    work: &WorkDir,
    track: usize,
    count: usize,
    batch_size: u64,
    cancel: &CancellationToken,
) -> io::Result<bool> {
    let Appended { next, size } = work
        .appended
        .get(&track)
        .copied()
        .unwrap_or(Appended { next: 0, size: 0 });
    let mut file = fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(work.track_path(track))?;
    file.set_len(size)?;
    file.seek(SeekFrom::Start(size))?;
    let (mut written, mut batch_start, mut batch) = (size, next, 0u64);
    for index in next..count {
        if cancel.is_cancelled() {
            return Ok(false);
        }
        let mut segment = File::open(work.segment_path(track, index))?;
        let copied = io::copy(&mut segment, &mut file)?;
        written += copied;
        batch += copied;
        if batch >= batch_size || index + 1 == count {
            // The data is on disk before its line, and the line before the segments go.
            file.sync_data()?;
            let appended = Appended {
                next: index + 1,
                size: written,
            };
            work.record_appended(track, appended)?;
            // A segment that cannot be deleted costs room, not the join: the folder goes when the download ends.
            for done in batch_start..=index {
                discarded(
                    &work.segment_path(track, done),
                    work.remove_segment(track, done),
                );
            }
            (batch_start, batch) = (index + 1, 0);
        }
    }
    file.sync_all()?;
    drop(file);
    work.record_joined(track, written)?;
    discarded(&work.segment_folder(track), work.remove_segments(track));
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codec::Codec;
    use crate::download::fetch::queue;
    use crate::plan::{Piece, Track};

    /// Streams finishing at once with the same name each land on their own file, and a file already there stays.
    #[test]
    fn streams_moved_beside_one_output_never_replace_each_other() {
        const RACERS: usize = 8;
        let dir = wirecatch_test_support::private_dir("join-free-name").unwrap();
        // The caller's own name has another extension, so each move needs a free name.
        let output = dir.join("video.mp4");
        std::fs::write(dir.join("video.mkv"), b"already there").unwrap();
        let start = std::sync::Barrier::new(RACERS);
        let moved: Vec<PathBuf> = std::thread::scope(|scope| {
            let racers: Vec<_> = (0..RACERS)
                .map(|i| {
                    let (dir, output, start) = (&dir, &output, &start);
                    scope.spawn(move || {
                        let from = dir.join(format!("muxed-{i}"));
                        std::fs::write(&from, i.to_string()).unwrap();
                        start.wait();
                        move_to_free_name(&from, output, "mkv").unwrap()
                    })
                })
                .collect();
            racers.into_iter().map(|r| r.join().unwrap()).collect()
        });
        let mut contents: Vec<String> = moved
            .iter()
            .map(|path| std::fs::read_to_string(path).unwrap())
            .collect();
        contents.sort();
        let expected: Vec<String> = (0..RACERS).map(|i| i.to_string()).collect();
        assert_eq!(contents, expected, "{moved:?}");
        assert_eq!(
            std::fs::read(dir.join("video.mkv")).unwrap(),
            b"already there"
        );
        dir.remove().unwrap();
    }

    /// A media name core cut to length, with ` (1)` added, still takes the container's extension: a long title in Cyrillic takes two bytes a letter.
    #[test]
    fn a_long_name_moves_when_the_container_changes_its_extension() {
        let dir = wirecatch_test_support::private_dir("join-long-name").unwrap();
        let stem = naming::sanitize(&"ж".repeat(150));
        for name in [format!("{stem}.mp4"), format!("{stem} (1).mp4")] {
            let output = dir.join(&name);
            let from = dir.join("muxed");
            std::fs::write(&from, b"joined").unwrap();
            let moved = move_to_free_name(&from, &output, "mkv").unwrap();
            assert_eq!(moved, output.with_extension("mkv"));
            assert_eq!(std::fs::read(&moved).unwrap(), b"joined");
        }
        dir.remove().unwrap();
    }

    #[test]
    fn container_follows_the_codecs() {
        let plan = |codecs: &[Option<&str>]| Plan {
            tracks: codecs
                .iter()
                .map(|c| Track {
                    role: Role::Video,
                    codecs: c.map(Codec::list).unwrap_or_default(),
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
            Container::Mkv
        );
        assert_eq!(
            container_for(&plan(&[Some("avc1.4d401f"), Some("opus")])),
            Container::Mkv
        );
        assert_eq!(
            container_for(&plan(&[Some("vp09.00.40.08,mp4a.40.2")])),
            Container::Mkv
        );
    }

    #[test]
    fn both_container_failures_are_reported() {
        let both = failure_summary(&[
            (
                Container::Mp4,
                "codec not currently supported in container".into(),
            ),
            (Container::Mkv, "something else".into()),
        ]);
        assert_eq!(
            both,
            "as MP4: codec not currently supported in container; as MKV: something else"
        );
        assert_eq!(failure_summary(&[(Container::Mkv, "x".into())]), "x");
    }

    fn plan_of(counts: &[usize]) -> Plan {
        let url = url::Url::parse("https://x.example/s.ts").unwrap();
        Plan {
            tracks: counts
                .iter()
                .map(|&n| Track {
                    role: Role::Video,
                    codecs: Vec::new(),
                    pieces: vec![Piece::plain(url.clone(), None); n],
                })
                .collect(),
        }
    }

    #[test]
    fn segments_are_removed_as_they_are_appended_and_an_interrupted_join_continues() {
        let dir = wirecatch_test_support::private_dir("concat").unwrap();
        let out = dir.path().join("video.ts");
        let work = WorkDir::open(&out, 1, &[3]).unwrap();
        for (index, bytes) in [b"aaaa", b"bbbb", b"cccc"].into_iter().enumerate() {
            fs::write(work.segment_path(0, index), bytes).unwrap();
            work.record_segment(0, index, 4).unwrap();
        }
        // A run noted the first two segments appended, removed them, and stopped half way through the third.
        fs::write(work.track_path(0), b"aaaabbbbcc").unwrap();
        work.record_appended(0, Appended { next: 2, size: 8 })
            .unwrap();
        work.remove_segment(0, 0).unwrap();
        work.remove_segment(0, 1).unwrap();
        drop(work);

        let work = WorkDir::open(&out, 1, &[3]).unwrap();
        let mut plan = plan_of(&[3]);
        let queued = queue(&mut plan, &work);
        assert!(
            queued.jobs.is_empty(),
            "nothing appended or saved is fetched again"
        );
        assert_eq!((queued.done, queued.bytes), (3, 12));
        assert!(concat(&work, 0, 3, 4, &CancellationToken::new()).unwrap());
        assert_eq!(fs::read(work.track_path(0)).unwrap(), b"aaaabbbbcccc");
        assert!(!work.segment_path(0, 2).exists());
        assert_eq!(work_joined_after_reopen(&out), Some(12));
        dir.remove().unwrap();
    }

    /// Each batch's segments go as soon as the batch is noted, not when the whole track is joined: a join that stops at a later segment has already freed the earlier ones.
    #[test]
    fn appended_segments_go_before_the_join_ends() {
        let dir = wirecatch_test_support::private_dir("concat-batches").unwrap();
        let out = dir.path().join("video.ts");
        let work = WorkDir::open(&out, 1, &[3]).unwrap();
        for (index, bytes) in [b"aaaa", b"bbbb"].into_iter().enumerate() {
            fs::write(work.segment_path(0, index), bytes).unwrap();
            work.record_segment(0, index, 4).unwrap();
        }
        // The third segment is missing, so the join stops there.
        assert!(concat(&work, 0, 3, 4, &CancellationToken::new()).is_err());
        assert!(!work.segment_path(0, 0).exists());
        assert!(!work.segment_path(0, 1).exists());
        drop(work);
        let work = WorkDir::open(&out, 1, &[3]).unwrap();
        assert_eq!(work.appended.get(&0), Some(&Appended { next: 2, size: 8 }));
        work.remove().unwrap();
        dir.remove().unwrap();
    }

    /// A crash after a batch is noted but before its segments are removed leaves them on disk: they are neither fetched nor appended again, and go with the rest.
    #[test]
    fn segments_left_by_a_crash_after_their_batch_was_noted_are_not_appended_twice() {
        let dir = wirecatch_test_support::private_dir("concat-crash").unwrap();
        let out = dir.path().join("video.ts");
        let work = WorkDir::open(&out, 1, &[3]).unwrap();
        for (index, bytes) in [b"aaaa", b"bbbb", b"cccc"].into_iter().enumerate() {
            fs::write(work.segment_path(0, index), bytes).unwrap();
            work.record_segment(0, index, 4).unwrap();
        }
        fs::write(work.track_path(0), b"aaaabbbb").unwrap();
        work.record_appended(0, Appended { next: 2, size: 8 })
            .unwrap();
        drop(work);

        let work = WorkDir::open(&out, 1, &[3]).unwrap();
        let mut plan = plan_of(&[3]);
        let queued = queue(&mut plan, &work);
        assert!(queued.jobs.is_empty());
        assert_eq!((queued.done, queued.bytes), (3, 12));
        assert!(concat(&work, 0, 3, 4, &CancellationToken::new()).unwrap());
        assert_eq!(fs::read(work.track_path(0)).unwrap(), b"aaaabbbbcccc");
        assert!(!work.segment_path(0, 0).exists());
        work.remove().unwrap();
        dir.remove().unwrap();
    }

    fn work_joined_after_reopen(out: &Path) -> Option<u64> {
        WorkDir::open(out, 1, &[3]).unwrap().joined.get(&0).copied()
    }
}
