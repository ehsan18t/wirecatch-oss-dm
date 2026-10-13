//! ffmpeg's arguments for every run Wirecatch makes (streams here, conversions in `wirecatch-core`), and joining a stream's tracks into one file by stream copy.
//!
//! ffmpeg is started with an argument vector, never through a shell, so no character in a path has a special meaning. Every path is absolute and passed as `file:<path>`, so ffmpeg reads it as a plain file even when the name starts with `-` or contains `:` or `|` (which ffmpeg would otherwise take for an option or a protocol such as `concat:`).
//!
//! The bytes ffmpeg reads come from a website, so ffmpeg is never left to guess what they are: each input names its demuxer (see [`InputFormat`]), may only be read by that demuxer, and may only open plain files. Otherwise a "segment" that is really a DASH manifest or an ffconcat script would make ffmpeg open other files on the computer, or other addresses, and copy them into the video.

#![forbid(unsafe_code)]

use std::ffi::OsString;
use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use tokio_util::sync::CancellationToken;

use crate::codec::Container;
use crate::error::{Error, Result};
use crate::model::Role;
use crate::process::{self, RunError, RunOptions, STALL_TIMEOUT};

/// The arguments [`preamble`] returns.
const PREAMBLE: [&str; 5] = ["-hide_banner", "-nostdin", "-loglevel", "error", "-y"];

/// The arguments [`progress_args`] returns.
const PROGRESS: [&str; 3] = ["-progress", "pipe:1", "-nostats"];

/// Bytes read from the start of a downloaded file to recognize its format.
const DETECT_HEAD_BYTES: usize = 512;

/// What every ffmpeg run starts with: no banner, never reading the keyboard, only errors on standard error, and replacing the output (always a file of Wirecatch's own).
#[must_use]
pub fn preamble() -> Vec<OsString> {
    PREAMBLE.into_iter().map(OsString::from).collect()
}

/// Progress reports on standard output (by default twice a second) and no other status lines. A run that reports progress can be watched for stalls ([`STALL_TIMEOUT`]).
#[must_use]
pub fn progress_args() -> Vec<OsString> {
    PROGRESS.into_iter().map(OsString::from).collect()
}

/// A format ffmpeg may read a downloaded file as, recognized from the file's first bytes with [`InputFormat::detect`].
///
/// These are the only formats Wirecatch hands to ffmpeg. Anything else (a playlist, a manifest, a script, an image) is refused before ffmpeg starts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum InputFormat {
    /// MPEG transport stream (`.ts`), the usual HLS segment format.
    MpegTs,
    /// MP4 or fragmented MP4 (`.mp4`, `.m4a`, `.m4s`), including CMAF.
    Mp4,
    /// Matroska (`.mkv`).
    Matroska,
    /// WebM, the Matroska flavor browsers play (`.webm`).
    WebM,
    /// Raw AAC audio in ADTS frames (`.aac`), as in HLS packed audio.
    Aac,
    /// MPEG audio layer 2 or 3 (`.mp3`).
    Mp3,
    /// Raw Dolby Digital audio (`.ac3`).
    Ac3,
    /// Raw Dolby Digital Plus audio (`.eac3`).
    Eac3,
}

impl InputFormat {
    /// Recognizes the file at `path` from its first bytes. `Ok(None)` when it is none of the formats above (or empty).
    ///
    /// Blocks on the file system, so call it off async threads.
    ///
    /// # Errors
    ///
    /// When the file cannot be opened or read.
    pub fn detect(path: &Path) -> io::Result<Option<InputFormat>> {
        let mut file = File::open(path)?;
        let mut head = [0u8; DETECT_HEAD_BYTES];
        let len = read_up_to(&mut file, &mut head)?;
        let head = &head[..len];
        if let Some(format) = Self::from_head(head) {
            return Ok(Some(format));
        }
        // HLS packed audio starts with an ID3 tag (its timestamp); the audio frames follow it.
        let Some(skip) = id3_len(head) else {
            return Ok(None);
        };
        file.seek(SeekFrom::Start(skip))?;
        let mut frame = [0u8; 6];
        let len = read_up_to(&mut file, &mut frame)?;
        Ok(audio_frame(&frame[..len]))
    }

