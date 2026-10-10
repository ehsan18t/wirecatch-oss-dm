//! Joining tracks into one file with ffmpeg, by stream copy only.
//!
//! ffmpeg is started with an argument vector, never through a shell, so no
//! character in a path has a special meaning. Every path is absolute and
//! passed as `file:<path>`, so ffmpeg reads it as a plain file even when the
//! name starts with `-` or contains `:` or `|` (which ffmpeg would otherwise
//! take for an option or a protocol such as `concat:`).

use std::ffi::OsString;
use std::path::Path;
use std::process::Stdio;

use tokio::io::AsyncReadExt;
use tokio_util::sync::CancellationToken;

use crate::error::{Error, Result};
use crate::plan::Role;

/// Most of ffmpeg's error output kept for the error message.
const STDERR_KEPT: usize = 16 * 1024;

/// The container ffmpeg writes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Container {
    Mp4,
    Matroska,
}

impl Container {
    /// The file extension, `audio_only` choosing the audio flavor.
    pub fn extension(self, audio_only: bool) -> &'static str {
        match (self, audio_only) {
            (Container::Mp4, false) => "mp4",
            (Container::Mp4, true) => "m4a",
            (Container::Matroska, false) => "mkv",
            (Container::Matroska, true) => "mka",
        }
    }

    fn format(self) -> &'static str {
        match self {
            Container::Mp4 => "mp4",
            Container::Matroska => "matroska",
        }
    }
}

/// ffmpeg's arguments to copy the video and audio streams of `inputs` (in
/// order, each with its role) into `output`. Paths must be absolute.
pub(crate) fn arguments(
    inputs: &[(&Path, Role)],
    output: &Path,
    container: Container,
) -> Vec<OsString> {
    let mut args: Vec<OsString> = ["-hide_banner", "-nostdin", "-loglevel", "error", "-y"]
        .into_iter()
        .map(OsString::from)
        .collect();
    for (path, _) in inputs {
        args.push("-i".into());
        args.push(file_argument(path));
    }
    let separate_audio = inputs.iter().any(|(_, role)| *role == Role::Audio);
    for (index, (_, role)) in inputs.iter().enumerate() {
        // Only video and audio: streams such as timed ID3 metadata in MPEG-TS
        // cannot go into MP4 and would make the copy fail. `?` skips a kind
        // that an input does not have.
        if *role == Role::Video {
            args.push("-map".into());
            args.push(format!("{index}:v?").into());
        }
        if *role == Role::Audio || !separate_audio {
            args.push("-map".into());
            args.push(format!("{index}:a?").into());
        }
    }
    for arg in ["-c", "copy", "-f", container.format()] {
        args.push(arg.into());
    }
    args.push(file_argument(output));
    args
}

/// `file:<path>`: read and written as a plain file, whatever the name.
fn file_argument(path: &Path) -> OsString {
    debug_assert!(path.is_absolute(), "ffmpeg paths must be absolute");
    let mut arg = OsString::from("file:");
    arg.push(path.as_os_str());
    arg
}

/// Runs `program` with `args`. Returns `Ok(false)` when cancelled (ffmpeg is
/// stopped), and an error with what ffmpeg reported when it fails.
pub(crate) async fn run(
    program: &Path,
    args: &[OsString],
    cancel: &CancellationToken,
) -> Result<bool> {
    let mut command = tokio::process::Command::new(program);
    command
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    #[cfg(windows)]
    {
        // No console window flashing up from a desktop app.
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Err(Error::FfmpegMissing(program.to_path_buf()));
        }
        Err(e) => return Err(Error::Ffmpeg(format!("it could not be started ({e})"))),
    };
    let mut stderr = child.stderr.take();
    let finished = {
        let read = async {
            let mut kept = Vec::new();
            if let Some(stderr) = stderr.as_mut() {
                let mut chunk = [0u8; 4096];
                // Read to the end so ffmpeg never blocks on a full pipe, but
                // keep only the last part.
                while let Ok(n) = stderr.read(&mut chunk).await {
                    if n == 0 {
                        break;
                    }
                    kept.extend_from_slice(&chunk[..n]);
                    if kept.len() > STDERR_KEPT {
                        kept.drain(..kept.len() - STDERR_KEPT);
                    }
                }
            }
            kept
        };
        let both = async { tokio::join!(child.wait(), read) };
        tokio::select! {
            biased;
            _ = cancel.cancelled() => None,
            done = both => Some(done),
        }
    };
    let Some((status, output)) = finished else {
        let _ = child.kill().await;
        return Ok(false);
    };
    let status = status.map_err(|e| Error::Ffmpeg(format!("waiting for it failed ({e})")))?;
    if status.success() {
        return Ok(true);
    }
    Err(Error::Ffmpeg(summary(&output, status.code())))
}

