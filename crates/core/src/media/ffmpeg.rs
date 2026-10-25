//! Running the media add-on's ffmpeg for one output file: the one place that decides whether a run made its file, removes what an unfinished run left, and names why it failed. The arguments come from [`wirecatch_streams::ffmpeg`] and [`output`](super::output); the process from [`wirecatch_streams::process`].

#![forbid(unsafe_code)]

use std::ffi::OsString;
use std::io;
use std::path::{Path, PathBuf};
use std::time::Duration;

use tracing::warn;
use wirecatch_engine::CancellationToken;
use wirecatch_streams::Role;
use wirecatch_streams::codec::VideoCodec;
use wirecatch_streams::process::STALL_TIMEOUT;
use wirecatch_streams::process::{self, Output, RunError, RunOptions};

use crate::media::MediaTools;

/// How an ffmpeg run that did not fail ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Finished {
    /// The output file is complete.
    Done,
    /// Cancelled; nothing of the output is left.
    Cancelled,
}

/// Why ffmpeg did not make its output file.
#[derive(Debug, thiserror::Error)]
pub(crate) enum FfmpegError {
    /// The add-on's ffmpeg could not be started.
    #[error("ffmpeg could not be started")]
    Start(#[source] io::Error),
    /// ffmpeg ran and failed; the reason is its own last error line.
    #[error("{reason}")]
    Failed {
        /// What ffmpeg said.
        reason: String,
    },
    /// ffmpeg reported nothing for this long and was stopped.
    #[error("ffmpeg reported nothing for {} seconds", .0.as_secs())]
    Stalled(Duration),
    /// ffmpeg ran past its time limit and was stopped.
    #[error("ffmpeg took longer than {} seconds", .0.as_secs())]
    TimedOut(Duration),
    /// This ffmpeg has no encoder for the codec.
    #[error("this ffmpeg has no {} encoder", .0.label())]
    NoEncoder(VideoCodec),
    /// A downloaded file could not be read to recognize its format.
    #[error("the downloaded {} could not be read", .role.name())]
    Unreadable {
        /// Which download.
        role: Role,
        /// Why.
        #[source]
        source: io::Error,
    },
    /// Recognizing a downloaded file's format panicked, a bug, or was stopped as the app closed.
    #[error("recognizing the downloaded {} stopped unexpectedly ({error})", .role.name())]
    Task {
        /// Which download.
        role: Role,
        /// How the task ended.
        error: tokio::task::JoinError,
    },
    /// A downloaded file is in no format ffmpeg may read it as, so ffmpeg was not started.
    #[error("the downloaded {} is in no format Wirecatch lets ffmpeg read", .role.name())]
    UnknownFormat {
        /// Which download.
        role: Role,
    },
    /// The output path has no folder to run ffmpeg in.
    #[error("{} is not in a folder", .0.display())]
    NoFolder(PathBuf),
    /// A file's path cannot be given to ffmpeg, so ffmpeg was not started.
    #[error(transparent)]
    Path(#[from] wirecatch_streams::ffmpeg::FileArgumentError),
}

/// The folder ffmpeg runs in: the one it writes into. Output paths are always absolute, inside a download's work folder.
pub(crate) fn ffmpeg_dir(output: &Path) -> Result<&Path, FfmpegError> {
    output
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .ok_or_else(|| FfmpegError::NoFolder(output.to_path_buf()))
}

/// Runs the add-on's ffmpeg with `args` to make `output`, passing each line of its progress report to `on_progress`. It counts as done only when ffmpeg succeeded and the file is there; whenever it ends otherwise, a partial `output` is removed. A run that reports nothing for [`STALL_TIMEOUT`] is stopped; `timeout` limits the whole run when given.
pub(crate) async fn run(
    tools: &MediaTools,
    args: &[OsString],
    output: &Path,
    timeout: Option<Duration>,
    cancel: &CancellationToken,
    mut on_progress: impl FnMut(&str),
) -> Result<Finished, FfmpegError> {
    let dir = ffmpeg_dir(output)?;
    let mut options = RunOptions::lines()
        .with_idle_timeout(STALL_TIMEOUT)
        .watching(output.to_path_buf());
    if let Some(timeout) = timeout {
        options = options.with_timeout(timeout);
    }
    let ran = process::run(&tools.ffmpeg, args, dir, options, cancel, |from, line| {
        if from == Output::Stdout {
            on_progress(line);
        }
    })
    .await;
    let result = match ran {
        Ok(ran) if ran.success() && is_file(output).await => return Ok(Finished::Done),
        Ok(ran) => Err(FfmpegError::Failed {
            reason: ran.reason().trim_end_matches('.').to_string(),
        }),
        Err(RunError::Cancelled) => Ok(Finished::Cancelled),
        Err(RunError::Start { source, .. }) => Err(FfmpegError::Start(source)),
        Err(RunError::Stalled { quiet, .. }) => Err(FfmpegError::Stalled(quiet)),
        Err(RunError::TimedOut { limit, .. }) => Err(FfmpegError::TimedOut(limit)),
        Err(other) => Err(FfmpegError::Failed {
            reason: other.to_string(),
        }),
    };
    remove(output).await;
    result
}

/// Whether `path` is a file, asked off the async threads.
async fn is_file(path: &Path) -> bool {
    tokio::fs::metadata(path).await.is_ok_and(|m| m.is_file())
}

/// Removes an unfinished file; one that is not there is fine, and a failure is logged (it is in the download's work folder, which goes when the download ends).
pub(crate) async fn remove(path: &Path) {
    match tokio::fs::remove_file(path).await {
        Ok(()) => {}
        Err(e) if e.kind() == io::ErrorKind::NotFound => {}
        Err(e) => warn!(path = %path.display(), error = %e, "could not remove an unfinished file"),
    }
}

#[cfg(test)]
pub(crate) mod test_support {
    //! A real ffmpeg for the tests that need one, and a way to make media with it.

    use std::path::{Path, PathBuf};

    /// A real ffmpeg from `WIRECATCH_TEST_FFMPEG`. Without one the test is skipped with a note, unless `WIRECATCH_REQUIRE_FFMPEG=1`.
    pub(crate) fn real_ffmpeg() -> Option<PathBuf> {
        let found = std::env::var_os("WIRECATCH_TEST_FFMPEG")
            .map(PathBuf::from)
            .filter(|p| p.is_file());
        if found.is_none() {
            let required = std::env::var("WIRECATCH_REQUIRE_FFMPEG").is_ok_and(|v| v == "1");
            assert!(
                !required,
                "WIRECATCH_REQUIRE_FFMPEG=1 but WIRECATCH_TEST_FFMPEG is not a file"
            );
            eprintln!("skipped: set WIRECATCH_TEST_FFMPEG to a real ffmpeg to run this test");
        }
        found
    }

    /// Makes `path` with `ffmpeg` and `args`, failing the test if it cannot.
    pub(crate) fn make(ffmpeg: &Path, args: &[&str], path: &Path) {
        let status = std::process::Command::new(ffmpeg)
            .args(["-hide_banner", "-loglevel", "error", "-y"])
            .args(args)
            .arg(path)
            .status()
            .unwrap();
        assert!(status.success(), "could not make {}", path.display());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ffmpeg_runs_in_its_output_folder_and_needs_one() {
        assert_eq!(
            ffmpeg_dir(Path::new("/work/out.mp4")).unwrap(),
            Path::new("/work")
        );
        assert!(matches!(
            ffmpeg_dir(Path::new("out.mp4")),
            Err(FfmpegError::NoFolder(_))
        ));
    }

    #[tokio::test]
    async fn a_missing_ffmpeg_is_a_start_error_and_leaves_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let output = dir.path().join("out.mkv");
        let tools = MediaTools::for_test(&dir.path().join("no-such-ffmpeg"));
        let result = run(
            &tools,
            &[],
            &output,
            None,
            &CancellationToken::new(),
            |_| {},
        )
        .await;
        assert!(matches!(result, Err(FfmpegError::Start(_))), "{result:?}");
        assert!(!output.exists());
    }
}
