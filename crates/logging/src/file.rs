//! A log file capped in size: past its limit it becomes the first old file, the old files move up one, and the oldest is deleted.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Write as _};
use std::path::{Path, PathBuf};

/// After a failed move on, how many more bytes are written, as a share of the limit, before the next try: one failure costs one try per that many bytes, not one per line.
const RETRY_PARTS: u64 = 16;

/// How many lines a writer writes, while its file is under half its limit, before it looks again whether another writer moved the file on: two file system calls a line would be too many at the trace level, and a few lines landing in the file just moved on are not lost.
const CHECK_EVERY: u32 = 16;

/// A log file `<name>.log` in a folder, with up to `kept` old files beside it (`<name>.1.log`, the newest, to `<name>.<kept>.log`), none much past `limit` bytes while the files can be moved on, and none past twice that when they cannot.
///
/// Other processes of the same binary may write the same file (a bridge for each browser, command-line runs side by side): appends never overwrite their lines, every writer counts the bytes the file holds rather than its own, and a writer whose file another one moved on follows to the new one within a few lines.
#[derive(Debug)]
pub(crate) struct RollingFile {
    path: PathBuf,
    limit: u64,
    kept: usize,
    file: File,
    written: u64,
    /// After a failed move on, the size from which it is tried again.
    retry_from: u64,
    /// Lines written since the path was last looked at.
    unchecked: u32,
}

impl RollingFile {
    /// Opens `<folder>/<name>.log` to append to, making the folder when it is missing: on Unix readable by its owner only, like the files, since the folder above may be the app's data folder, which the bridge or the command-line downloader can make before the app does.
    pub(crate) fn open(folder: &Path, name: &str, limit: u64, kept: usize) -> io::Result<Self> {
        let mut folders = fs::DirBuilder::new();
        folders.recursive(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt as _;
            folders.mode(0o700);
        }
        folders.create(folder)?;
        let path = folder.join(format!("{name}.log"));
        let (file, written) = append_to(&path)?;
        Ok(Self {
            path,
            limit,
            kept,
            file,
            written,
            retry_from: 0,
            unchecked: 0,
        })
    }

    /// Writes `line` whole, moving the files on first when it would take the file past its limit. A file that cannot be moved on (a reader holds it without delete sharing) keeps its lines, and the move is tried again after some more; past twice the limit it is emptied instead, with a line saying so, so the folder never grows past its cap. Whatever happens to the files, the line is written to the file held.
    pub(crate) fn write_line(&mut self, line: &[u8]) -> io::Result<()> {
        if self.unchecked >= CHECK_EVERY || self.written >= self.limit / 2 {
            self.follow_path();
            self.unchecked = 0;
        }
        self.unchecked = self.unchecked.saturating_add(1);
        let len = u64::try_from(line.len()).unwrap_or(u64::MAX);
        let past = self.written.saturating_add(len);
        if self.written > 0 && past > self.limit {
            let moved_on = self.written >= self.retry_from && self.move_on_or_retry_later();
            if !moved_on && past > self.limit.saturating_mul(2) {
                self.drop_older_lines()?;
            }
        }
        self.file.write_all(line)?;
        self.written = self.written.saturating_add(len);
        Ok(())
    }

    /// Makes sure the file held is the one at the path, which another writer may have moved on, and counts what it holds, other writers' lines included. A file that cannot be reopened stays held, and the next line tries again.
    fn follow_path(&mut self) {
        let held = self.file.metadata().map(|held| held.len());
        let there = fs::metadata(&self.path).map(|there| there.len());
        match (held, there) {
            (Ok(held), Ok(there)) if held == there => self.written = held,
            _ => {
                if let Ok((file, written)) = append_to(&self.path) {
                    self.file = file;
                    self.written = written;
                }
            }
        }
    }

    /// Moves the files on; when that fails, it is tried again only once some more bytes are written.
    fn move_on_or_retry_later(&mut self) -> bool {
        if self.roll().is_ok() {
            self.retry_from = 0;
            true
        } else {
            self.retry_from = self.written.saturating_add(self.limit / RETRY_PARTS);
            false
        }
    }

