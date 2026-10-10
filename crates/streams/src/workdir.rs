//! The temporary folder next to the output, which makes a stream download
//! resumable.
//!
//! `<output name>.wcstream/` holds:
//! - `stream.txt`: which plan the folder belongs to (a fingerprint of its
//!   segments). A folder for another plan is cleared.
//! - `done.log`: one line per finished segment (`S <track> <index> <size>`)
//!   and per joined track (`J <track> <size>`), appended as they finish. A
//!   line whose file is missing or has another size is ignored, so a crash at
//!   any point only costs the segments it interrupted.
//! - `t<track>/<index>.seg`: finished segments, decrypted. A segment is
//!   written as `.part` and renamed once complete.
//! - `track<n>`: a track's segments joined, before muxing.

use std::collections::HashMap;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// Added to the output file name to name the temporary folder that holds a
/// stream download in progress (for example `video.mp4.wcstream`).
pub const DIR_SUFFIX: &str = ".wcstream";

const HEADER: &str = "stream.txt";
const LOG: &str = "done.log";

pub(crate) struct WorkDir {
    dir: PathBuf,
    /// `None` once the folder is removed.
    log: Mutex<Option<File>>,
    /// Finished segments and their sizes, by `(track, index)`.
    pub done: HashMap<(usize, usize), u64>,
    /// Joined tracks and their sizes.
    pub joined: HashMap<usize, u64>,
}

impl WorkDir {
    /// Opens (or starts) the folder for `output`. `pieces` holds the number of
    /// pieces of each track. Blocks on the file system.
    pub fn open(output: &Path, fingerprint: u64, pieces: &[usize]) -> io::Result<Self> {
        let dir = path_for(output);
        let header = format!("wirecatch-stream 1 {fingerprint:016x}\n");
        let reuse = fs::read_to_string(dir.join(HEADER)).is_ok_and(|h| h == header);
        if !reuse {
            match fs::remove_dir_all(&dir) {
                Err(e) if e.kind() != io::ErrorKind::NotFound => return Err(e),
                _ => {}
            }
        }
        for track in 0..pieces.len() {
            fs::create_dir_all(dir.join(format!("t{track}")))?;
        }
        if !reuse {
            fs::write(dir.join(HEADER), header)?;
        }

        let mut done = HashMap::new();
        let mut joined = HashMap::new();
        if reuse && let Ok(text) = fs::read_to_string(dir.join(LOG)) {
            for line in text.lines() {
                let fields: Vec<&str> = line.split(' ').collect();
                let number = |i: usize| fields.get(i).and_then(|f| f.parse::<u64>().ok());
                match (fields.first(), number(1), number(2), number(3)) {
                    (Some(&"S"), Some(track), Some(index), Some(size)) if fields.len() == 4 => {
                        done.insert((track as usize, index as usize), size);
                    }
                    (Some(&"J"), Some(track), Some(size), None) if fields.len() == 3 => {
                        joined.insert(track as usize, size);
                    }
                    // A line cut short by a crash, or garbage: ignored.
                    _ => {}
                }
            }
        }
        let mut work = Self {
            log: Mutex::new(Some(
                OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(dir.join(LOG))?,
            )),
            dir,
            done,
            joined,
        };
        let size_is = |path: PathBuf, size: u64| fs::metadata(path).is_ok_and(|m| m.len() == size);
        let done = std::mem::take(&mut work.done)
            .into_iter()
            .filter(|((track, index), size)| {
                pieces.get(*track).is_some_and(|n| index < n)
                    && size_is(work.segment_path(*track, *index), *size)
            })
            .collect();
        let joined = std::mem::take(&mut work.joined)
            .into_iter()
            .filter(|(track, size)| {
                *track < pieces.len() && size_is(work.track_path(*track), *size)
            })
            .collect();
        work.done = done;
        work.joined = joined;
        Ok(work)
    }

    pub fn segment_path(&self, track: usize, index: usize) -> PathBuf {
        self.dir
            .join(format!("t{track}"))
            .join(format!("{index:06}.seg"))
    }

    pub fn part_path(&self, track: usize, index: usize) -> PathBuf {
        self.dir
            .join(format!("t{track}"))
            .join(format!("{index:06}.part"))
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
        self.append(&format!("S {track} {index} {size}\n"))
    }

    /// Notes a joined track. Blocks on the file system.
    pub fn record_joined(&self, track: usize, size: u64) -> io::Result<()> {
        self.append(&format!("J {track} {size}\n"))
    }

    fn append(&self, line: &str) -> io::Result<()> {
        let mut log = self.log.lock().unwrap_or_else(|p| p.into_inner());
        match log.as_mut() {
            Some(log) => log.write_all(line.as_bytes()),
            None => Err(io::Error::other(
                "the download's temporary folder was already removed",
            )),
        }
    }

    /// Removes the segments of one track once they are joined.
    pub fn remove_segments(&self, track: usize) -> io::Result<()> {
        remove_dir(&self.dir.join(format!("t{track}")))
    }

    /// Removes the whole folder. Blocks on the file system.
    pub fn remove(&self) -> io::Result<()> {
        // The log must be closed first: Windows cannot delete an open file.
        self.log.lock().unwrap_or_else(|p| p.into_inner()).take();
        remove_dir(&self.dir)
    }
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

    fn output(test: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("wirecatch-workdir-{test}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir.join("video.mp4")
    }

    #[test]
    fn finished_segments_survive_only_with_the_right_size() {
        let out = output("sizes");
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
        work.append("S 0 garbage\nJ").unwrap(); // a torn line
        drop(work);

        let work = WorkDir::open(&out, 1, &[3, 1]).unwrap();
        let mut done: Vec<_> = work.done.keys().copied().collect();
        done.sort();
        assert_eq!(done, [(0, 0), (1, 0)]);
        drop(work);

        // Another plan clears the folder.
        let work = WorkDir::open(&out, 2, &[3, 1]).unwrap();
        assert!(work.done.is_empty());
        assert!(!work.segment_path(0, 0).exists());
        work.remove().unwrap();
        assert!(!path_for(&out).exists());
    }
}
