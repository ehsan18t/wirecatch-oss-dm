//! Joining separately downloaded video and audio into one file with the media
//! add-on's ffmpeg: streams are copied, never re-encoded.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use wirecatch_engine::CancellationToken;
use wirecatch_protocol::app::{FixAction, UserError};
use wirecatch_streams::{Error as StreamError, InputFormat, ffmpeg_input_args};

use super::process::{self, RunError};
use crate::addons::MediaTools;

/// How a join ended without failing.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Joined {
    Done,
    Cancelled,
}

/// ffmpeg's arguments to copy the first video of `video` and the first audio of
/// `audio` into `output`. Each input is read only as the format it was
/// recognized as, and only from a local file (see [`ffmpeg_input_args`]), so a
/// crafted download cannot make ffmpeg guess another format, open an address or
/// pull in another file. The output goes in as one absolute `file:` path, so a
/// name that starts with `-` or contains `:` is never read as an option or a
/// protocol.
pub(crate) fn arguments(
    video: (InputFormat, &Path),
    audio: (InputFormat, &Path),
    output: &Path,
) -> Vec<OsString> {
    let mut args: Vec<OsString> = ["-hide_banner", "-nostdin", "-loglevel", "error", "-y"]
        .into_iter()
        .map(OsString::from)
        .collect();
    for (format, input) in [video, audio] {
        args.extend(ffmpeg_input_args(format, input));
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

/// The format ffmpeg may read the downloaded `role` (`video` or `audio`) as,
/// recognized from its first bytes. A file that is none of the expected
/// formats is refused before ffmpeg starts.
async fn input_format(path: &Path, role: &'static str) -> Result<InputFormat, UserError> {
    let owned = path.to_path_buf();
    let detected = tokio::task::spawn_blocking(move || InputFormat::detect(&owned))
        .await
        .unwrap_or_else(|e| Err(std::io::Error::other(e.to_string())));
    match detected {
        Ok(Some(format)) => Ok(format),
        Ok(None) => Err(unknown_format(role)),
        Err(e) => Err(UserError {
            message: format!(
                "Wirecatch could not read the downloaded {role} to join it ({e}). Try again; the downloaded parts are kept."
            ),
            fix: Some(FixAction::Retry),
        }),
    }
}

/// The downloaded `role` is not in a format ffmpeg may read, worded for a
/// person, with a fresh link as the fix.
pub(crate) fn unknown_format(role: &'static str) -> UserError {
    UserError {
        message: StreamError::UnknownFormat { role }.to_string(),
        fix: Some(FixAction::RefreshLink),
    }
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
    let video_format = input_format(video, "video").await?;
    let audio_format = input_format(audio, "audio").await?;
    let args = arguments((video_format, video), (audio_format, audio), output);
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

    fn text(args: &[OsString]) -> Vec<String> {
        args.iter()
            .map(|a| a.to_string_lossy().into_owned())
            .collect()
    }

    #[test]
    fn paths_are_single_file_arguments() {
        let args = arguments(
            (InputFormat::Mp4, Path::new("/v/-video.mp4")),
            (InputFormat::Aac, Path::new("/v/a:b.m4a")),
            Path::new("/v/out.mp4"),
        );
        let text = text(&args);
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

    #[test]
    fn every_input_names_its_format_and_reads_only_local_files() {
        let args = arguments(
            (InputFormat::Mp4, Path::new("/v/video.mp4")),
            (InputFormat::WebM, Path::new("/v/audio.webm")),
            Path::new("/v/out.mkv"),
        );
        let text = text(&args);
        for (i, window) in text.windows(2).enumerate() {
            if window[0] != "-i" {
                continue;
            }
            // Every `-i` comes right after its own whitelist and format.
            let before = &text[i.saturating_sub(6)..i];
            assert_eq!(before[0], "-protocol_whitelist", "{text:?}");
            assert_eq!(before[1], "file");
            assert_eq!(before[2], "-format_whitelist");
            assert_eq!(before[4], "-f");
            assert_eq!(before[3], before[5]);
        }
        let formats: Vec<&String> = text
            .windows(2)
            .filter(|w| w[0] == "-f")
            .map(|w| &w[1])
            .collect();
        assert_eq!(formats, ["mov", "matroska"]);
    }

    fn tools(ffmpeg: &Path) -> MediaTools {
        MediaTools::for_test(ffmpeg)
    }

    /// A file that is none of the expected formats never reaches ffmpeg: the
    /// program named here does not exist, so starting it would fail with a
    /// different error and mark the add-on broken.
    #[tokio::test]
    async fn an_unknown_input_is_refused_before_ffmpeg_starts() {
        let dir = tempfile::tempdir().unwrap();
        let video = dir.path().join("video.part");
        let audio = dir.path().join("audio.part");
        std::fs::write(&video, b"#EXTM3U\n#EXT-X-ENDLIST\n").unwrap();
        std::fs::write(&audio, b"\x1A\x45\xDF\xA3webm").unwrap();
        let output = dir.path().join("joined.mkv");
        let tools = tools(&dir.path().join("no-such-ffmpeg.exe"));
        let error = join(&tools, &video, &audio, &output, &CancellationToken::new())
            .await
            .unwrap_err();
        assert!(
            error
                .message
                .starts_with("The downloaded video is not in a format Wirecatch can join"),
            "{}",
            error.message
        );
        assert_eq!(error.fix, Some(FixAction::RefreshLink));
        assert!(!tools.is_broken(), "ffmpeg was started");
        assert!(!output.exists());
    }

    /// A real ffmpeg from `WIRECATCH_TEST_FFMPEG`. Without one the test is
    /// skipped with a note, unless `WIRECATCH_REQUIRE_FFMPEG=1`.
    fn real_ffmpeg() -> Option<PathBuf> {
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

    fn make(ffmpeg: &Path, args: &[&str], path: &Path) {
        let status = std::process::Command::new(ffmpeg)
            .args(["-hide_banner", "-loglevel", "error", "-y"])
            .args(args)
            .arg(path)
            .status()
            .unwrap();
        assert!(status.success(), "could not make {}", path.display());
    }

    /// A DASH manifest that starts like MP4 (a comment puts `free` at bytes 4
    /// to 8) and names a file elsewhere on the computer: ffmpeg left to guess
    /// reads it as DASH and copies that file into the result.
    #[tokio::test]
    async fn real_ffmpeg_does_not_follow_a_manifest_disguised_as_video() {
        let Some(ffmpeg) = real_ffmpeg() else { return };
        let elsewhere = tempfile::tempdir().unwrap();
        let secret = elsewhere.path().join("secret.mp4");
        make(
            &ffmpeg,
            &[
                "-f",
                "lavfi",
                "-i",
                "testsrc=duration=1:size=64x48:rate=5",
                "-c:v",
                "mpeg4",
            ],
            &secret,
        );
        let dir = tempfile::tempdir().unwrap();
        let audio = dir.path().join("audio.m4a");
        make(
            &ffmpeg,
            &["-f", "lavfi", "-i", "sine=duration=1", "-c:a", "aac", "-vn"],
            &audio,
        );
        let target = secret.to_string_lossy().replace('\\', "/");
        let video = dir.path().join("video.mp4");
        std::fs::write(
            &video,
            format!(
                "<!--free--><MPD xmlns=\"urn:mpeg:dash:schema:mpd:2011\" \
                 profiles=\"urn:mpeg:dash:profile:isoff-on-demand:2011\" type=\"static\" \
                 mediaPresentationDuration=\"PT1S\" minBufferTime=\"PT1S\"><Period>\
                 <AdaptationSet mimeType=\"video/mp4\" contentType=\"video\">\
                 <Representation id=\"1\" bandwidth=\"1000\" mimeType=\"video/mp4\" codecs=\"mp4v.20.9\">\
                 <BaseURL>file:{target}</BaseURL><SegmentBase/></Representation>\
                 </AdaptationSet></Period></MPD>\n"
            ),
        )
        .unwrap();
        assert_eq!(
            InputFormat::detect(&video).unwrap(),
            Some(InputFormat::Mp4),
            "the crafted file must pass the format check to test ffmpeg itself"
        );
        let output = dir.path().join("joined.mp4");
        let result = join(
            &tools(&ffmpeg),
            &video,
            &audio,
            &output,
            &CancellationToken::new(),
        )
        .await;
        let error = result.expect_err("ffmpeg must refuse the disguised manifest");
        assert!(
            error.message.starts_with("ffmpeg could not join"),
            "{}",
            error.message
        );
        assert!(!output.exists(), "the joined file was written");
    }

    /// Real video and audio still join with the formats named.
    #[tokio::test]
    async fn real_ffmpeg_joins_recognized_inputs() {
        let Some(ffmpeg) = real_ffmpeg() else { return };
        let dir = tempfile::tempdir().unwrap();
        let video = dir.path().join("video.webm.part");
        let audio = dir.path().join("audio.part");
        make(
            &ffmpeg,
            &[
                "-f",
                "lavfi",
                "-i",
                "testsrc=duration=1:size=64x48:rate=5",
                "-c:v",
                "mpeg4",
                "-f",
                "mp4",
            ],
            &video,
        );
        make(
            &ffmpeg,
            &[
                "-f",
                "lavfi",
                "-i",
                "sine=duration=1",
                "-c:a",
                "aac",
                "-vn",
                "-f",
                "adts",
            ],
            &audio,
        );
        let output = dir.path().join("joined.mkv");
        let joined = join(
            &tools(&ffmpeg),
            &video,
            &audio,
            &output,
            &CancellationToken::new(),
        )
        .await
        .unwrap();
        assert_eq!(joined, Joined::Done);
        assert!(output.is_file());
    }
}
