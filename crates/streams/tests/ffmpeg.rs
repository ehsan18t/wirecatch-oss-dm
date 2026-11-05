//! Joining with ffmpeg: the arguments it gets, its failures, and (when one
//! is installed) a real ffmpeg.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::Command;

use wirecatch_streams::{Error, InputFormat, Outcome, Role, extension, ffmpeg_input_args};
use wirecatch_test_support::files::FileServer;
use wirecatch_test_support::{PrivateDir, TestResult, private_dir, real_ffmpeg, ts_segment};

mod common;
use common::{request, run};

const MPEGURL: &str = "application/vnd.apple.mpegurl";

/// The fake ffmpeg that `cargo test` builds from `examples/fake_ffmpeg.rs`.
/// Missing only when the tests are built alone (`--test ffmpeg`), which
/// skips the examples; that fails loudly instead of skipping, because these
/// tests prove the injection safety.
fn fake_ffmpeg() -> TestResult<PathBuf> {
    let exe = std::env::current_exe()?;
    let debug = exe
        .parent()
        .and_then(Path::parent)
        .ok_or("the test binary is not in a build folder")?;
    let path = debug
        .join("examples")
        .join(format!("fake_ffmpeg{}", std::env::consts::EXE_SUFFIX));
    if !path.is_file() {
        let missing = path.display();
        return Err(format!(
            "{missing} is missing: run `cargo test -p wirecatch-streams`, which builds the examples"
        )
        .into());
    }
    Ok(path)
}

/// A master playlist with video and separate audio, two segments each.
fn video_and_audio(server: &FileServer) -> TestResult {
    for (prefix, seed) in [("v", 1), ("a", 2)] {
        let mut text = String::from("#EXTM3U\n");
        for i in 0..2 {
            server.put(&format!("{prefix}/{i}.ts"), ts_segment(seed * 10 + i, 1500));
            write!(text, "#EXTINF:2,\n{i}.ts\n")?;
        }
        text.push_str("#EXT-X-ENDLIST\n");
        server.put_typed(&format!("{prefix}/index.m3u8"), text, MPEGURL);
    }
    server.put_typed(
        "master.m3u8",
        "#EXTM3U\n#EXT-X-MEDIA:TYPE=AUDIO,GROUP-ID=\"aud\",NAME=\"Main\",DEFAULT=YES,URI=\"a/index.m3u8\"\n\
         #EXT-X-STREAM-INF:BANDWIDTH=900000,CODECS=\"avc1.4d401e,mp4a.40.2\",AUDIO=\"aud\"\nv/index.m3u8\n",
        MPEGURL,
    );
    Ok(())
}

/// A folder name full of characters a shell or ffmpeg would treat specially.
/// A folder whose name a shell would misread, inside a folder of the test's
/// own that holds it until the test ends.
fn hostile_dir(test: &str) -> TestResult<(PrivateDir, PathBuf)> {
    // Windows does not allow `"` in names; other systems get it too.
    let name = if cfg!(windows) {
        "-y it's & $HOME `x` (1);"
    } else {
        "-y it's \"odd\" & $HOME `x` (1);"
    };
    let root = private_dir(test)?;
    let dir = root.join(name);
    std::fs::create_dir_all(&dir)?;
    Ok((root, dir))
}