/// The last few lines ffmpeg printed, or its exit code if it printed nothing.
fn summary(output: &[u8], code: Option<i32>) -> String {
    let text = String::from_utf8_lossy(output);
    let lines: Vec<&str> = text
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect();
    let tail = lines[lines.len().saturating_sub(3)..].join(" / ");
    if tail.is_empty() {
        match code {
            Some(code) => format!("it stopped with exit code {code}"),
            None => "it was stopped".into(),
        }
    } else {
        tail
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn absolute(name: &str) -> PathBuf {
        std::env::temp_dir().join(name)
    }

    #[test]
    fn paths_are_single_file_arguments_whatever_they_contain() {
        let hostile = [
            "-y evil.mp4",
            "name with spaces.ts",
            "quote\"d 'name'.ts",
            "concat:a.ts|b.ts",
            "semi;colon & ampersand $HOME `x`.ts",
            "-i",
        ];
        for name in hostile {
            let video = absolute(name);
            let audio = absolute(&format!("audio {name}"));
            let out = absolute(&format!("out {name}"));
            let args = arguments(
                &[(&video, Role::Video), (&audio, Role::Audio)],
                &out,
                Container::Mp4,
            );
            let as_text: Vec<String> = args
                .iter()
                .map(|a| a.to_string_lossy().into_owned())
                .collect();
            // Each path is exactly one argument, prefixed so it can never be read
            // as an option or a protocol.
            for path in [&video, &audio, &out] {
                let expected = format!("file:{}", path.display());
                assert_eq!(
                    as_text.iter().filter(|a| **a == expected).count(),
                    1,
                    "{expected}"
                );
            }
            // Nothing else in the vector came from the paths.
            let fixed: Vec<&str> = as_text
                .iter()
                .filter(|a| !a.starts_with("file:"))
                .map(String::as_str)
                .collect();
            assert_eq!(
                fixed,
                [
                    "-hide_banner",
                    "-nostdin",
                    "-loglevel",
                    "error",
                    "-y",
                    "-i",
                    "-i",
                    "-map",
                    "0:v?",
                    "-map",
                    "1:a?",
                    "-c",
                    "copy",
                    "-f",
                    "mp4"
                ]
            );
            assert!(as_text.last().unwrap().starts_with("file:"));
        }
    }

    #[test]
    fn one_input_keeps_its_own_audio() {
        let video = absolute("v.ts");
        let out = absolute("o.mkv");
        let args = arguments(&[(&video, Role::Video)], &out, Container::Matroska);
        let text: Vec<_> = args
            .iter()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        let maps: Vec<_> = text
            .windows(2)
            .filter(|w| w[0] == "-map")
            .map(|w| w[1].clone())
            .collect();
        assert_eq!(maps, ["0:v?", "0:a?"]);
        assert!(text.contains(&"matroska".to_string()));
        assert!(
            !text.iter().any(|a| a.contains("-c:v") || a == "libx264"),
            "never re-encodes"
        );
    }

    #[test]
    fn summaries_keep_the_last_lines() {
        assert_eq!(summary(b"a\n\nb\nc\nd\n", Some(1)), "b / c / d");
        assert_eq!(summary(b"", Some(3)), "it stopped with exit code 3");
    }
}
