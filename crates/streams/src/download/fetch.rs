//! Fetching a stream's segments into the work folder, several at a time, decrypting those that need it.

#![forbid(unsafe_code)]

use std::collections::{HashMap, VecDeque};
use std::fs::{self, File};
use std::sync::{Arc, Mutex};

use tokio::sync::OnceCell;
use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;
use tracing::{debug, warn};
use url::Url;
use wirecatch_engine::{Fetcher, FileWork};

use super::{blocking, discarded};
use crate::crypto;
use crate::error::{Error, KeyProblem, Result, map_engine};
use crate::lock;
use crate::model::{Outcome, StreamProgress};
use crate::plan::{Piece, Plan};
use crate::workdir::WorkDir;

/// Longest key accepted (AES-128 keys are 16 bytes).
const KEY_LIMIT: u64 = 1024;

/// Downloads the queued pieces, `connections` at a time. `Some` when `cancel` paused the download first.
pub(super) async fn download_pieces(
    shared: Arc<Shared>,
    connections: usize,
    cancel: &CancellationToken,
) -> Result<Option<Outcome>> {
    let queued = lock(&shared.queue).len();
    if queued == 0 {
        return Ok(None);
    }
    let progress = shared.progress.clone();
    let total = shared.total;
    let result = fetch_all(shared, connections.clamp(1, queued)).await;
    if cancel.is_cancelled() {
        return Ok(Some(Outcome::Paused {
            segments_left: total.saturating_sub(progress.segments_done()),
        }));
    }
    result.map(|()| None)
}

/// The pieces still to fetch, and what earlier runs already saved.
pub(super) struct Queue {
    pub(super) jobs: VecDeque<Job>,
    /// Pieces already saved, joined tracks included.
    pub(super) done: usize,
    /// Their size in bytes.
    pub(super) bytes: u64,
}

/// Queues every piece of `plan` that `work` does not hold yet, moving the pieces out of the plan (which keeps each track's role and codecs for joining). A piece is held when its track is joined, when it is appended to its track's file, or when its segment is on disk.
pub(super) fn queue(plan: &mut Plan, work: &WorkDir) -> Queue {
    let mut queue = Queue {
        jobs: VecDeque::new(),
        done: 0,
        bytes: 0,
    };
    let mut number = 0;
    for (track, planned) in plan.tracks.iter_mut().enumerate() {
        let pieces = std::mem::take(&mut planned.pieces);
        if let Some(size) = work.joined.get(&track) {
            queue.done += pieces.len();
            queue.bytes += size;
            number += pieces.len();
            continue;
        }
        let appended = work.appended.get(&track).copied();
        if let Some(appended) = appended {
            queue.done += appended.next;
            queue.bytes += appended.size;
        }
        let first = appended.map_or(0, |a| a.next);
        for (index, piece) in pieces.into_iter().enumerate() {
            number += 1;
            if index < first {
                continue;
            }
            if let Some(size) = work.done.get(&(track, index)) {
                queue.done += 1;
                queue.bytes += size;
            } else {
                queue.jobs.push_back(Job {
                    track,
                    index,
                    number,
                    piece,
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
        if let Err(e) = joined.map_err(Error::Task).and_then(|r| r) {
            warn!(error = %e, "segment download failed; stopping the others");
            shared.stop.cancel();
            first_error.get_or_insert(e);
        }
    }
    first_error.map_or(Ok(()), Err)
}

/// One piece to fetch.
pub(super) struct Job {
    track: usize,
    index: usize,
    /// Position among all pieces, from 1, for messages.
    number: usize,
    piece: Piece,
}

pub(super) struct Shared {
    pub(super) fetcher: Fetcher,
    pub(super) work: Arc<WorkDir>,
    pub(super) progress: Arc<StreamProgress>,
    pub(super) keys: Keys,
    pub(super) queue: Mutex<VecDeque<Job>>,
    pub(super) file_work: FileWork,
    pub(super) total: usize,
    pub(super) stop: CancellationToken,
}

async fn worker(shared: Arc<Shared>) -> Result<()> {
    // The decryption memory, kept from segment to segment.
    let mut buffer = Vec::new();
    loop {
        if shared.stop.is_cancelled() {
            return Ok(());
        }
        let Some(job) = lock(&shared.queue).pop_front() else {
            return Ok(());
        };
        match fetch_piece(&shared, &job, &mut buffer).await {
            Ok(()) => shared.progress.segment_done(),
            // Cancelled, or stopped because another segment failed.
            Err(_) if shared.stop.is_cancelled() => return Ok(()),
            Err(e) => return Err(e),
        }
    }
}

/// Fetches one piece into its `.part` file, decrypts it if needed, moves it
/// to its final name and notes it in the log.
async fn fetch_piece(shared: &Shared, job: &Job, buffer: &mut Vec<u8>) -> Result<()> {
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
        .map_err(|e| {
            map_engine(e, |cause| Error::Segment {
                number: job.number,
                total: shared.total,
                cause,
            })
        })?;

    let work = shared.work.clone();
    let (track, index) = (job.track, job.index);
    let mut memory = std::mem::take(buffer);
    let (decrypted, memory) = blocking(&shared.file_work, move || {
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
                // Streamed file to file, a chunk at a time: memory use does not
                // grow with the segment's size.
                let mut input = File::open(&part)?;
                let mut output = File::create(&path)?;
                let Some(len) = crypto::decrypt(&mut input, &mut output, &key, &iv, &mut memory)?
                else {
                    drop(output);
                    discarded(&path, fs::remove_file(&path));
                    return Ok((false, memory));
                };
                output.sync_all()?;
                drop(input);
                fs::remove_file(&part)?;
                len
            }
        };
        work.record_segment(track, index, size)?;
        Ok((true, memory))
    })
    .await?;
    *buffer = memory;
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
pub(super) struct Keys {
    cells: Mutex<HashMap<Url, Arc<OnceCell<[u8; 16]>>>>,
}

impl Keys {
    async fn get(&self, fetcher: &Fetcher, url: &Url) -> Result<[u8; 16]> {
        let cell = lock(&self.cells).entry(url.clone()).or_default().clone();
        let key = cell
            .get_or_try_init(|| async {
                let response = fetcher.get(url, None, KEY_LIMIT).await.map_err(|e| {
                    map_engine(e, |e| Error::Key {
                        url: url.to_string(),
                        problem: KeyProblem::Fetch(e),
                    })
                })?;
                <[u8; 16]>::try_from(&response.body[..]).map_err(|_| Error::Key {
                    url: url.to_string(),
                    problem: KeyProblem::Length(response.body.len()),
                })
            })
            .await?;
        Ok(*key)
    }
}
