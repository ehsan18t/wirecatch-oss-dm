//! The temporary folder next to the output, which makes a stream download
//! resumable.
//!
//! `<output name>.wcstream/` holds:
//! - `stream.txt`: which plan the folder belongs to (a fingerprint of its
//!   segments). A folder for another plan is cleared.
//! - `done.log`: one line per finished segment (`S <track> <index> <size>`), per batch of segments appended to their track (`A <track> <next index> <track size>`) and per joined track (`J <track> <size>`), appended as they finish. A line whose file is missing or has another size is ignored, so a crash at any point only costs the segments it interrupted.
//! - `t<track>/<index>.seg`: finished segments, decrypted. A segment is
//!   written as `.part` and renamed once complete, and removed once it is
//!   appended to its track.
//! - `track<n>`: a track's segments joined, before muxing.

#![forbid(unsafe_code)]

use std::collections::HashMap;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use crate::lock;

/// Added to the output file name to name the temporary folder that holds a
/// stream download in progress (for example `video.mp4.wcstream`).
pub const DIR_SUFFIX: &str = ".wcstream";

const HEADER: &str = "stream.txt";
const LOG: &str = "done.log";

/// The first words of `stream.txt`, naming the layout of the folder. A folder written in another layout is cleared, like one for another plan, except one of [`PREVIOUS_VERSION`].
const FORMAT_VERSION: &str = "wirecatch-stream 2";

/// The layout before [`FORMAT_VERSION`]: the same lines without `A`, so its folders are kept and renamed to the current layout.
const PREVIOUS_VERSION: &str = "wirecatch-stream 1";

/// How far a track's segments are appended to its file: the next segment to append, and the file's size up to there.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Appended {
    pub next: usize,
    pub size: u64,
}

pub(crate) struct WorkDir {
    dir: PathBuf,
    /// `None` once the folder is removed.
    log: Mutex<Option<File>>,
    /// Finished segments and their sizes, by `(track, index)`.
    pub done: HashMap<(usize, usize), u64>,
    /// Tracks whose segments are partly appended to their file.
    pub appended: HashMap<usize, Appended>,
    /// Joined tracks and their sizes.
    pub joined: HashMap<usize, u64>,
}

/// What the log says, before it is checked against the files.
#[derive(Default)]
struct Log {
    done: HashMap<(usize, usize), u64>,
    appended: HashMap<usize, Appended>,
    joined: HashMap<usize, u64>,
}

impl Log {
    fn read(text: &str) -> Self {
        let mut log = Log::default();
        for line in text.lines() {
            let fields: Vec<&str> = line.split(' ').collect();
            match fields.as_slice() {
                ["S", track, index, size] => {
                    if let (Ok(track), Ok(index), Ok(size)) =
                        (track.parse(), index.parse(), size.parse())
                    {
                        log.done.insert((track, index), size);
                    }
                }
                ["A", track, next, size] => {
                    if let (Ok(track), Ok(next), Ok(size)) =
                        (track.parse(), next.parse(), size.parse())
                    {
                        let later = log.appended.get(&track).is_none_or(|a| a.next < next);
                        if later {
                            log.appended.insert(track, Appended { next, size });
                        }
                    }
                }
                ["J", track, size] => {
                    if let (Ok(track), Ok(size)) = (track.parse(), size.parse()) {
                        log.joined.insert(track, size);
                    }
                }
                // A line cut short by a crash, or garbage: ignored.
                _ => {}
            }
        }
        log
    }
}