#[tokio::test]
async fn ffmpeg_gets_each_path_as_one_untouched_argument() {
    let ffmpeg = fake_ffmpeg().unwrap();
    let server = FileServer::start().await;
    video_and_audio(&server).unwrap();
    let (root, dir) = hostile_dir("ffmpeg-args").unwrap();
    let output = dir.join("-i evil name.mp4");
    let mut request = request(server.url("master.m3u8"), &output);
    request.ffmpeg = Some(ffmpeg);

    let (path, bytes) = match run(request).await.unwrap() {
        Outcome::Completed { path, bytes, .. } => (path, bytes),
        other => panic!("{other:?}"),
    };
    assert_eq!(path, output, "MP4 for avc1 and mp4a");
    // The fake wrote the arguments it received, one per line, as its output.
    let received = std::fs::read_to_string(&path).unwrap();
    assert_eq!(bytes, received.len() as u64);
    let args: Vec<&str> = received.lines().collect();
    let work = dir.join("-i evil name.mp4.wcstream");
    let file = |p: PathBuf| format!("file:{}", p.display());
    assert_eq!(
        args,
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
            &file(work.join("track0")),
            "-protocol_whitelist",
            "file",
            "-format_whitelist",
            "mpegts",
            "-f",
            "mpegts",
            "-i",
            &file(work.join("track1")),
            "-map",
            "0:v?",
            "-map",
            "1:a?",
            "-c",
            "copy",
            // Players can start an MP4 before they have read all of it.
            "-movflags",
            "+faststart",
            "-progress",
            "pipe:1",
            "-nostats",
            "-f",
            "mp4",
            &file(work.join("joined.mp4")),
        ]
    );
    assert!(!work.exists(), "temporary files are removed");
    root.remove().unwrap();
}

/// ffmpeg's last lines are rarely the cause: the error keeps all it wrote, for the wording table.
#[tokio::test]
async fn a_cause_ffmpeg_named_early_stays_in_the_error() {
    let ffmpeg = fake_ffmpeg().unwrap();
    let server = FileServer::start().await;
    video_and_audio(&server).unwrap();
    let dir = private_dir("ffmpeg-early-cause").unwrap();
    let mut request = request(server.url("master.m3u8"), dir.join("fail-early-cause.mp4"));
    request.ffmpeg = Some(ffmpeg);
    let Error::Ffmpeg(said) = run(request).await.unwrap_err() else {
        panic!("ffmpeg's failure is not reported as one");
    };
    assert!(
        said.contains("not currently supported in container"),
        "{said}"
    );
    dir.remove().unwrap();
}

#[tokio::test]
async fn ffmpeg_failures_are_reported_and_the_parts_kept() {
    let ffmpeg = fake_ffmpeg().unwrap();
    let server = FileServer::start().await;
    video_and_audio(&server).unwrap();
    let dir = private_dir("ffmpeg-fail").unwrap();
    let output = dir.join("fail-please.mp4");
    let mut request = request(server.url("master.m3u8"), &output);
    request.ffmpeg = Some(ffmpeg.clone());
    let error = run(request.clone()).await.unwrap_err();
    assert!(matches!(error, Error::Ffmpeg(_)), "{error}");
    let message = error.to_string();
    assert!(
        message.starts_with(
            "ffmpeg could not join the video: as MP4: [mp4 @ 0x1] fake ffmpeg failed / Conversion failed!; as MKV: [mp4 @ 0x1] fake ffmpeg failed"
        ),
        "{message}"
    );
    let work = dir.join("fail-please.mp4.wcstream");
    assert!(work.join("track0").is_file() && work.join("track1").is_file());

    // Trying again joins from the kept tracks without downloading anything.
    let before = server.seen().len();
    request.ffmpeg = Some(dir.join("no such ffmpeg.exe"));
    let error = run(request).await.unwrap_err();
    assert!(matches!(error, Error::FfmpegMissing(_)), "{error}");
    assert!(
        error
            .to_string()
            .starts_with("The ffmpeg add-on was not found at")
    );
    let fetched_again: Vec<_> = server.seen()[before..]
        .iter()
        .filter(|s| s.has_extension("ts"))
        .map(|s| s.path.clone())
        .collect();
    assert!(fetched_again.is_empty(), "{fetched_again:?}");
    dir.remove().unwrap();
}

