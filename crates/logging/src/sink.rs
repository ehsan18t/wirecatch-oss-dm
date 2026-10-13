//! Where a binary's log lines go: stderr, and its log file once the folder is known. Every line is passed through [`redact::text_for_log`] first, whatever logged it.

use std::io::{self, Write as _};
use std::ops::{Deref, DerefMut};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use tracing_subscriber::fmt::MakeWriter;

use crate::file::RollingFile;
use crate::{EARLY_LIMIT, FILE_LIMIT, OLD_FILES_KEPT, redact};

/// The log lines of one binary.
#[derive(Debug)]
pub(crate) struct Sink {
    /// The log file's name, without `.log`.
    name: &'static str,
    /// Whether lines go to stderr as well.
    stderr: bool,
    file: Mutex<FileState>,
    /// The thread holding `file` ([`this_thread`]), or 0: a line that thread logs meanwhile, as a panic report does when the panic happens with the file held, goes to stderr only instead of waiting on itself.
    holder: AtomicUsize,
    /// The log file failed once, and stderr was told.
    failed: AtomicBool,
}

/// The log file's state, held by the thread that has it.
pub(crate) struct Held<'a> {
    sink: &'a Sink,
    state: MutexGuard<'a, FileState>,
}

impl Deref for Held<'_> {
    type Target = FileState;

    fn deref(&self) -> &FileState {
        &self.state
    }
}

impl DerefMut for Held<'_> {
    fn deref_mut(&mut self) -> &mut FileState {
        &mut self.state
    }
}

impl Drop for Held<'_> {
    fn drop(&mut self) {
        self.sink.holder.store(0, Ordering::Release);
    }
}

/// A number for the running thread, never 0 and never the same as another running thread's: where its own thread-local lives.
fn this_thread() -> usize {
    thread_local! {
        static MARK: u8 = const { 0 };
    }
    MARK.with(|mark| std::ptr::from_ref(mark).addr())
}

/// The log file's state.
#[derive(Debug)]
pub(crate) enum FileState {
    /// Its folder is not known yet: lines are held, up to [`EARLY_LIMIT`] bytes, and counted past it.
    Early {
        held: Vec<u8>,
        dropped: usize,
    },
    /// Its folder is known, and the file is made there with the first line, so a run that logs nothing makes no folder.
    Waiting(PathBuf),
    Open(RollingFile),
    /// There is no log file; only stderr gets lines.
    None,
}

impl Sink {
    pub(crate) fn new(name: &'static str, stderr: bool) -> Self {
        Self {
            name,
            stderr,
            file: Mutex::new(FileState::Early {
                held: Vec::new(),
                dropped: 0,
            }),
            holder: AtomicUsize::new(0),
            failed: AtomicBool::new(false),
        }
    }

    pub(crate) fn file_state(&self) -> Held<'_> {
        let state = self.file.lock().unwrap_or_else(PoisonError::into_inner);
        self.holder.store(this_thread(), Ordering::Release);
        Held { sink: self, state }
    }

    /// Writes the log file in `folder` from now on: at once when lines were held until now, which come first, otherwise with the first line. On failure no file is written, and nothing more is held.
    pub(crate) fn write_to(&self, folder: &Path) -> io::Result<()> {
        let mut file = self.file_state();
        let early = std::mem::replace(&mut *file, FileState::None);
        *file = match early {
            FileState::Early { held, dropped } if !held.is_empty() || dropped > 0 => {
                let mut log = RollingFile::open(folder, self.name, FILE_LIMIT, OLD_FILES_KEPT)?;
                log.write_line(&held)?;
                if dropped > 0 {
                    let note = format!(
                        "{dropped} lines logged while starting were dropped: there were too many to hold\n"
                    );
                    log.write_line(note.as_bytes())?;
                }
                FileState::Open(log)
            }
            _ => FileState::Waiting(folder.to_path_buf()),
        };
        Ok(())
    }

    /// Stops holding lines for a log file that will not be written.
    pub(crate) fn without_file(&self) {
        *self.file_state() = FileState::None;
    }

    /// Writes one log line (already redacted) to stderr and to the file.
    pub(crate) fn write(&self, line: &[u8]) {
        if self.stderr {
            // Stderr is gone in a Windows app without a console, and a browser may close the
            // bridge's: the file is where the line is kept, so a failure here changes nothing.
            let (Ok(()) | Err(_)) = io::stderr().write_all(line);
        }
        if self.holder.load(Ordering::Acquire) == this_thread() {
            return;
        }
        let mut file = self.file_state();
        let mut opened = Ok(());
        if let FileState::Waiting(folder) = &*file {
            *file = match RollingFile::open(folder, self.name, FILE_LIMIT, OLD_FILES_KEPT) {
                Ok(log) => FileState::Open(log),
                Err(e) => {
                    opened = Err(e);
                    FileState::None
                }
            };
        }
        let written = opened.and_then(|()| match &mut *file {
            FileState::Early { held, dropped } => {
                if held.len() + line.len() <= EARLY_LIMIT {
                    held.extend_from_slice(line);
                } else {
                    *dropped += 1;
                }
                Ok(())
            }
            FileState::Open(log) => log.write_line(line),
            FileState::Waiting(_) | FileState::None => Ok(()),
        });
        drop(file);
        if let Err(e) = written
            && !self.failed.swap(true, Ordering::Relaxed)
        {
            let note = format!(
                "{}: could not write the log file, so log lines are lost: {e}\n",
                self.name
            );
            let (Ok(()) | Err(_)) = io::stderr().write_all(note.as_bytes());
        }
    }
}