impl WorkDir {
    /// Opens (or starts) the folder for `output`. `pieces` holds the number of
    /// pieces of each track. Blocks on the file system.
    pub fn open(output: &Path, fingerprint: u64, pieces: &[usize]) -> io::Result<Self> {
        let dir = path_for(output);
        let header = format!("{FORMAT_VERSION} {fingerprint:016x}\n");
        let previous = format!("{PREVIOUS_VERSION} {fingerprint:016x}\n");
        let written = fs::read_to_string(dir.join(HEADER));
        let current = written.as_ref().is_ok_and(|h| *h == header);
        let reuse = current || written.is_ok_and(|h| h == previous);
        if !reuse {
            remove_dir(&dir)?;
        }
        for track in 0..pieces.len() {
            fs::create_dir_all(track_dir(&dir, track))?;
        }
        if !current {
            fs::write(dir.join(HEADER), header)?;
        }
        let log = match fs::read_to_string(dir.join(LOG)) {
            Ok(text) if reuse => Log::read(&text),
            Ok(_) => Log::default(),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Log::default(),
            Err(e) => return Err(e),
        };
        let mut work = Self {
            log: Mutex::new(Some(
                OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(dir.join(LOG))?,
            )),
            dir,
            done: HashMap::new(),
            appended: HashMap::new(),
            joined: HashMap::new(),
        };
        let size_is = |path: PathBuf, size: u64| fs::metadata(path).is_ok_and(|m| m.len() == size);
        work.done = log
            .done
            .into_iter()
            .filter(|((track, index), size)| {
                pieces.get(*track).is_some_and(|n| index < n)
                    && size_is(work.segment_path(*track, *index), *size)
            })
            .collect();
        // The track file holds at least what was appended: it is synced before its line is written, and cut back to it before appending more.
        work.appended = log
            .appended
            .into_iter()
            .filter(|(track, appended)| {
                pieces.get(*track).is_some_and(|n| appended.next <= *n)
                    && fs::metadata(work.track_path(*track)).is_ok_and(|m| m.len() >= appended.size)
            })
            .collect();
        work.joined = log
            .joined
            .into_iter()
            .filter(|(track, size)| {
                *track < pieces.len() && size_is(work.track_path(*track), *size)
            })
            .collect();
        Ok(work)
    }

    pub fn segment_path(&self, track: usize, index: usize) -> PathBuf {
        track_dir(&self.dir, track).join(format!("{index:06}.seg"))
    }

    pub fn part_path(&self, track: usize, index: usize) -> PathBuf {
        track_dir(&self.dir, track).join(format!("{index:06}.part"))
    }

    /// The folder of a track's segments.
    pub fn segment_folder(&self, track: usize) -> PathBuf {
        track_dir(&self.dir, track)
    }

    pub fn track_path(&self, track: usize) -> PathBuf {
        self.dir.join(format!("track{track}"))
    }

    /// Where ffmpeg writes before the result is moved to the output.
    pub fn muxed_path(&self, extension: &str) -> PathBuf {
        self.dir.join(format!("joined.{extension}"))
    }

    /// Notes a finished segment. Blocks on the file system.
    pub fn record_segment(&self, track: usize, index: usize, size: u64) -> io::Result<()> {
        self.append(&format!("S {track} {index} {size}\n"), Sync::No)
    }

    /// Notes that a track's segments before `appended.next` are in its file, which is `appended.size` long. The line is on disk when this returns, so the segments can be removed. Blocks on the file system.
    pub fn record_appended(&self, track: usize, appended: Appended) -> io::Result<()> {
        let Appended { next, size } = appended;
        self.append(&format!("A {track} {next} {size}\n"), Sync::Yes)
    }

    /// Notes a joined track. Blocks on the file system.
    pub fn record_joined(&self, track: usize, size: u64) -> io::Result<()> {
        self.append(&format!("J {track} {size}\n"), Sync::No)
    }

    fn append(&self, line: &str, sync: Sync) -> io::Result<()> {
        match lock(&self.log).as_mut() {
            Some(log) => {
                log.write_all(line.as_bytes())?;
                match sync {
                    Sync::Yes => log.sync_data(),
                    Sync::No => Ok(()),
                }
            }
            None => Err(io::Error::other(
                "the download's temporary folder was already removed",
            )),
        }
    }

    /// Removes one segment once it is appended to its track.
    pub fn remove_segment(&self, track: usize, index: usize) -> io::Result<()> {
        match fs::remove_file(self.segment_path(track, index)) {
            Err(e) if e.kind() != io::ErrorKind::NotFound => Err(e),
            _ => Ok(()),
        }
    }

    /// Removes the segments of one track once they are joined.
    pub fn remove_segments(&self, track: usize) -> io::Result<()> {
        remove_dir(&track_dir(&self.dir, track))
    }

    /// Removes the whole folder. Blocks on the file system.
    pub fn remove(&self) -> io::Result<()> {
        // The log must be closed first: Windows cannot delete an open file.
        lock(&self.log).take();
        remove_dir(&self.dir)
    }
}

/// Whether a log line must be on disk before the next step.
#[derive(Clone, Copy)]
enum Sync {
    Yes,
    No,
}

/// The folder of a track's segments.
fn track_dir(dir: &Path, track: usize) -> PathBuf {
    dir.join(format!("t{track}"))
}

/// The temporary folder for `output`.
pub(crate) fn path_for(output: &Path) -> PathBuf {
    let mut name = output.file_name().unwrap_or_default().to_owned();
    name.push(DIR_SUFFIX);
    output.with_file_name(name)
}

