//! Joining separately downloaded video and audio into one file with the media
//! add-on's ffmpeg: streams are copied, never re-encoded, with the same arguments a stream download uses ([`copy_arguments`]).

#![forbid(unsafe_code)]

use std::path::{Path, PathBuf};

use wirecatch_engine::CancellationToken;
use wirecatch_streams::codec::Container;
use wirecatch_streams::ffmpeg::{CopyInput, copy_arguments};
use wirecatch_streams::{InputFormat, Role};

use super::ffmpeg::{self, FfmpegError, Finished};
use crate::media::MediaTools;

/// The format ffmpeg may read the downloaded `role` as, recognized from its first bytes. A file that is none of the expected formats is refused before ffmpeg starts.
pub(crate) async fn input_format(path: &Path, role: Role) -> Result<InputFormat, FfmpegError> {
    let owned = path.to_path_buf();
    let detected = tokio::task::spawn_blocking(move || InputFormat::detect(&owned))
        .await
        .map_err(|error| FfmpegError::Task { role, error })?;
    match detected {
        Ok(Some(format)) => Ok(format),
        Ok(None) => Err(FfmpegError::UnknownFormat { role }),
        Err(source) => Err(FfmpegError::Unreadable { role, source }),
    }
}

/// `path` with the format ffmpeg may read it as (see [`input_format`]), when there is a file.
pub(crate) async fn optional_input(
    path: Option<&PathBuf>,
    role: Role,
) -> Result<Option<(InputFormat, PathBuf)>, FfmpegError> {
    match path {
        Some(path) => Ok(Some((input_format(path, role).await?, path.clone()))),
        None => Ok(None),
    }
}

/// Joins `video` and `audio` into `output` as `container` with `ffmpeg`, copying the streams.
pub(crate) async fn join(
    tools: &MediaTools,
    (video, audio): (&Path, &Path),
    output: &Path,
    container: Container,
    cancel: &CancellationToken,
) -> Result<Finished, FfmpegError> {
    let inputs = [
        CopyInput::new(video, Role::Video, input_format(video, Role::Video).await?),
        CopyInput::new(audio, Role::Audio, input_format(audio, Role::Audio).await?),
    ];
    let args = copy_arguments(&inputs, output, container)?;
    ffmpeg::run(tools, &args, output, None, cancel, |_| {}).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::media::ffmpeg::test_support::{make, real_ffmpeg};

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
        let error = join(
            &tools,
            (&video, &audio),
            &output,
            Container::Mkv,
            &CancellationToken::new(),
        )
        .await
        .unwrap_err();
        assert!(
            matches!(error, FfmpegError::UnknownFormat { role: Role::Video }),
            "{error}"
        );
        assert!(!output.exists());
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
            (&video, &audio),
            &output,
            Container::Mp4,
            &CancellationToken::new(),
        )
        .await;
        let error = result.expect_err("ffmpeg must refuse the disguised manifest");
        assert!(matches!(error, FfmpegError::Failed { .. }), "{error}");
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
            (&video, &audio),
            &output,
            Container::Mkv,
            &CancellationToken::new(),
        )
        .await
        .unwrap();
        assert_eq!(joined, Finished::Done);
        assert!(output.is_file());
    }
}
