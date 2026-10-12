use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::time::Duration;

use wirecatch_engine::{CancellationToken, DownloadOptions};
use wirecatch_streams::{Error, Outcome, StreamProgress, StreamRequest, download};
use wirecatch_test_support::files::FileServer;
use wirecatch_test_support::{fresh_dir, ts_segment};

const MPEGURL: &str = "application/vnd.apple.mpegurl";

/// The fake ffmpeg that `cargo test` builds from `examples/fake_ffmpeg.rs`.
/// Missing only when the tests are built alone (`--test ffmpeg`), which
/// skips the examples; that fails loudly instead of skipping, because these
/// tests prove the injection safety.
fn fake_ffmpeg() -> Option<PathBuf> {
    let exe = std::env::current_exe().unwrap();
    let debug = exe.parent().unwrap().parent().unwrap();
    let path = debug
        .join("examples")
        .join(format!("fake_ffmpeg{}", std::env::consts::EXE_SUFFIX));
    assert!(
        path.is_file(),
        "{} is missing: run `cargo test -p wirecatch-streams`, which builds the examples",
        path.display()
    );
    Some(path)
}

/// A real ffmpeg from `WIRECATCH_TEST_FFMPEG` or the `PATH`.
///
/// Without one the test is skipped with a note, unless
/// `WIRECATCH_REQUIRE_FFMPEG=1` (set in CI): then it fails, so a missing
/// ffmpeg can never make these tests pass silently.
fn real_ffmpeg() -> Option<PathBuf> {
    let configured = std::env::var_os("WIRECATCH_TEST_FFMPEG").map(PathBuf::from);
    let found = match &configured {
        Some(path) if path.is_file() => Some(path.clone()),
        Some(_) => None,
        None => {
            let name = format!("ffmpeg{}", std::env::consts::EXE_SUFFIX);
            std::env::var_os("PATH").and_then(|paths| {
                std::env::split_paths(&paths)
                    .map(|d| d.join(&name))
                    .find(|p| p.is_file())
            })
        }
    };
    if found.is_none() {
        let why = match &configured {
            Some(path) => format!(
                "WIRECATCH_TEST_FFMPEG is {}, which is not a file",
                path.display()
            ),
            None => "no ffmpeg on PATH and WIRECATCH_TEST_FFMPEG is not set".to_string(),
        };
        let required = std::env::var("WIRECATCH_REQUIRE_FFMPEG").is_ok_and(|v| v == "1");
        assert!(!required, "WIRECATCH_REQUIRE_FFMPEG=1 but {why}");
        eprintln!("skipped: {why}");
    }
    found
}

fn options() -> DownloadOptions {
    let mut options = DownloadOptions::default();
    options.stall_timeout = Duration::from_secs(5);
    options
}

async fn run(request: StreamRequest) -> Result<Outcome, Error> {
    download(
        request,
        Arc::new(StreamProgress::default()),
        CancellationToken::new(),
    )
    .await
}