    /// Empties the file, which could not be moved on, and says so in it. When even that fails, the file held keeps growing and the next line tries again.
    fn drop_older_lines(&mut self) -> io::Result<()> {
        if empty(&self.path).is_err() {
            return Ok(());
        }
        let note = "the older lines of this log were dropped: the log file could not be moved on, as another program holds it open\n";
        self.file.write_all(note.as_bytes())?;
        self.written = u64::try_from(note.len()).unwrap_or(u64::MAX);
        self.retry_from = 0;
        Ok(())
    }

    /// Deletes the oldest file and moves every other one up, the file being moved on (0) to the first.
    fn move_old_files_up(&self) -> io::Result<()> {
        remove_if_there(&self.old(self.kept))?;
        for n in (0..self.kept).rev() {
            rename_if_there(&self.old(n), &self.old(n + 1))?;
        }
        Ok(())
    }

    /// The `n`th old file, `n` from 1, or the file being moved on for 0.
    fn old(&self, n: usize) -> PathBuf {
        self.path.with_extension(format!("{n}.log"))
    }

    /// Deletes the oldest file, moves every other one up, and starts the file anew. The file is moved aside first, so when it cannot be moved (another program holds it) the old files stay as they are.
    fn roll(&mut self) -> io::Result<()> {
        if self.kept == 0 {
            empty(&self.path)?;
        } else {
            // Renamed while still open: files are opened with delete sharing on Windows, which allows it, and on Unix the open handle follows the file.
            // A file a move cut short left aside goes up with the others rather than be overwritten.
            if self.old(0).exists() {
                self.move_old_files_up()?;
            }
            fs::rename(&self.path, self.old(0))?;
            if let Err(e) = self.move_old_files_up() {
                // Back where it was, so its lines are not overwritten by the next move; should that fail too, the next line starts a new file and this one waits as the file being moved on.
                if fs::rename(self.old(0), &self.path).is_ok() {
                    return Err(e);
                }
            }
            (self.file, _) = append_to(&self.path)?;
        }
        self.written = 0;
        Ok(())
    }
}

/// Empties the file at `path`, through a handle of its own: a handle that appends cannot change the length on Windows.
fn empty(path: &Path) -> io::Result<()> {
    OpenOptions::new().write(true).open(path)?.set_len(0)
}

fn remove_if_there(path: &Path) -> io::Result<()> {
    match fs::remove_file(path) {
        Err(e) if e.kind() != io::ErrorKind::NotFound => Err(e),
        _ => Ok(()),
    }
}

fn rename_if_there(from: &Path, to: &Path) -> io::Result<()> {
    match fs::rename(from, to) {
        Err(e) if e.kind() != io::ErrorKind::NotFound => Err(e),
        _ => Ok(()),
    }
}