fn remove_dir(dir: &Path) -> io::Result<()> {
    match fs::remove_dir_all(dir) {
        Err(e) if e.kind() != io::ErrorKind::NotFound => Err(e),
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finished_segments_survive_only_with_the_right_size() {
        let dir = wirecatch_test_support::private_dir("workdir").unwrap();
        let out = dir.path().join("video.mp4");
        let work = WorkDir::open(&out, 1, &[3, 1]).unwrap();
        assert_eq!(path_for(&out).file_name().unwrap(), "video.mp4.wcstream");
        fs::write(work.segment_path(0, 0), b"abc").unwrap();
        work.record_segment(0, 0, 3).unwrap();
        fs::write(work.segment_path(0, 1), b"abcd").unwrap();
        work.record_segment(0, 1, 99).unwrap(); // size does not match
        work.record_segment(0, 2, 5).unwrap(); // file missing
        fs::write(work.segment_path(1, 0), b"x").unwrap();
        work.record_segment(1, 0, 1).unwrap();
        work.record_segment(7, 0, 1).unwrap(); // no such track
        work.append("S 0 garbage\nJ", Sync::No).unwrap(); // a torn line
        drop(work);

        let work = WorkDir::open(&out, 1, &[3, 1]).unwrap();
        let mut done: Vec<_> = work.done.keys().copied().collect();
        done.sort_unstable();
        assert_eq!(done, [(0, 0), (1, 0)]);
        drop(work);

        // Another plan clears the folder.
        let work = WorkDir::open(&out, 2, &[3, 1]).unwrap();
        assert!(work.done.is_empty());
        assert!(!work.segment_path(0, 0).exists());
        work.remove().unwrap();
        assert!(!path_for(&out).exists());
        dir.remove().unwrap();
    }

    #[test]
    fn appended_segments_count_only_while_the_track_file_holds_them() {
        let dir = wirecatch_test_support::private_dir("workdir-appended").unwrap();
        let out = dir.path().join("video.mp4");
        let work = WorkDir::open(&out, 1, &[4]).unwrap();
        fs::write(work.track_path(0), b"0123456789").unwrap();
        work.record_appended(0, Appended { next: 1, size: 4 })
            .unwrap();
        work.record_appended(0, Appended { next: 2, size: 10 })
            .unwrap();
        drop(work);
        let work = WorkDir::open(&out, 1, &[4]).unwrap();
        assert_eq!(work.appended.get(&0), Some(&Appended { next: 2, size: 10 }));
        drop(work);
        // A track file shorter than the log says (it was lost): nothing counts as appended.
        fs::write(path_for(&out).join("track0"), b"01").unwrap();
        let work = WorkDir::open(&out, 1, &[4]).unwrap();
        assert!(work.appended.is_empty());
        work.remove().unwrap();
        dir.remove().unwrap();
    }

    /// A download paused by the build before (layout 1, which had no `A` lines) resumes with its segments and joined tracks.
    #[test]
    fn a_folder_of_the_layout_before_is_kept() {
        let dir = wirecatch_test_support::private_dir("workdir-v1").unwrap();
        let out = dir.path().join("video.mp4");
        let folder = path_for(&out);
        fs::create_dir_all(folder.join("t0")).unwrap();
        fs::write(folder.join(HEADER), "wirecatch-stream 1 0000000000000001\n").unwrap();
        fs::write(folder.join("t0").join("000000.seg"), b"abc").unwrap();
        fs::write(folder.join("track1"), b"joined").unwrap();
        fs::write(folder.join(LOG), "S 0 0 3\nJ 1 6\n").unwrap();

        let work = WorkDir::open(&out, 1, &[2, 1]).unwrap();
        assert_eq!(work.done.get(&(0, 0)), Some(&3));
        assert_eq!(work.joined.get(&1), Some(&6));
        drop(work);
        // It is now written in the current layout, and still kept.
        let header = fs::read_to_string(folder.join(HEADER)).unwrap();
        assert_eq!(header, format!("{FORMAT_VERSION} 0000000000000001\n"));
        let work = WorkDir::open(&out, 1, &[2, 1]).unwrap();
        assert_eq!(work.done.get(&(0, 0)), Some(&3));
        // A folder of the layout before for another plan is cleared as any is.
        drop(work);
        fs::write(folder.join(HEADER), "wirecatch-stream 1 0000000000000002\n").unwrap();
        let work = WorkDir::open(&out, 1, &[2, 1]).unwrap();
        assert!(work.done.is_empty());
        work.remove().unwrap();
        dir.remove().unwrap();
    }
}