/// Writes a short HLS stream with a real ffmpeg (stream copy friendly codecs:
/// MPEG-4 Part 2 video and AAC audio, both native ffmpeg encoders).
fn generate(ffmpeg: &Path, dir: &Path, input: &str, codec: &[&str], name: &str) -> TestResult {
    let status = Command::new(ffmpeg)
        .args([
            "-hide_banner",
            "-loglevel",
            "error",
            "-y",
            "-f",
            "lavfi",
            "-i",
            input,
        ])
        .args(codec)
        .args([
            "-f",
            "hls",
            "-hls_time",
            "1",
            "-hls_playlist_type",
            "vod",
            "-hls_segment_filename",
        ])
        .arg(dir.join(format!("{name}%d.ts")))
        .arg(dir.join(format!("{name}.m3u8")))
        .status()?;
    if !status.success() {
        return Err(format!("ffmpeg could not generate the {name} fixture").into());
    }
    Ok(())
}

/// What a real ffmpeg reports about a file's streams.
fn probe(ffmpeg: &Path, file: &Path) -> TestResult<String> {
    let output = Command::new(ffmpeg)
        .arg("-hide_banner")
        .arg("-i")
        .arg(file)
        .output()?;
    Ok(String::from_utf8_lossy(&output.stderr).into_owned())
}

/// Serves every file in `dir` at its name.
fn serve_folder(server: &FileServer, dir: &Path) -> TestResult {
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().into_owned();
        server.put(&name, std::fs::read(entry.path())?);
    }
    Ok(())
}

#[tokio::test]
async fn real_ffmpeg_merges_a_codec_mp4_players_refuse_into_matroska() {
    let Some(ffmpeg) = real_ffmpeg() else { return };
    let source = private_dir("real-ffmpeg-source").unwrap();
    generate(
        &ffmpeg,
        &source,
        "testsrc=duration=3:size=160x120:rate=10",
        // MPEG-4 Part 2 needs its VOL header repeated in each segment.
        &["-c:v", "mpeg4", "-bsf:v", "dump_extra"],
        "v",
    )
    .unwrap();
    generate(&ffmpeg, &source, "sine=duration=3", &["-c:a", "aac"], "a").unwrap();
    let server = FileServer::start().await;
    serve_folder(&server, &source).unwrap();
    server.put_typed(
        "master.m3u8",
        "#EXTM3U\n#EXT-X-MEDIA:TYPE=AUDIO,GROUP-ID=\"aud\",NAME=\"Main\",DEFAULT=YES,URI=\"a.m3u8\"\n\
         #EXT-X-STREAM-INF:BANDWIDTH=200000,CODECS=\"mp4v.20.9,mp4a.40.2\",AUDIO=\"aud\"\nv.m3u8\n",
        MPEGURL,
    );
    let (root, dir) = hostile_dir("real-ffmpeg").unwrap();
    let mut request = request(server.url("master.m3u8"), dir.join("-out video.mp4"));
    request.ffmpeg = Some(ffmpeg.clone());
    let path = match run(request).await.unwrap() {
        Outcome::Completed { path, .. } => path,
        other => panic!("{other:?}"),
    };
    // MPEG-4 Part 2 is valid in MP4, but MP4 is kept to what its players play (D24).
    assert_eq!(path, dir.join("-out video.mkv"));
    let report = probe(&ffmpeg, &path).unwrap();
    assert!(report.contains("matroska"), "{report}");
    assert!(report.contains("Video: mpeg4"), "{report}");
    assert!(report.contains("Audio: aac"), "{report}");
    root.remove().unwrap();
    source.remove().unwrap();
}