/// `path` opened to append to, created when missing, with the bytes it holds; on Unix readable by its owner only, since a log names the files and sites a person downloads from.
fn append_to(path: &Path) -> io::Result<(File, u64)> {
    let mut options = OpenOptions::new();
    options.create(true).append(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        options.mode(0o600);
    }
    let file = options.open(path)?;
    let len = file.metadata()?.len();
    Ok((file, len))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn files_in(folder: &Path) -> Vec<(String, u64)> {
        let mut files: Vec<_> = fs::read_dir(folder)
            .unwrap()
            .map(|entry| {
                let entry = entry.unwrap();
                (
                    entry.file_name().to_string_lossy().into_owned(),
                    entry.metadata().unwrap().len(),
                )
            })
            .collect();
        files.sort();
        files
    }

    #[test]
    fn the_file_moves_on_at_its_limit_and_only_the_newest_old_files_are_kept() {
        let folder = tempfile::tempdir().unwrap();
        let mut log = RollingFile::open(folder.path(), "app", 100, 2).unwrap();
        for n in 0..20 {
            log.write_line(format!("line {n:02} of the log........\n").as_bytes())
                .unwrap();
        }
        let files = files_in(folder.path());
        let names: Vec<&str> = files.iter().map(|(name, _)| name.as_str()).collect();
        assert_eq!(names, ["app.1.log", "app.2.log", "app.log"]);
        assert!(files.iter().all(|(_, len)| *len <= 100), "{files:?}");
        let newest = fs::read_to_string(folder.path().join("app.log")).unwrap();
        assert!(newest.ends_with("line 19 of the log........\n"), "{newest}");
        let older = fs::read_to_string(folder.path().join("app.1.log")).unwrap();
        assert!(older.contains("line 17"), "{older}");
    }

    #[test]
    fn an_existing_file_is_appended_to_and_counts_toward_the_limit() {
        let folder = tempfile::tempdir().unwrap();
        fs::write(folder.path().join("app.log"), "x".repeat(90)).unwrap();
        let mut log = RollingFile::open(folder.path(), "app", 100, 1).unwrap();
        log.write_line(b"a line longer than ten bytes\n").unwrap();
        assert_eq!(
            fs::read_to_string(folder.path().join("app.1.log")).unwrap(),
            "x".repeat(90)
        );
        assert_eq!(
            fs::read_to_string(folder.path().join("app.log")).unwrap(),
            "a line longer than ten bytes\n"
        );
    }

    #[test]
    fn a_file_that_cannot_move_on_is_emptied_rather_than_growing() {
        let folder = tempfile::tempdir().unwrap();
        // A folder where the old file goes cannot be replaced by a file.
        fs::create_dir(folder.path().join("app.1.log")).unwrap();
        fs::write(folder.path().join("app.1.log").join("keep"), "x").unwrap();
        let mut log = RollingFile::open(folder.path(), "app", 200, 1).unwrap();
        for _ in 0..50 {
            log.write_line(b"0123456789012345678901234567890123456789\n")
                .unwrap();
        }
        let text = fs::read_to_string(folder.path().join("app.log")).unwrap();
        assert!(text.len() <= 400, "{} bytes", text.len());
        assert!(
            text.starts_with("the older lines of this log were dropped"),
            "{text}"
        );
    }

    /// The lines of every log file in `folder`, of all its writers.
    fn lines_in(folder: &Path) -> Vec<String> {
        let mut lines = Vec::new();
        for entry in fs::read_dir(folder).unwrap() {
            let text = fs::read_to_string(entry.unwrap().path()).unwrap();
            lines.extend(text.lines().map(str::to_string));
        }
        lines
    }

    /// Two bridges (one per browser), or two command-line runs, write one file: when one moves the files on, the other follows to the new file instead of writing into an old one about to be deleted.
    #[test]
    fn two_writers_of_one_file_lose_no_line_when_one_moves_it_on() {
        let folder = tempfile::tempdir().unwrap();
        let mut first = RollingFile::open(folder.path(), "app", 1000, 3).unwrap();
        let mut second = RollingFile::open(folder.path(), "app", 1000, 3).unwrap();
        // 80 lines of 40 bytes: the four files can hold them all.
        for n in 0..40 {
            first
                .write_line(format!("first  writer, line {n:02} of 40........\n").as_bytes())
                .unwrap();
            second
                .write_line(format!("second writer, line {n:02} of 40........\n").as_bytes())
                .unwrap();
        }
        let lines = lines_in(folder.path());
        for n in 0..40 {
            for writer in ["first  writer", "second writer"] {
                let line = format!("{writer}, line {n:02} of 40........");
                assert!(
                    lines.contains(&line),
                    "{line:?} was lost: {} lines kept",
                    lines.len()
                );
            }
        }
        // Past half its limit a writer looks at the path before every line, so once a file is near its limit, at most the line each writer had started lands in it after another writer moved it on.
        let slack = 40;
        assert!(
            files_in(folder.path())
                .iter()
                .all(|(_, len)| *len <= 1000 + slack),
            "{:?}",
            files_in(folder.path())
        );
    }

    /// A writer that logs little (the bridge of a browser left idle) follows the file another writer moved on within a few lines, though its own lines are far from half the limit.
    #[test]
    fn a_quiet_writer_follows_the_file_a_busy_one_moved_on() {
        let folder = tempfile::tempdir().unwrap();
        let mut quiet = RollingFile::open(folder.path(), "app", 10_000, 3).unwrap();
        let mut busy = RollingFile::open(folder.path(), "app", 10_000, 3).unwrap();
        quiet
            .write_line(b"quiet writer, line 00..................\n")
            .unwrap();
        // 300 lines of 40 bytes move the file on once.
        for n in 0..300 {
            busy.write_line(format!("busy writer, line {n:03}..................\n").as_bytes())
                .unwrap();
        }
        assert!(folder.path().join("app.1.log").is_file());
        let last = CHECK_EVERY + 4;
        for n in 1..=last {
            quiet
                .write_line(format!("quiet writer, line {n:02}..................\n").as_bytes())
                .unwrap();
        }
        let newest = fs::read_to_string(folder.path().join("app.log")).unwrap();
        let line = format!("quiet writer, line {last:02}");
        assert!(
            newest.contains(&line),
            "{line:?} went to an old file: {:?}",
            files_in(folder.path())
        );
    }

    /// Someone watching the log while reproducing a problem holds it open without delete sharing (PowerShell's `Get-Content -Wait`, a .NET reader), so it cannot be moved on: lines are kept until twice the limit, and it moves on once the reader lets go.
    #[cfg(windows)]
    #[test]
    fn a_file_held_by_a_reader_keeps_its_lines_and_moves_on_later() {
        use std::os::windows::fs::OpenOptionsExt as _;
        const FILE_SHARE_READ: u32 = 1;
        const FILE_SHARE_WRITE: u32 = 2;

        let folder = tempfile::tempdir().unwrap();
        let mut log = RollingFile::open(folder.path(), "app", 1000, 2).unwrap();
        let reader = OpenOptions::new()
            .read(true)
            .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
            .open(folder.path().join("app.log"))
            .unwrap();
        for n in 0..30 {
            log.write_line(format!("line {n:02} written while it is watched......\n").as_bytes())
                .unwrap();
        }
        let text = fs::read_to_string(folder.path().join("app.log")).unwrap();
        assert!(text.starts_with("line 00 "), "{text}");
        assert!(text.contains("line 29 "), "{text}");
        // Past twice the limit the file is emptied rather than growing on.
        for _ in 0..30 {
            log.write_line(b"a line past the limit, while the reader stays..\n")
                .unwrap();
        }
        assert!(fs::metadata(folder.path().join("app.log")).unwrap().len() <= 2000);
        drop(reader);
        for _ in 0..30 {
            log.write_line(b"a line after the reader let go................\n")
                .unwrap();
        }
        assert!(
            folder.path().join("app.1.log").is_file(),
            "{:?}",
            files_in(folder.path())
        );
        assert!(
            files_in(folder.path()).iter().all(|(_, len)| *len <= 2000),
            "{:?}",
            files_in(folder.path())
        );
    }

    /// A file left aside by a move cut short is moved on with the others, not overwritten by the next move.
    #[test]
    fn a_file_left_by_a_move_cut_short_keeps_its_lines() {
        let folder = tempfile::tempdir().unwrap();
        fs::write(folder.path().join("app.0.log"), "a line left aside\n").unwrap();
        let mut log = RollingFile::open(folder.path(), "app", 100, 3).unwrap();
        for n in 0..6 {
            log.write_line(format!("line {n} of the log, long enough..\n").as_bytes())
                .unwrap();
        }
        assert!(
            lines_in(folder.path()).contains(&"a line left aside".to_string()),
            "{:?}",
            files_in(folder.path())
        );
        assert!(!folder.path().join("app.0.log").exists());
    }

    #[test]
    fn a_missing_folder_is_made() {
        let folder = tempfile::tempdir().unwrap();
        let logs = folder.path().join("data").join("logs");
        let mut log = RollingFile::open(&logs, "app", 100, 1).unwrap();
        log.write_line(b"hello\n").unwrap();
        assert_eq!(fs::read_to_string(logs.join("app.log")).unwrap(), "hello\n");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            for made in [folder.path().join("data"), logs] {
                let mode = fs::metadata(&made).unwrap().permissions().mode() & 0o777;
                assert_eq!(
                    mode,
                    0o700,
                    "{} is not private to its owner",
                    made.display()
                );
            }
        }
    }
}
