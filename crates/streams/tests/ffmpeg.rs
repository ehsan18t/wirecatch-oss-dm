mod common;

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::time::Duration;

use common::{Server, segment, temp_dir};
use wirecatch_engine::{CancellationToken, DownloadOptions};
use wirecatch_streams::{Error, Outcome, StreamProgress, StreamRequest, download};

const MPEGURL: Option<&str> = Some("application/vnd.apple.mpegurl");

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
fn real_ffmpeg() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("WIRECATCH_TEST_FFMPEG").map(PathBuf::from)
        && path.is_file()
    {
        return Some(path);
    }
    let name = format!("ffmpeg{}", std::env::consts::EXE_SUFFIX);
    let found = std::env::var_os("PATH").and_then(|paths| {
        std::env::split_paths(&paths)
            .map(|d| d.join(&name))
            .find(|p| p.is_file())
    });
    if found.is_none() {
        eprintln!("skipped: no ffmpeg on PATH and WIRECATCH_TEST_FFMPEG is not set");
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
fn video_and_audio(server: &Server) {
    for (prefix, seed) in [("v", 1), ("a", 2)] {
        let mut text = String::from("#EXTM3U\n");
        for i in 0..2 {
            server.put(&format!("{prefix}/{i}.ts"), segment(seed * 10 + i, 1500));
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
    let dir = temp_dir(test).join(name);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[tokio::test]
async fn ffmpeg_gets_each_path_as_one_untouched_argument() {
    let Some(ffmpeg) = fake_ffmpeg() else { return };
    let server = Server::start().await;
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
            "-i",
            &file(work.join("track0")),
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
    let server = Server::start().await;
    video_and_audio(&server);
    let dir = temp_dir("ffmpeg-fail");
    let output = dir.join("fail-please.mp4");
    let mut request = StreamRequest::new(server.url("master.m3u8"), &output);
    request.options = options();
    request.ffmpeg = Some(ffmpeg.clone());
    let error = run(request.clone()).await.unwrap_err();
    assert!(matches!(error, Error::Ffmpeg(_)), "{error}");
    let message = error.to_string();
    assert!(
        message.starts_with(
            "ffmpeg could not join the video: [mp4 @ 0x1] fake ffmpeg failed / Conversion failed!"
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

fn serve_folder(server: &Server, dir: &Path) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let entry = entry.unwrap();
        let name = entry.file_name().to_string_lossy().into_owned();
        server.put(&name, std::fs::read(entry.path()).unwrap());
    }
}

#[tokio::test]
async fn real_ffmpeg_merges_separate_video_and_audio_into_mp4() {
    let Some(ffmpeg) = real_ffmpeg() else { return };
    let source = temp_dir("real-ffmpeg-source");
    generate(
        &ffmpeg,
        &source,
        "testsrc=duration=3:size=160x120:rate=10",
        // MPEG-4 Part 2 needs its VOL header repeated in each segment.
        &["-c:v", "mpeg4", "-bsf:v", "dump_extra"],
        "v",
    );
    generate(&ffmpeg, &source, "sine=duration=3", &["-c:a", "aac"], "a");
    let server = Server::start().await;
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
    let source = temp_dir("real-ffmpeg-single-source");
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
    let server = Server::start().await;
    serve_folder(&server, &source);
    let dir = temp_dir("real-ffmpeg-single");
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