#[tokio::test]
async fn real_ffmpeg_remuxes_a_single_ts_track() {
    let Some(ffmpeg) = real_ffmpeg() else { return };
    let source = private_dir("real-ffmpeg-single-source").unwrap();
    let status = Command::new(&ffmpeg)
        .args(["-hide_banner", "-loglevel", "error", "-y"])
        .args([
            "-f",
            "lavfi",
            "-i",
            "testsrc=duration=3:size=160x120:rate=10",
        ])
        .args([
            "-f",
            "lavfi",
            "-i",
            "sine=duration=3",
            "-c:v",
            "mpeg4",
            "-bsf:v",
            "dump_extra",
            "-c:a",
            "aac",
        ])
        .args([
            "-f",
            "hls",
            "-hls_time",
            "1",
            "-hls_playlist_type",
            "vod",
            "-hls_segment_filename",
        ])
        .arg(source.join("s%d.ts"))
        .arg(source.join("index.m3u8"))
        .status()
        .unwrap();
    assert!(status.success());
    let server = FileServer::start().await;
    serve_folder(&server, &source).unwrap();
    let dir = private_dir("real-ffmpeg-single").unwrap();
    let mut request = request(server.url("index.m3u8"), dir.join("clip"));
    request.ffmpeg = Some(ffmpeg.clone());
    let path = match run(request).await.unwrap() {
        Outcome::Completed { path, .. } => path,
        other => panic!("{other:?}"),
    };
    assert_eq!(path, dir.join("clip.mp4"));
    let report = probe(&ffmpeg, &path).unwrap();
    assert!(
        report.contains("Video: mpeg4") && report.contains("Audio: aac"),
        "{report}"
    );
    dir.remove().unwrap();
    source.remove().unwrap();
}

/// A one-segment HLS stream whose only segment is `payload`.
fn one_segment(server: &FileServer, payload: impl Into<bytes::Bytes>) {
    server.put("0.m4s", payload);
    server.put_typed(
        "index.m3u8",
        "#EXTM3U\n#EXT-X-TARGETDURATION:2\n#EXTINF:1,\n0.m4s\n#EXT-X-ENDLIST\n",
        MPEGURL,
    );
}

#[tokio::test]
async fn a_track_in_an_unknown_format_is_never_given_to_ffmpeg() {
    let ffmpeg = fake_ffmpeg().unwrap();
    let server = FileServer::start().await;
    // An ffconcat script would make ffmpeg read whatever files it names. A control byte, which no text has, leaves it to the format check.
    one_segment(&server, "ffconcat version 1.0\u{1}\nfile secret.mp4\n");
    let dir = private_dir("ffmpeg-unknown").unwrap();
    let output = dir.join("clip.mp4");
    let mut request = request(server.url("index.m3u8"), &output);
    request.ffmpeg = Some(ffmpeg);
    let error = run(request).await.unwrap_err();
    assert!(
        matches!(error, Error::UnknownFormat { role: Role::Video }),
        "{error}"
    );
    assert!(
        error
            .to_string()
            .starts_with("The downloaded video is not in a format Wirecatch can join"),
        "{error}"
    );
    // The fake ffmpeg would have written the output had it been started.
    assert!(!output.exists());
    // Kept, the same bad track would fail every try, whatever the link.
    assert!(!dir.join("clip.mp4.wcstream").join("track0").exists());
    let mut again = common::request(server.url("index.m3u8"), &output);
    again.ffmpeg = Some(fake_ffmpeg().unwrap());
    one_segment(&server, ts_segment(1, 1500));
    assert!(matches!(run(again).await, Ok(Outcome::Completed { .. })));
    assert_eq!(server.requests("0.m4s"), 2);
    dir.remove().unwrap();
}