/// Makes a [`LineWriter`] for each event, for the `tracing` formatter.
#[derive(Debug, Clone)]
pub(crate) struct MakeSink(pub(crate) Arc<Sink>);

impl<'a> MakeWriter<'a> for MakeSink {
    type Writer = LineWriter<'a>;

    fn make_writer(&'a self) -> Self::Writer {
        LineWriter {
            sink: &self.0,
            text: Vec::new(),
        }
    }
}

/// One event's text: collected whole, then redacted and written when the formatter is done with it,
/// so a secret split over several writes is still seen whole.
#[derive(Debug)]
pub(crate) struct LineWriter<'a> {
    sink: &'a Sink,
    text: Vec<u8>,
}

impl io::Write for LineWriter<'_> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.text.extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl Drop for LineWriter<'_> {
    fn drop(&mut self) {
        if self.text.is_empty() {
            return;
        }
        let clean = redact::text_for_log(&String::from_utf8_lossy(&self.text));
        self.sink.write(clean.as_bytes());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::panic::{AssertUnwindSafe, catch_unwind};
    use std::sync::mpsc;
    use wirecatch_test_support::PATIENCE;

    /// Logs a line when dropped, as code does while a panic unwinds through it.
    struct LogsWhenDropped<'a>(&'a Sink);

    impl Drop for LogsWhenDropped<'_> {
        fn drop(&mut self) {
            self.0.write(b"unwinding\n");
        }
    }

    #[test]
    fn lines_past_what_is_held_while_starting_are_counted_in_the_file() {
        let folder = tempfile::tempdir().unwrap();
        let sink = Sink::new("test", false);
        let line = format!("{}\n", "x".repeat(1023));
        let fit = EARLY_LIMIT / line.len();
        for _ in 0..fit + 5 {
            sink.write(line.as_bytes());
        }
        sink.write_to(folder.path()).unwrap();
        let text = std::fs::read_to_string(folder.path().join("test.log")).unwrap();
        assert_eq!(
            text.lines().filter(|kept| *kept == line.trim_end()).count(),
            fit
        );
        assert!(
            text.ends_with(
                "5 lines logged while starting were dropped: there were too many to hold\n"
            ),
            "{}",
            &text[text.len() - 200..]
        );
    }

    /// A panic in code that holds the log file (the panic report logs through the same sink) would otherwise wait on itself forever.
    #[test]
    fn logging_while_a_panic_unwinds_past_the_held_file_does_not_hang() {
        let (done, finished) = mpsc::channel();
        let worker = std::thread::spawn(move || {
            let sink = Sink::new("test", false);
            let held = sink.file_state();
            let unwound = catch_unwind(AssertUnwindSafe(|| {
                let _logs = LogsWhenDropped(&sink);
                std::panic::panic_any("a fault while the log file is held");
            }));
            drop(held);
            done.send(unwound.is_err())
        });
        assert_eq!(finished.recv_timeout(PATIENCE), Ok(true));
        assert!(worker.join().is_ok_and(|sent| sent.is_ok()));
    }
}