    /// The format whose signature `head` starts with, without looking past an ID3 tag.
    fn from_head(head: &[u8]) -> Option<InputFormat> {
        if head.first() == Some(&0x47) {
            Some(InputFormat::MpegTs)
        } else if head
            .get(4..8)
            .is_some_and(|b| matches!(b, b"ftyp" | b"styp" | b"moov" | b"moof" | b"sidx" | b"free"))
        {
            Some(InputFormat::Mp4)
        } else if head.starts_with(&[0x1A, 0x45, 0xDF, 0xA3]) {
            if head.windows(4).any(|w| w == b"webm") {
                Some(InputFormat::WebM)
            } else {
                Some(InputFormat::Matroska)
            }
        } else {
            audio_frame(head)
        }
    }

    /// The name of the ffmpeg demuxer that reads this format (as in `ffmpeg -demuxers`).
    #[must_use]
    pub fn demuxer(self) -> &'static str {
        match self {
            InputFormat::MpegTs => "mpegts",
            InputFormat::Mp4 => "mov",
            InputFormat::Matroska | InputFormat::WebM => "matroska",
            InputFormat::Aac => "aac",
            InputFormat::Mp3 => "mp3",
            InputFormat::Ac3 => "ac3",
            InputFormat::Eac3 => "eac3",
        }
    }

    /// The usual file extension, without the dot.
    #[must_use]
    pub fn extension(self) -> &'static str {
        match self {
            InputFormat::MpegTs => "ts",
            InputFormat::Mp4 => "mp4",
            InputFormat::Matroska => "mkv",
            InputFormat::WebM => "webm",
            InputFormat::Aac => "aac",
            InputFormat::Mp3 => "mp3",
            InputFormat::Ac3 => "ac3",
            InputFormat::Eac3 => "eac3",
        }
    }
}

/// The ffmpeg arguments that open `path` as one input read as `format`, and nothing else: `-protocol_whitelist file -format_whitelist <demuxer> -f <demuxer> -i file:<path>`.
///
/// Use this for every input of an ffmpeg run that reads downloaded bytes, before the output options. ffmpeg then reads the file with exactly the demuxer named, refuses to read it as anything else, and may open no other address (no network, no `concat:` or `subfile:`), so a crafted file cannot make ffmpeg fetch something or copy another local file into the result. Find `format` with [`InputFormat::detect`] and refuse the file when that returns `None`. Keep the run to stream copy (`-c copy`), so no decoder ever parses the downloaded bytes.
///
/// `path` should be absolute; a relative one is made absolute against the current folder. It is passed as one `file:` argument, so a name starting with `-` or containing `:` or `|` is never read as an option or protocol.
///
/// # Errors
///
/// [`FileArgumentError`] when `path` cannot be made absolute (it is empty, say).
///
/// ```
/// use std::path::Path;
/// use wirecatch_streams::{InputFormat, ffmpeg_input_args};
///
/// let args = ffmpeg_input_args(InputFormat::Mp4, Path::new("/tmp/video.mp4"))?;
/// let text: Vec<_> = args.iter().map(|a| a.to_string_lossy().into_owned()).collect();
/// assert_eq!(
///     text[..7],
///     ["-protocol_whitelist", "file", "-format_whitelist", "mov", "-f", "mov", "-i"]
/// );
/// assert!(text[7].starts_with("file:"));
/// # Ok::<(), wirecatch_streams::ffmpeg::FileArgumentError>(())
/// ```
pub fn ffmpeg_input_args(
    format: InputFormat,
    path: &Path,
) -> std::result::Result<Vec<OsString>, FileArgumentError> {
    input_args_for(format.demuxer(), path)
}

/// The ffmpeg arguments that open the local file `path` as one input read only with the demuxer `demuxer` (as `ffmpeg -demuxers` names it), from a file only, as one absolute `file:` path. [`ffmpeg_input_args`] is this for downloaded media; other local inputs, such as subtitles, a metadata file or a picture that was checked to be what it claims, use it directly (D21.3).
///
/// # Errors
///
/// [`FileArgumentError`] when `path` cannot be made absolute.
pub fn input_args_for(
    demuxer: &str,
    path: &Path,
) -> std::result::Result<Vec<OsString>, FileArgumentError> {
    let mut args: Vec<OsString> = [
        "-protocol_whitelist",
        "file",
        "-format_whitelist",
        demuxer,
        "-f",
        demuxer,
        "-i",
    ]
    .into_iter()
    .map(OsString::from)
    .collect();
    args.push(file_argument(path)?);
    Ok(args)
}