#[tokio::test]
async fn real_ffmpeg_does_not_follow_a_manifest_disguised_as_a_segment() {
    let Some(ffmpeg) = real_ffmpeg() else { return };
    // A file elsewhere on the computer that the "segment" tries to pull in.
    let elsewhere = private_dir("real-ffmpeg-elsewhere").unwrap();
    let secret = elsewhere.join("secret.mp4");
    let status = Command::new(&ffmpeg)
        .args(["-hide_banner", "-loglevel", "error", "-y", "-f", "lavfi"])
        .args([
            "-i",
            "testsrc=duration=1:size=64x48:rate=5",
            "-c:v",
            "mpeg4",
        ])
        .arg(&secret)
        .status()
        .unwrap();
    assert!(
        status.success(),
        "ffmpeg could not generate the secret file"
    );
    // A DASH manifest naming that file. The leading comment puts `free` at bytes 4 to 8 and a control byte, which no text has, so the segment passes as MP4; ffmpeg left to guess would read it as DASH and copy the file in.
    let target = secret.to_string_lossy().replace('\\', "/");
    let payload = format!(
        "<!--free\u{1}--><MPD xmlns=\"urn:mpeg:dash:schema:mpd:2011\" \
         profiles=\"urn:mpeg:dash:profile:isoff-on-demand:2011\" type=\"static\" \
         mediaPresentationDuration=\"PT1S\" minBufferTime=\"PT1S\"><Period>\
         <AdaptationSet mimeType=\"video/mp4\" contentType=\"video\">\
         <Representation id=\"1\" bandwidth=\"1000\" mimeType=\"video/mp4\" codecs=\"mp4v.20.9\">\
         <BaseURL>file:{target}</BaseURL><SegmentBase/></Representation>\
         </AdaptationSet></Period></MPD>\n"
    );
    let server = FileServer::start().await;
    one_segment(&server, payload);
    let dir = private_dir("real-ffmpeg-disguised").unwrap();
    let output = dir.join("clip.mp4");
    let mut request = request(server.url("index.m3u8"), &output);
    request.ffmpeg = Some(ffmpeg);
    let result = run(request).await;
    assert!(
        matches!(result, Err(Error::Ffmpeg(_))),
        "ffmpeg must refuse the segment: {result:?}"
    );
    for name in ["clip.mp4", "clip.mkv"] {
        assert!(!dir.join(name).exists(), "{name} was written");
    }
    dir.remove().unwrap();
    elsewhere.remove().unwrap();
}

/// A real ffmpeg copies past a damaged part and reports success, so a page in place of a segment has to be caught before it runs.
#[tokio::test]
async fn real_ffmpeg_never_sees_a_page_in_place_of_a_segment() -> TestResult {
    const SEGMENT_TYPES: [(&str, &[&str]); 2] = [
        ("s%d.ts", &[]),
        (
            "s%d.m4s",
            &[
                "-hls_segment_type",
                "fmp4",
                "-hls_fmp4_init_filename",
                "init.mp4",
            ],
        ),
    ];
    let Some(ffmpeg) = real_ffmpeg() else {
        return Ok(());
    };
    for (n, (segment, extra)) in SEGMENT_TYPES.into_iter().enumerate() {
        let source = private_dir(&format!("page-in-a-track-source-{n}"))?;
        let made = Command::new(&ffmpeg)
            .current_dir(source.path())
            .args(["-hide_banner", "-loglevel", "error", "-y", "-f", "lavfi"])
            .args(["-i", "testsrc=duration=6:size=160x120:rate=10"])
            .args(["-c:v", "mpeg4", "-bsf:v", "dump_extra", "-g", "10"])
            .args(["-f", "hls", "-hls_time", "2", "-hls_playlist_type", "vod"])
            .args(extra)
            .args(["-hls_segment_filename", segment, "index.m3u8"])
            .status()?;
        assert!(made.success());
        let page = segment.replace("%d", "1");
        let server = FileServer::start().await;
        for entry in std::fs::read_dir(source.path())? {
            let path = entry?.path();
            let name = path
                .file_name()
                .and_then(|name| name.to_str())
                .ok_or("a file without a name")?
                .to_owned();
            let bytes = std::fs::read(&path)?;
            if name == page {
                server.put(&name, b"<!doctype html><html>Sign in.</html>".to_vec());
            } else if path.extension().is_some_and(|e| e == "m3u8") {
                server.put_typed(&name, bytes, MPEGURL);
            } else {
                server.put(&name, bytes);
            }
        }
        let dir = private_dir(&format!("page-in-a-track-{n}"))?;
        let mut asked = request(server.url("index.m3u8"), dir.join("clip"));
        asked.ffmpeg = Some(ffmpeg.clone());
        let got = run(asked).await;
        assert!(
            matches!(got, Err(Error::NotMedia { .. })),
            "{segment}: {got:?}"
        );
    }
    Ok(())
}