/// A master playlist with video and separate audio, two segments each.
fn video_and_audio(server: &FileServer) {
    for (prefix, seed) in [("v", 1), ("a", 2)] {
        let mut text = String::from("#EXTM3U\n");
        for i in 0..2 {
            server.put(&format!("{prefix}/{i}.ts"), ts_segment(seed * 10 + i, 1500));
            text.push_str(&format!("#EXTINF:2,\n{i}.ts\n"));
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
}

/// A folder name full of characters a shell or ffmpeg would treat specially.
fn hostile_dir(test: &str) -> PathBuf {
    // Windows does not allow `"` in names; other systems get it too.
    let name = if cfg!(windows) {
        "-y it's & $HOME `x` (1);"
    } else {
        "-y it's \"odd\" & $HOME `x` (1);"
    };
    let dir = fresh_dir(test).join(name);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[tokio::test]
async fn ffmpeg_gets_each_path_as_one_untouched_argument() {
    let Some(ffmpeg) = fake_ffmpeg() else { return };
    let server = FileServer::start().await;
    video_and_audio(&server);
    let dir = hostile_dir("ffmpeg-args");
    let output = dir.join("-i evil name.mp4");
    let mut request = StreamRequest::new(server.url("master.m3u8"), &output);
    request.options = options();
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
            "-f",
            "mp4",
            &file(work.join("joined.mp4")),
        ]
    );
    assert!(!work.exists(), "temporary files are removed");
}

#[tokio::test]
async fn ffmpeg_failures_are_reported_and_the_parts_kept() {
    let Some(ffmpeg) = fake_ffmpeg() else { return };
    let server = FileServer::start().await;
    video_and_audio(&server);
    let dir = fresh_dir("ffmpeg-fail");
    let output = dir.join("fail-please.mp4");
    let mut request = StreamRequest::new(server.url("master.m3u8"), &output);
    request.options = options();
    request.ffmpeg = Some(ffmpeg.clone());
    let error = run(request.clone()).await.unwrap_err();
    assert!(matches!(error, Error::Ffmpeg(_)), "{error}");
    let message = error.to_string();
    assert!(
        message.starts_with(
            "ffmpeg could not join the video: as MP4: [mp4 @ 0x1] fake ffmpeg failed / Conversion failed!; as Matroska: [mp4 @ 0x1] fake ffmpeg failed"
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
        .filter(|s| s.path.ends_with(".ts"))
        .map(|s| s.path.clone())
        .collect();
    assert!(fetched_again.is_empty(), "{fetched_again:?}");
}

/// Writes a short HLS stream with a real ffmpeg (stream copy friendly codecs:
/// MPEG-4 Part 2 video and AAC audio, both native ffmpeg encoders).
fn generate(ffmpeg: &Path, dir: &Path, input: &str, codec: &[&str], name: &str) {
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
        .status()
        .unwrap();
    assert!(
        status.success(),
        "ffmpeg could not generate the {name} fixture"
    );
}

/// What a real ffmpeg reports about a file's streams.
fn probe(ffmpeg: &Path, file: &Path) -> String {
    let output = Command::new(ffmpeg)
        .arg("-hide_banner")
        .arg("-i")
        .arg(file)
        .output()
        .unwrap();
    String::from_utf8_lossy(&output.stderr).into_owned()
}

fn serve_folder(server: &FileServer, dir: &Path) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let entry = entry.unwrap();
        let name = entry.file_name().to_string_lossy().into_owned();
        server.put(&name, std::fs::read(entry.path()).unwrap());
    }
}

#[tokio::test]
async fn real_ffmpeg_merges_separate_video_and_audio_into_mp4() {
    let Some(ffmpeg) = real_ffmpeg() else { return };
    let source = fresh_dir("real-ffmpeg-source");
    generate(
        &ffmpeg,
        &source,
        "testsrc=duration=3:size=160x120:rate=10",
        // MPEG-4 Part 2 needs its VOL header repeated in each segment.
        &["-c:v", "mpeg4", "-bsf:v", "dump_extra"],
        "v",
    );
    generate(&ffmpeg, &source, "sine=duration=3", &["-c:a", "aac"], "a");
    let server = FileServer::start().await;
    serve_folder(&server, &source);
    server.put_typed(
        "master.m3u8",
        "#EXTM3U\n#EXT-X-MEDIA:TYPE=AUDIO,GROUP-ID=\"aud\",NAME=\"Main\",DEFAULT=YES,URI=\"a.m3u8\"\n\
         #EXT-X-STREAM-INF:BANDWIDTH=200000,CODECS=\"mp4v.20.9,mp4a.40.2\",AUDIO=\"aud\"\nv.m3u8\n",
        MPEGURL,
    );
    let dir = hostile_dir("real-ffmpeg");
    let mut request = StreamRequest::new(server.url("master.m3u8"), dir.join("-out video.mp4"));
    request.options = options();
    request.ffmpeg = Some(ffmpeg.clone());
    let path = match run(request).await.unwrap() {
        Outcome::Completed { path, .. } => path,
        other => panic!("{other:?}"),
    };
    assert_eq!(path, dir.join("-out video.mp4"));
    let report = probe(&ffmpeg, &path);
    assert!(report.contains("Video: mpeg4"), "{report}");
    assert!(report.contains("Audio: aac"), "{report}");
}

#[tokio::test]
async fn real_ffmpeg_remuxes_a_single_ts_track() {
    let Some(ffmpeg) = real_ffmpeg() else { return };
    let source = fresh_dir("real-ffmpeg-single-source");
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
    serve_folder(&server, &source);
    let dir = fresh_dir("real-ffmpeg-single");
    let mut request = StreamRequest::new(server.url("index.m3u8"), dir.join("clip"));
    request.options = options();
    request.ffmpeg = Some(ffmpeg.clone());
    let path = match run(request).await.unwrap() {
        Outcome::Completed { path, .. } => path,
        other => panic!("{other:?}"),
    };
    assert_eq!(path, dir.join("clip.mp4"));
    let report = probe(&ffmpeg, &path);
    assert!(
        report.contains("Video: mpeg4") && report.contains("Audio: aac"),
        "{report}"
    );
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
    let Some(ffmpeg) = fake_ffmpeg() else { return };
    let server = FileServer::start().await;
    // An ffconcat script would make ffmpeg read whatever files it names.
    one_segment(&server, "ffconcat version 1.0\nfile secret.mp4\n");
    let dir = fresh_dir("ffmpeg-unknown");
    let output = dir.join("clip.mp4");
    let mut request = StreamRequest::new(server.url("index.m3u8"), &output);
    request.options = options();
    request.ffmpeg = Some(ffmpeg);
    let error = run(request).await.unwrap_err();
    assert!(
        matches!(error, Error::UnknownFormat { role: "video" }),
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
    assert!(dir.join("clip.mp4.wcstream").join("track0").is_file());
}

#[tokio::test]
async fn real_ffmpeg_does_not_follow_a_manifest_disguised_as_a_segment() {
    let Some(ffmpeg) = real_ffmpeg() else { return };
    // A file elsewhere on the computer that the "segment" tries to pull in.
    let elsewhere = fresh_dir("real-ffmpeg-elsewhere");
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
    // A DASH manifest naming that file. The leading comment puts `free` at
    // bytes 4 to 8, so the segment looks like MP4 and passes the format check;
    // ffmpeg left to guess would read it as DASH and copy the file in.
    let target = secret.to_string_lossy().replace('\\', "/");
    let payload = format!(
        "<!--free--><MPD xmlns=\"urn:mpeg:dash:schema:mpd:2011\" \
         profiles=\"urn:mpeg:dash:profile:isoff-on-demand:2011\" type=\"static\" \
         mediaPresentationDuration=\"PT1S\" minBufferTime=\"PT1S\"><Period>\
         <AdaptationSet mimeType=\"video/mp4\" contentType=\"video\">\
         <Representation id=\"1\" bandwidth=\"1000\" mimeType=\"video/mp4\" codecs=\"mp4v.20.9\">\
         <BaseURL>file:{target}</BaseURL><SegmentBase/></Representation>\
         </AdaptationSet></Period></MPD>\n"
    );
    let server = FileServer::start().await;
    one_segment(&server, payload);
    let dir = fresh_dir("real-ffmpeg-disguised");
    let output = dir.join("clip.mp4");
    let mut request = StreamRequest::new(server.url("index.m3u8"), &output);
    request.options = options();
    request.ffmpeg = Some(ffmpeg);
    let result = run(request).await;
    assert!(
        matches!(result, Err(Error::Ffmpeg(_))),
        "ffmpeg must refuse the segment: {result:?}"
    );
    for name in ["clip.mp4", "clip.mkv"] {
        assert!(!dir.join(name).exists(), "{name} was written");
    }
}