/// A path that cannot be given to ffmpeg as one absolute `file:` argument.
#[derive(Debug, thiserror::Error)]
#[error("{} cannot be given to ffmpeg as an absolute path", .path.display())]
#[non_exhaustive]
pub struct FileArgumentError {
    /// The path.
    pub path: PathBuf,
    /// Why it cannot be made absolute.
    #[source]
    pub source: io::Error,
}

/// `path` as one `file:` argument, made absolute against the current folder when it is not, so ffmpeg reads or writes it as a plain file whatever the name: one starting with `-` or holding `:` or `|` is never taken for an option or a protocol.
///
/// # Errors
///
/// [`FileArgumentError`] when `path` cannot be made absolute (it is empty, say): a relative path would be read against whatever folder ffmpeg runs in.
pub fn file_argument(path: &Path) -> std::result::Result<OsString, FileArgumentError> {
    let absolute = std::path::absolute(path).map_err(|source| FileArgumentError {
        path: path.to_path_buf(),
        source,
    })?;
    let mut arg = OsString::from("file:");
    arg.push(absolute.as_os_str());
    Ok(arg)
}

/// One downloaded file to copy into a container. Made with [`CopyInput::new`].
#[derive(Debug, Clone, Copy)]
#[non_exhaustive]
pub struct CopyInput<'a> {
    /// The file.
    pub path: &'a Path,
    /// What it holds. Audio inside a video input is taken only when no input is audio alone.
    pub role: Role,
    /// The format it was recognized as.
    pub format: InputFormat,
}

impl<'a> CopyInput<'a> {
    /// `path`, holding `role`, recognized as `format`.
    #[must_use]
    pub fn new(path: &'a Path, role: Role, format: InputFormat) -> Self {
        Self { path, role, format }
    }
}