/// Every extension a saved file may have is recognised as the format the list names, and a real ffmpeg reads such a file with that format's demuxer, the only one it is allowed.
#[test]
fn real_ffmpeg_reads_every_media_extension_as_the_format_listed() -> TestResult {
    const VIDEO: &[&str] = &["-f", "lavfi", "-i", "testsrc=duration=1:size=64x48:rate=5"];
    const AUDIO: &[&str] = &["-f", "lavfi", "-i", "sine=duration=1"];
    let Some(ffmpeg) = real_ffmpeg() else {
        return Ok(());
    };
    let samples: [(&str, &[&str], &[&str]); 18] = [
        ("mp4", VIDEO, &["-c:v", "mpeg4"]),
        ("m4v", VIDEO, &["-c:v", "mpeg4", "-f", "mp4"]),
        ("mkv", VIDEO, &["-c:v", "mpeg4"]),
        ("mka", AUDIO, &["-c:a", "aac"]),
        ("webm", VIDEO, &["-c:v", "libvpx", "-b:v", "100k"]),
        ("mov", VIDEO, &["-c:v", "mpeg4"]),
        ("ts", VIDEO, &["-c:v", "mpeg4"]),
        (
            "m2ts",
            VIDEO,
            &["-c:v", "mpeg4", "-f", "mpegts", "-mpegts_m2ts_mode", "1"],
        ),
        ("mp3", AUDIO, &["-c:a", "libmp3lame"]),
        ("m4a", AUDIO, &["-c:a", "aac"]),
        ("aac", AUDIO, &["-c:a", "aac", "-f", "adts"]),
        ("opus", AUDIO, &["-c:a", "libopus"]),
        ("ogg", AUDIO, &["-c:a", "libvorbis"]),
        ("flac", AUDIO, &["-c:a", "flac"]),
        ("wav", AUDIO, &["-c:a", "pcm_s16le"]),
        (
            "3gp",
            VIDEO,
            &["-c:v", "h263", "-s", "176x144", "-f", "3gp"],
        ),
        ("flv", VIDEO, &["-c:v", "flv1"]),
        ("avi", VIDEO, &["-c:v", "mpeg4"]),
    ];
    let mut with_sample: Vec<&str> = samples.iter().map(|(ext, _, _)| *ext).collect();
    let mut listed: Vec<&str> = extension::media().collect();
    with_sample.sort_unstable();
    listed.sort_unstable();
    assert_eq!(
        with_sample, listed,
        "every media extension needs a sample here"
    );
    let dir = private_dir("every-media-extension")?;
    for (ext, input, output) in samples {
        let listed = extension::read_as(ext).ok_or(format!("{ext} is not listed"))?;
        let file = dir.join(format!("sample.{ext}"));
        let made = Command::new(&ffmpeg)
            .args(["-hide_banner", "-loglevel", "error", "-y"])
            .args(input)
            .args(output)
            .arg(&file)
            .status()?;
        // Some builds of ffmpeg leave out an encoder; the sample then says nothing.
        if !made.success() {
            eprintln!("skipped {ext}: this ffmpeg could not make a sample");
            continue;
        }
        assert_eq!(InputFormat::detect(&file)?, Some(listed), "{ext}");
        let read = Command::new(&ffmpeg)
            .args(["-hide_banner", "-loglevel", "error"])
            .args(ffmpeg_input_args(listed, &file)?)
            .args(["-c", "copy", "-f", "null", "-"])
            .status()?;
        assert!(read.success(), "{ext}");
    }
    Ok(())
}
