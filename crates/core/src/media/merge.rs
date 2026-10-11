//! Joining separately downloaded video and audio into one file with the media
//! add-on's ffmpeg: streams are copied, never re-encoded.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use wirecatch_engine::CancellationToken;
use wirecatch_protocol::app::{FixAction, UserError};

use super::process::{self, RunError};
use crate::addons::MediaTools;

/// How a join ended without failing.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Joined {
    Done,
    Cancelled,
}

/// ffmpeg's arguments to copy the first video of `video` and the first audio of
/// `audio` into `output`. Paths go in as absolute `file:` paths, so a name that
/// starts with `-` or contains `:` is never read as an option or a protocol.
pub(crate) fn arguments(video: &Path, audio: &Path, output: &Path) -> Vec<OsString> {
    let mut args: Vec<OsString> = ["-hide_banner", "-nostdin", "-loglevel", "error", "-y"]
        .into_iter()
        .map(OsString::from)
        .collect();
    for input in [video, audio] {
        args.push("-i".into());
        args.push(file_arg(input));
    }
    for arg in ["-map", "0:v:0", "-map", "1:a:0", "-c", "copy"] {
        args.push(arg.into());
    }
    let ext = output
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase);
    if matches!(ext.as_deref(), Some("mp4" | "m4a" | "mov")) {
        args.push("-movflags".into());
        args.push("+faststart".into());
    }
    args.push(file_arg(output));
    args
}

/// The folder ffmpeg runs in: the one it writes into.
fn work_dir(output: &Path) -> &Path {
    output.parent().unwrap_or(Path::new("."))
}

fn file_arg(path: &Path) -> OsString {
    let absolute: PathBuf = std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf());
    let mut arg = OsString::from("file:");
    arg.push(absolute.as_os_str());
    arg
}

/// Joins `video` and `audio` into `output` with `ffmpeg`.
pub(crate) async fn join(
    tools: &MediaTools,
    video: &Path,
    audio: &Path,
    output: &Path,
    cancel: &CancellationToken,
) -> Result<Joined, UserError> {
    let args = arguments(video, audio, output);
    match process::run(
        &tools.ffmpeg,
        &args,
        work_dir(output),
        false,
        None,
        cancel,
        |_| {},
    )
    .await
    {
        Ok(ran) if ran.success && output.is_file() => Ok(Joined::Done),
        Ok(ran) => Err(UserError {
            message: format!(
                "ffmpeg could not join the video and audio ({}). Try again; the downloaded parts are kept.",
                ran.error_line()
                    .unwrap_or("no reason given")
                    .trim_end_matches('.')
            ),
            fix: Some(FixAction::Retry),
        }),
        Err(RunError::Cancelled) => Ok(Joined::Cancelled),
        Err(RunError::TimedOut) => Err(UserError {
            message: "ffmpeg took too long to join the video and audio. Try again.".into(),
            fix: Some(FixAction::Retry),
        }),
        Err(RunError::Start(e)) => Err({
            tools.mark_broken();
            UserError {
                message: format!(
                    "The media add-on's ffmpeg could not be started ({e}). Reinstall the media add-on in Settings, Add-ons, then try again."
                ),
                fix: Some(FixAction::Retry),
            }
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_are_single_file_arguments() {
        let args = arguments(
            Path::new("/v/-video.mp4"),
            Path::new("/v/a:b.m4a"),
            Path::new("/v/out.mp4"),
        );
        let text: Vec<String> = args
            .iter()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        let inputs: Vec<&String> = text
            .windows(2)
            .filter(|w| w[0] == "-i")
            .map(|w| &w[1])
            .collect();
        assert_eq!(inputs.len(), 2);
        assert!(inputs.iter().all(|i| i.starts_with("file:")));
        assert!(text.contains(&"copy".to_string()));
        assert!(text.last().unwrap().starts_with("file:"));
        assert!(text.contains(&"+faststart".to_string()));
    }
}