/// ffmpeg's arguments to copy the video and audio streams of `inputs` (in order) into `output` as `container`, never re-encoding them, with progress reports ([`progress_args`]). The muxer is always named, so the extension never decides, and MP4 files get their index at the front so players start at once.
///
/// # Errors
///
/// [`FileArgumentError`] when an input or the output cannot be made absolute.
pub fn copy_arguments(
    inputs: &[CopyInput<'_>],
    output: &Path,
    container: Container,
) -> std::result::Result<Vec<OsString>, FileArgumentError> {
    let mut args = preamble();
    for input in inputs {
        args.extend(ffmpeg_input_args(input.format, input.path)?);
    }
    let separate_audio = inputs.iter().any(|i| i.role == Role::Audio);
    for (index, input) in inputs.iter().enumerate() {
        // Only video and audio: streams such as timed ID3 metadata in MPEG-TS cannot go into MP4 and would make the copy fail. `?` skips a kind that an input does not have.
        if input.role == Role::Video {
            args.push("-map".into());
            args.push(format!("{index}:v?").into());
        }
        if input.role == Role::Audio || !separate_audio {
            args.push("-map".into());
            args.push(format!("{index}:a?").into());
        }
    }
    args.extend(["-c", "copy"].map(OsString::from));
    if container.faststart() {
        args.extend(["-movflags", "+faststart"].map(OsString::from));
    }
    args.extend(progress_args());
    args.extend(["-f", container.muxer()].map(OsString::from));
    args.push(file_argument(output)?);
    Ok(args)
}

/// Reads until `buf` is full or the file ends; returns how much was read.
fn read_up_to(file: &mut File, buf: &mut [u8]) -> io::Result<usize> {
    let mut len = 0;
    while len < buf.len() {
        match file.read(&mut buf[len..])? {
            0 => break,
            n => len += n,
        }
    }
    Ok(len)
}

/// The full length of the ID3v2 tag `head` starts with, if it starts with one.
fn id3_len(head: &[u8]) -> Option<u64> {
    let header = head.get(..10)?;
    if !header.starts_with(b"ID3") || header[6..10].iter().any(|b| b & 0x80 != 0) {
        return None;
    }
    let size = header[6..10]
        .iter()
        .fold(0u64, |size, b| (size << 7) | u64::from(*b));
    let footer = if header[5] & 0x10 != 0 { 10 } else { 0 };
    Some(10 + size + footer)
}

/// The raw audio format whose frame `head` starts with.
fn audio_frame(head: &[u8]) -> Option<InputFormat> {
    match head {
        // ADTS: 12-bit sync, layer 0.
        [0xFF, b, ..] if b & 0xF6 == 0xF0 => Some(InputFormat::Aac),
        // MPEG audio: 11-bit sync, then a version and a layer that are not the reserved values.
        [0xFF, b, ..] if b & 0xE0 == 0xE0 && b & 0x18 != 0x08 && b & 0x06 != 0 => {
            Some(InputFormat::Mp3)
        }
        // AC-3 sync word; the bitstream id tells E-AC-3 apart.
        [0x0B, 0x77, _, _, _, info, ..] => match info >> 3 {
            0..=10 => Some(InputFormat::Ac3),
            11..=16 => Some(InputFormat::Eac3),
            _ => None,
        },
        _ => None,
    }
}

/// Runs `program` with `args`, writing `output` in its folder. Returns `Ok(false)` when cancelled (ffmpeg and its group are stopped), and an error with what ffmpeg reported when it fails. A run that reports nothing is stuck only when `output` does not grow either.
pub(crate) async fn run(
    program: &Path,
    args: &[OsString],
    output: &Path,
    cancel: &CancellationToken,
) -> Result<bool> {
    let dir = output.parent().unwrap_or(output);
    let options = RunOptions::lines()
        .with_idle_timeout(STALL_TIMEOUT)
        .watching(output.to_path_buf());
    match process::run(program, args, dir, options, cancel, |_, _| {}).await {
        Ok(ran) if ran.success => Ok(true),
        Ok(ran) => Err(Error::Ffmpeg(summary(&ran.stderr))),
        Err(RunError::Cancelled) => Ok(false),
        Err(RunError::Start { source, .. }) if source.kind() == io::ErrorKind::NotFound => {
            Err(Error::FfmpegMissing(program.to_path_buf()))
        }
        Err(e) => Err(Error::FfmpegRun(e)),
    }
}

/// Lines of ffmpeg's error output kept in its error message.
const SUMMARY_LINES: usize = 3;

/// The last few lines ffmpeg printed.
fn summary(stderr: &[String]) -> String {
    let lines: Vec<&str> = stderr
        .iter()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty())
        .collect();
    let tail = lines[lines.len().saturating_sub(SUMMARY_LINES)..].join(" / ");
    if tail.is_empty() {
        process::NO_REASON.into()
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
            let args = copy_arguments(
                &[
                    CopyInput::new(&video, Role::Video, InputFormat::MpegTs),
                    CopyInput::new(&audio, Role::Audio, InputFormat::Aac),
                ],
                &out,
                Container::Mp4,
            )
            .unwrap();
            let as_text: Vec<String> = args
                .iter()
                .map(|a| a.to_string_lossy().into_owned())
                .collect();
            // Each path is exactly one argument, prefixed so it can never be read as an option or a protocol.
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
                    "-protocol_whitelist",
                    "file",
                    "-format_whitelist",
                    "mpegts",
                    "-f",
                    "mpegts",
                    "-i",
                    "-protocol_whitelist",
                    "file",
                    "-format_whitelist",
                    "aac",
                    "-f",
                    "aac",
                    "-i",
                    "-map",
                    "0:v?",
                    "-map",
                    "1:a?",
                    "-c",
                    "copy",
                    "-movflags",
                    "+faststart",
                    "-progress",
                    "pipe:1",
                    "-nostats",
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
        let args = copy_arguments(
            &[CopyInput::new(&video, Role::Video, InputFormat::MpegTs)],
            &out,
            Container::Mkv,
        )
        .unwrap();
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
        assert!(!text.contains(&"+faststart".to_string()), "MP4 only");
        assert!(
            !text.iter().any(|a| a.contains("-c:v") || a == "libx264"),
            "never re-encodes"
        );
    }

    #[test]
    fn every_input_names_its_demuxer_and_allows_only_files() {
        for format in [
            InputFormat::MpegTs,
            InputFormat::Mp4,
            InputFormat::Matroska,
            InputFormat::WebM,
            InputFormat::Aac,
            InputFormat::Mp3,
            InputFormat::Ac3,
            InputFormat::Eac3,
        ] {
            let path = absolute("in");
            let text: Vec<String> = ffmpeg_input_args(format, &path)
                .unwrap()
                .iter()
                .map(|a| a.to_string_lossy().into_owned())
                .collect();
            let demuxer = format.demuxer();
            // The options apply to the next `-i`, so they must come first.
            assert_eq!(
                text,
                [
                    "-protocol_whitelist",
                    "file",
                    "-format_whitelist",
                    demuxer,
                    "-f",
                    demuxer,
                    "-i",
                    &format!("file:{}", path.display()),
                ],
                "{format:?}"
            );
        }
    }

    #[test]
    fn relative_input_paths_are_made_absolute() {
        let text = ffmpeg_input_args(InputFormat::Mp4, Path::new("relative.mp4")).unwrap();
        let last = text.last().unwrap().to_string_lossy().into_owned();
        let path = Path::new(last.strip_prefix("file:").unwrap());
        assert!(path.is_absolute(), "{last}");
    }

    /// A path that cannot be made absolute is refused rather than passed as it is, which ffmpeg would read against the folder it runs in.
    #[test]
    fn a_path_that_cannot_be_made_absolute_is_refused() {
        let refused = file_argument(Path::new(""));
        assert!(
            matches!(&refused, Err(FileArgumentError { path, .. }) if path.as_os_str().is_empty()),
            "{refused:?}"
        );
        assert!(ffmpeg_input_args(InputFormat::Mp4, Path::new("")).is_err());
        let video = absolute("v.ts");
        let input = CopyInput::new(&video, Role::Video, InputFormat::MpegTs);
        assert!(copy_arguments(&[input], Path::new(""), Container::Mkv).is_err());
    }

    #[test]
    fn formats_are_recognized_from_their_first_bytes() {
        let dir = wirecatch_test_support::private_dir("detect").unwrap();
        // An ID3 tag of 20 bytes (size 10 plus the header), then a frame.
        let id3 = |frame: &[u8]| {
            let mut bytes = b"ID3\x04\x00\x00\x00\x00\x00\x0a".to_vec();
            bytes.extend_from_slice(&[0; 10]);
            bytes.extend_from_slice(frame);
            bytes
        };
        let cases: Vec<(Vec<u8>, Option<InputFormat>)> = vec![
            (vec![0x47, 0x40, 0x11], Some(InputFormat::MpegTs)),
            (b"\0\0\0\x18ftypisom".to_vec(), Some(InputFormat::Mp4)),
            (b"\0\0\0\x18stypmsdh".to_vec(), Some(InputFormat::Mp4)),
            (
                vec![
                    0x1A, 0x45, 0xDF, 0xA3, 0x42, 0x82, 0x84, b'w', b'e', b'b', b'm',
                ],
                Some(InputFormat::WebM),
            ),
            (
                vec![0x1A, 0x45, 0xDF, 0xA3, 0x42, 0x82, 0x88, b'm', b'a', b't'],
                Some(InputFormat::Matroska),
            ),
            (vec![0xFF, 0xF1, 0x50], Some(InputFormat::Aac)),
            (vec![0xFF, 0xFB, 0x90], Some(InputFormat::Mp3)),
            (vec![0x0B, 0x77, 0, 0, 0, 0x40], Some(InputFormat::Ac3)),
            (vec![0x0B, 0x77, 0, 0, 0, 0x80], Some(InputFormat::Eac3)),
            (id3(&[0xFF, 0xF1, 0x50]), Some(InputFormat::Aac)),
            (id3(&[0xFF, 0xFB, 0x90]), Some(InputFormat::Mp3)),
            (id3(&[0x0B, 0x77, 0, 0, 0, 0x80]), Some(InputFormat::Eac3)),
            // Never handed to ffmpeg: playlists, manifests, scripts, nothing.
            (b"#EXTM3U\n#EXT-X-ENDLIST\n".to_vec(), None),
            (b"<?xml version=\"1.0\"?><MPD/>".to_vec(), None),
            (b"ffconcat version 1.0\nfile a.ts\n".to_vec(), None),
            (id3(b"#EXTM3U"), None),
            (b"ID3".to_vec(), None),
            (Vec::new(), None),
        ];
        for (bytes, expected) in cases {
            let path = dir.join("track");
            std::fs::write(&path, &bytes).unwrap();
            assert_eq!(InputFormat::detect(&path).unwrap(), expected, "{bytes:?}");
        }
        dir.remove().unwrap();
    }

    #[test]
    fn summaries_keep_the_last_lines() {
        let lines = ["a", "", "b", "c", "d"].map(String::from);
        assert_eq!(summary(&lines), "b / c / d");
        assert_eq!(summary(&[]), "it gave no reason");
    }
}
