mod common;

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use aes::Aes128;
use cbc::cipher::block_padding::Pkcs7;
use cbc::cipher::{BlockEncryptMut, KeyIvInit};
use common::{Server, segment, temp_dir};
use wirecatch_engine::header::{COOKIE, HeaderValue};
use wirecatch_engine::{CancellationToken, DownloadOptions};
use wirecatch_streams::{
    Choice, Error, Outcome, PrivateNetwork, Stage, StreamKind, StreamProgress, StreamRequest,
    download, inspect,
};

const MPEGURL: Option<&str> = Some("application/vnd.apple.mpegurl");

fn options() -> DownloadOptions {
    let mut options = DownloadOptions::default();
    options.stall_timeout = Duration::from_secs(5);
    options.max_retries = 3;
    options
}

fn request(server: &Server, playlist: &str, output: &Path) -> StreamRequest {
    let mut request = StreamRequest::new(server.url(playlist), output);
    request.options = options();
    request
}

async fn run(request: StreamRequest) -> Result<Outcome, Error> {
    download(
        request,
        Arc::new(StreamProgress::default()),
        CancellationToken::new(),
    )
    .await
}

fn completed(outcome: Outcome) -> (std::path::PathBuf, u64) {
    match outcome {
        Outcome::Completed { path, bytes, .. } => (path, bytes),
        other => panic!("expected completion, got {other:?}"),
    }
}

/// Serves a media playlist of `count` segments under `prefix`, returning
/// the segments' bytes.
fn media_playlist(server: &Server, prefix: &str, count: usize, seed: u64) -> Vec<Vec<u8>> {
    let mut text = String::from("#EXTM3U\n#EXT-X-VERSION:3\n#EXT-X-TARGETDURATION:4\n");
    let mut segments = Vec::new();
    for i in 0..count {
        let data = segment(seed * 1000 + i as u64, 3000 + i * 17);
        server.put(&format!("{prefix}/seg{i}.ts"), data.clone());
        text.push_str(&format!("#EXTINF:4.0,\nseg{i}.ts\n"));
        segments.push(data);
    }
    text.push_str("#EXT-X-ENDLIST\n");
    server.put_typed(&format!("{prefix}/index.m3u8"), text, MPEGURL);
    segments
}

fn master(server: &Server) -> (Vec<Vec<u8>>, Vec<Vec<u8>>) {
    let low = media_playlist(server, "low", 3, 1);
    let high = media_playlist(server, "high", 3, 2);
    server.put_typed(
        "master.m3u8",
        "#EXTM3U\n\
         #EXT-X-STREAM-INF:BANDWIDTH=500000,RESOLUTION=640x360,CODECS=\"avc1.4d401e,mp4a.40.2\",FRAME-RATE=25\n\
         low/index.m3u8\n\
         #EXT-X-STREAM-INF:BANDWIDTH=3000000,RESOLUTION=1920x1080,CODECS=\"avc1.640028,mp4a.40.2\"\n\
         high/index.m3u8\n\
         #EXT-X-MEDIA:TYPE=SUBTITLES,GROUP-ID=\"s\",LANGUAGE=\"de\",NAME=\"Deutsch\",URI=\"subs/de.m3u8\"\n",
        MPEGURL,
    );
    (low, high)
}

#[tokio::test]
async fn master_playlists_are_inspected_and_the_chosen_variant_downloaded() {
    let server = Server::start().await;
    let (low, high) = master(&server);

    let info = inspect(
        &server.url("master.m3u8"),
        &options(),
        CancellationToken::new(),
    )
    .await
    .unwrap();
    assert_eq!(info.kind, StreamKind::Hls);
    assert!(!info.live);
    assert_eq!(info.duration, Some(Duration::from_secs(12)));
    assert_eq!(info.video.len(), 2);
    assert_eq!(info.video[0].height, Some(360));
    assert_eq!(info.video[0].frame_rate, Some(25.0));
    assert_eq!(info.video[1].bandwidth, Some(3_000_000));
    assert_eq!(info.subtitles.len(), 1);
    assert_eq!(info.subtitles[0].language.as_deref(), Some("de"));

    // Automatic choice: the best quality.
    let dir = temp_dir("hls-auto");
    let (path, bytes) = completed(
        run(request(&server, "master.m3u8", &dir.join("auto.mp4")))
            .await
            .unwrap(),
    );
    assert_eq!(
        path,
        dir.join("auto.ts"),
        "no ffmpeg: the segments joined as they are"
    );
    assert_eq!(std::fs::read(&path).unwrap(), high.concat());
    assert_eq!(bytes, high.concat().len() as u64);
    assert!(
        !dir.join("auto.mp4.wcstream").exists(),
        "temporary files are removed"
    );

    // An explicit choice.
    let mut req = request(&server, "master.m3u8", &dir.join("low.mp4"));
    req.video = Choice::Track(info.video[0].id.clone());
    let (path, _) = completed(run(req).await.unwrap());
    assert_eq!(std::fs::read(path).unwrap(), low.concat());

    let mut req = request(&server, "master.m3u8", &dir.join("x.mp4"));
    req.video = Choice::Track("v9".into());
    assert!(matches!(
        run(req).await,
        Err(Error::NoSuchTrack { kind: "video", .. })
    ));
}

fn encrypt(plain: &[u8], key: &[u8; 16], iv: &[u8; 16]) -> Vec<u8> {
    cbc::Encryptor::<Aes128>::new_from_slices(key, iv)
        .unwrap()
        .encrypt_padded_vec_mut::<Pkcs7>(plain)
}

#[tokio::test]
async fn aes_128_segments_are_decrypted_with_explicit_and_sequence_ivs() {
    let server = Server::start().await;
    let key1: [u8; 16] = *b"0123456789abcdef";
    let key2: [u8; 16] = *b"fedcba9876543210";
    let explicit: [u8; 16] = core::array::from_fn(|i| 0xA0 + i as u8);
    server.put("keys/1", key1.to_vec());
    server.put("keys/2", key2.to_vec());
    let plain: Vec<Vec<u8>> = (0..5)
        .map(|i| segment(50 + i, 1000 + 333 * i as usize))
        .collect();
    // Media sequence 40: segments 0 and 1 use IVs 40 and 41; 2 and 3 use the
    // explicit IV with the second key; 4 is clear.
    let sequence_iv = |n: u64| {
        let mut iv = [0u8; 16];
        iv[8..].copy_from_slice(&n.to_be_bytes());
        iv
    };
    server.put("enc/s0.ts", encrypt(&plain[0], &key1, &sequence_iv(40)));
    server.put("enc/s1.ts", encrypt(&plain[1], &key1, &sequence_iv(41)));
    server.put("enc/s2.ts", encrypt(&plain[2], &key2, &explicit));
    server.put("enc/s3.ts", encrypt(&plain[3], &key2, &explicit));
    server.put("enc/s4.ts", plain[4].clone());
    server.put_typed(
        "enc/index.m3u8",
        "#EXTM3U\n#EXT-X-MEDIA-SEQUENCE:40\n\
         #EXT-X-KEY:METHOD=AES-128,URI=\"/keys/1\"\n#EXTINF:2,\ns0.ts\n#EXTINF:2,\ns1.ts\n\
         #EXT-X-KEY:METHOD=AES-128,URI=\"../keys/2\",IV=0xA0A1A2A3A4A5A6A7A8A9AAABACADAEAF\n#EXTINF:2,\ns2.ts\n#EXTINF:2,\ns3.ts\n\
         #EXT-X-KEY:METHOD=NONE\n#EXTINF:2,\ns4.ts\n#EXT-X-ENDLIST\n",
        MPEGURL,
    );
    let dir = temp_dir("hls-aes");
    let (path, _) = completed(
        run(request(&server, "enc/index.m3u8", &dir.join("v.ts")))
            .await
            .unwrap(),
    );
    assert_eq!(std::fs::read(path).unwrap(), plain.concat());
    assert_eq!(server.requests("keys/1"), 1, "each key is fetched once");
    assert_eq!(server.requests("keys/2"), 1);
}

#[tokio::test]
async fn a_wrong_key_is_a_clear_error() {
    let server = Server::start().await;
    server.put("k", b"0123456789abcdef".to_vec());
    server.put("short", b"short".to_vec());
    server.put(
        "s0.ts",
        encrypt(&segment(1, 500), b"another key 1234", &[0; 16]),
    );
    server.put_typed(
        "wrong.m3u8",
        "#EXTM3U\n#EXT-X-KEY:METHOD=AES-128,URI=\"k\"\n#EXTINF:2,\ns0.ts\n#EXT-X-ENDLIST\n",
        MPEGURL,
    );
    server.put_typed(
        "short.m3u8",
        "#EXTM3U\n#EXT-X-KEY:METHOD=AES-128,URI=\"short\"\n#EXTINF:2,\ns0.ts\n#EXT-X-ENDLIST\n",
        MPEGURL,
    );
    let dir = temp_dir("hls-wrong-key");
    let error = run(request(&server, "wrong.m3u8", &dir.join("a.ts")))
        .await
        .unwrap_err();
    assert!(
        matches!(
            error,
            Error::Decrypt {
                number: 1,
                total: 1
            }
        ),
        "{error}"
    );
    assert!(error.to_string().contains("could not be decrypted"));
    let error = run(request(&server, "short.m3u8", &dir.join("b.ts")))
        .await
        .unwrap_err();
    assert!(matches!(error, Error::Key { .. }), "{error}");
    assert!(
        error.to_string().contains("5 bytes long instead of 16"),
        "{error}"
    );
}

#[tokio::test]
async fn byte_ranges_and_init_sections_rebuild_the_file() {
    let server = Server::start().await;
    // One fMP4 file holding an init section and three fragments.
    let mut file = b"\0\0\0\x10ftypisom\0\0\0\0".to_vec();
    file.extend(segment(9, 700));
    let init_len = file.len();
    let fragments = [segment(10, 1000), segment(11, 1500), segment(12, 800)];
    for f in &fragments {
        file.extend(f);
    }
    server.put("fmp4/all.mp4", file.clone());
    let (a, b) = (init_len + 1000, init_len + 2500);
    server.put_typed(
        "fmp4/index.m3u8",
        format!(
            "#EXTM3U\n#EXT-X-VERSION:7\n#EXT-X-MAP:URI=\"all.mp4\",BYTERANGE=\"{init_len}@0\"\n\
             #EXTINF:2,\n#EXT-X-BYTERANGE:1000@{init_len}\nall.mp4\n\
             #EXTINF:2,\n#EXT-X-BYTERANGE:1500\nall.mp4\n\
             #EXTINF:2,\n#EXT-X-BYTERANGE:800@{b}\nall.mp4\n#EXT-X-ENDLIST\n"
        ),
        MPEGURL,
    );
    let dir = temp_dir("hls-ranges");
    let (path, _) = completed(
        run(request(&server, "fmp4/index.m3u8", &dir.join("v.mp4")))
            .await
            .unwrap(),
    );
    assert_eq!(path, dir.join("v.mp4"), "fMP4 is recognized");
    assert_eq!(std::fs::read(&path).unwrap(), file);
    let ranges: Vec<String> = server
        .seen()
        .iter()
        .filter_map(|s| s.header("range").map(str::to_string))
        .collect();
    let mut ranges = ranges;
    ranges.sort();
    let mut expected = vec![
        format!("bytes=0-{}", init_len - 1),
        format!("bytes={init_len}-{}", a - 1),
        format!("bytes={a}-{}", b - 1),
        format!("bytes={b}-{}", b + 799),
    ];
    expected.sort();
    assert_eq!(ranges, expected);
}

#[tokio::test]
async fn parallel_downloads_join_in_playlist_order() {
    let server = Server::start().await;
    let segments = media_playlist(&server, "many", 40, 3);
    // Uneven answer times, so segments finish out of order.
    for i in (0..40).step_by(3) {
        server.delay(&format!("many/seg{i}.ts"), Duration::from_millis(60));
    }
    let dir = temp_dir("hls-parallel");
    let mut req = request(&server, "many/index.m3u8", &dir.join("v.ts"));
    req.options.connections = 8;
    let progress = Arc::new(StreamProgress::default());
    let outcome = download(req, progress.clone(), CancellationToken::new())
        .await
        .unwrap();
    let (path, _) = completed(outcome);
    assert_eq!(std::fs::read(path).unwrap(), segments.concat());
    assert!(server.max_active() > 1, "segments were fetched in parallel");
    assert!(server.max_active() <= 8);
    assert_eq!(progress.segments_done(), 40);
    assert_eq!(progress.segments_total(), 40);
    assert_eq!(progress.bytes(), segments.concat().len() as u64);
    assert_eq!(progress.stage(), Stage::Finished);
}

#[tokio::test]
async fn resuming_after_a_cancel_fetches_only_the_missing_segments() {
    let server = Server::start().await;
    let segments = media_playlist(&server, "r", 30, 4);
    for i in 0..30 {
        server.delay(&format!("r/seg{i}.ts"), Duration::from_millis(40));
    }
    let dir = temp_dir("hls-resume");
    let output = dir.join("v.mp4");
    let mut req = request(&server, "r/index.m3u8", &output);
    req.options.connections = 2;

    let progress = Arc::new(StreamProgress::default());
    let cancel = CancellationToken::new();
    let task = tokio::spawn(download(req.clone(), progress.clone(), cancel.clone()));
    while progress.segments_done() < 10 {
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    cancel.cancel();
    let left = match task.await.unwrap().unwrap() {
        Outcome::Paused { segments_left, .. } => segments_left,
        other => panic!("{other:?}"),
    };
    assert!((1..=20).contains(&left), "{left} left");
    assert!(
        dir.join("v.mp4.wcstream").is_dir(),
        "finished segments are kept"
    );
    let before = server.seen().len();

    let progress = Arc::new(StreamProgress::default());
    let outcome = download(req, progress.clone(), CancellationToken::new())
        .await
        .unwrap();
    let (path, _) = completed(outcome);
    assert_eq!(std::fs::read(path).unwrap(), segments.concat());
    let again: Vec<_> = server.seen()[before..]
        .iter()
        .filter(|s| s.path.ends_with(".ts"))
        .map(|s| s.path.clone())
        .collect();
    assert_eq!(again.len(), left, "only the missing segments: {again:?}");
    let mut unique = again.clone();
    unique.sort();
    unique.dedup();
    assert_eq!(unique.len(), again.len());
}

#[tokio::test]
async fn resuming_checks_segment_sizes() {
    let server = Server::start().await;
    let segments = media_playlist(&server, "s", 4, 5);
    let dir = temp_dir("hls-sizes");
    let output = dir.join("v.ts");
    // Pause while the last segment is still on its way, leaving three finished.
    let req = request(&server, "s/index.m3u8", &output);
    let progress = Arc::new(StreamProgress::default());
    let cancel = CancellationToken::new();
    server.delay("s/seg3.ts", Duration::from_millis(400));
    let task = tokio::spawn(download(req.clone(), progress.clone(), cancel.clone()));
    while progress.segments_done() < 3 {
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    cancel.cancel();
    assert!(matches!(
        task.await.unwrap().unwrap(),
        Outcome::Paused {
            segments_left: 1,
            ..
        }
    ));

    // Damage one finished segment: it must be fetched again.
    let work = dir.join("v.ts.wcstream").join("t0");
    std::fs::write(work.join("000001.seg"), b"truncated").unwrap();
    let before = server.seen().len();
    let (path, _) = completed(run(req).await.unwrap());
    assert_eq!(std::fs::read(path).unwrap(), segments.concat());
    let mut again: Vec<_> = server.seen()[before..]
        .iter()
        .filter(|s| s.path.ends_with(".ts"))
        .map(|s| s.path.clone())
        .collect();
    again.sort();
    assert_eq!(again, ["/s/seg1.ts", "/s/seg3.ts"]);
}

#[tokio::test]
async fn live_streams_are_refused_with_a_plain_message() {
    let server = Server::start().await;
    server.put("live/seg0.ts", segment(1, 100));
    server.put_typed(
        "live/index.m3u8",
        "#EXTM3U\n#EXT-X-TARGETDURATION:4\n#EXT-X-MEDIA-SEQUENCE:100\n#EXTINF:4,\nseg0.ts\n",
        MPEGURL,
    );
    let info = inspect(
        &server.url("live/index.m3u8"),
        &options(),
        CancellationToken::new(),
    )
    .await
    .unwrap();
    assert!(info.live);
    let dir = temp_dir("hls-live");
    let error = run(request(&server, "live/index.m3u8", &dir.join("v.ts")))
        .await
        .unwrap_err();
    assert!(matches!(error, Error::Live));
    assert_eq!(
        error.to_string(),
        "This video is a live broadcast. Wirecatch can download it after the broadcast ends."
    );
    assert_eq!(server.requests("live/seg0.ts"), 0);
}

#[tokio::test]
async fn credentials_stay_with_the_playlist_origin() {
    let site = Server::start().await;
    let cdn = Server::start().await;
    let key: [u8; 16] = *b"kkkkkkkkkkkkkkkk";
    cdn.put("key", key.to_vec());
    let plain = [segment(1, 900), segment(2, 900)];
    let mut text = String::from("#EXTM3U\n");
    text.push_str(&format!(
        "#EXT-X-KEY:METHOD=AES-128,URI=\"{}\",IV=0x01\n",
        cdn.url("key")
    ));
    for (i, p) in plain.iter().enumerate() {
        let mut iv = [0u8; 16];
        iv[15] = 1;
        cdn.put(&format!("seg{i}.ts"), encrypt(p, &key, &iv));
        text.push_str(&format!("#EXTINF:2,\n{}\n", cdn.url(&format!("seg{i}.ts"))));
    }
    text.push_str("#EXT-X-ENDLIST\n");
    site.put_typed("watch/index.m3u8", text, MPEGURL);

    let dir = temp_dir("hls-origin");
    let mut req = request(&site, "watch/index.m3u8", &dir.join("v.ts"));
    req.options
        .headers
        .insert(COOKIE, HeaderValue::from_static("session=secret"));
    let (path, _) = completed(run(req).await.unwrap());
    assert_eq!(std::fs::read(path).unwrap(), plain.concat());

    assert_eq!(site.seen()[0].header("cookie"), Some("session=secret"));
    let at_cdn = cdn.seen();
    assert_eq!(at_cdn.len(), 3, "a key and two segments");
    for seen in at_cdn {
        assert_eq!(seen.header("cookie"), None, "{} got the cookie", seen.path);
    }
}

#[tokio::test]
async fn a_failing_segment_is_retried_and_then_reported_clearly() {
    let server = Server::start().await;
    let segments = media_playlist(&server, "f", 5, 6);
    server.fail("f/seg2.ts", 2, 503);
    let dir = temp_dir("hls-fail");
    let (path, _) = completed(
        run(request(&server, "f/index.m3u8", &dir.join("a.ts")))
            .await
            .unwrap(),
    );
    assert_eq!(std::fs::read(path).unwrap(), segments.concat());
    assert_eq!(
        server.requests("f/seg2.ts"),
        3,
        "two failures, then success"
    );

    server.fail("f/seg2.ts", 1000, 500);
    let before = server.requests("f/seg2.ts");
    let mut req = request(&server, "f/index.m3u8", &dir.join("b.ts"));
    req.options.max_retries = 2;
    let error = run(req).await.unwrap_err();
    assert!(
        matches!(
            error,
            Error::Segment {
                number: 3,
                total: 5,
                ..
            }
        ),
        "{error}"
    );
    assert_eq!(
        server.requests("f/seg2.ts") - before,
        3,
        "first try and two retries"
    );
    let message = error.to_string();
    assert!(
        message.starts_with("Part 3 of 5 of the video could not be downloaded"),
        "{message}"
    );
    assert!(message.contains("500"), "{message}");
}

#[tokio::test]
async fn separate_audio_without_ffmpeg_is_refused_before_downloading() {
    let server = Server::start().await;
    media_playlist(&server, "v", 2, 7);
    media_playlist(&server, "a", 2, 8);
    server.put_typed(
        "av.m3u8",
        "#EXTM3U\n#EXT-X-MEDIA:TYPE=AUDIO,GROUP-ID=\"aud\",NAME=\"English\",LANGUAGE=\"en\",DEFAULT=YES,URI=\"a/index.m3u8\"\n\
         #EXT-X-STREAM-INF:BANDWIDTH=1000,AUDIO=\"aud\"\nv/index.m3u8\n",
        MPEGURL,
    );
    let info = inspect(&server.url("av.m3u8"), &options(), CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(info.audio.len(), 1);
    assert!(info.audio[0].default);
    let dir = temp_dir("hls-noffmpeg");
    let error = run(request(&server, "av.m3u8", &dir.join("v.mp4")))
        .await
        .unwrap_err();
    assert!(matches!(error, Error::NeedsFfmpeg));
    assert_eq!(
        error.to_string(),
        "Joining video and audio needs the ffmpeg add-on. Install it and try again."
    );
    assert!(!server.seen().iter().any(|s| s.path.ends_with(".ts")));

    // Video alone, leaving the separate audio out, works without ffmpeg.
    let mut req = request(&server, "av.m3u8", &dir.join("v.mp4"));
    req.audio = Choice::Skip;
    completed(run(req).await.unwrap());
}

#[tokio::test]
async fn links_that_are_not_streams_are_explained() {
    let server = Server::start().await;
    server.put_typed(
        "page.html",
        "<html><body>hi</body></html>",
        Some("text/html"),
    );
    let error = inspect(
        &server.url("page.html"),
        &options(),
        CancellationToken::new(),
    )
    .await
    .unwrap_err();
    assert!(matches!(error, Error::NotAStream), "{error}");
    let error = inspect(
        &server.url("missing.m3u8"),
        &options(),
        CancellationToken::new(),
    )
    .await
    .unwrap_err();
    assert!(matches!(error, Error::Request(_)), "{error}");
    assert!(error.to_string().contains("404"), "{error}");
}

#[tokio::test]
async fn huge_playlists_are_refused_quickly_and_clearly() {
    let server = Server::start().await;
    // 200 001 short segment lines: under the size limit, over the segment limit.
    let mut text = String::from("#EXTM3U\n");
    for _ in 0..=200_000 {
        text.push_str("#EXTINF:1,\ns.ts\n");
    }
    server.put_typed("many.m3u8", text, MPEGURL);
    // Over 8 MiB: refused by size before parsing.
    let mut big = String::from("#EXTM3U\n");
    while big.len() <= 8 * 1024 * 1024 {
        big.push_str("#EXTINF:1,\nsome-rather-long-segment-name-to-fill-the-playlist.ts\n");
    }
    server.put_typed("big.m3u8", big, MPEGURL);

    let start = std::time::Instant::now();
    let error = inspect(
        &server.url("many.m3u8"),
        &options(),
        CancellationToken::new(),
    )
    .await
    .unwrap_err();
    assert!(matches!(error, Error::Manifest(_)), "{error}");
    assert!(
        error.to_string().contains("more than 200000 segments"),
        "{error}"
    );
    let error = inspect(
        &server.url("big.m3u8"),
        &options(),
        CancellationToken::new(),
    )
    .await
    .unwrap_err();
    assert!(
        matches!(
            error,
            Error::Request(wirecatch_engine::Error::TooLarge { .. })
        ),
        "{error}"
    );
    assert!(start.elapsed() < Duration::from_secs(10));
}

#[tokio::test]
async fn the_extension_is_known_in_advance() {
    let server = Server::start().await;
    master(&server);
    let info = inspect(
        &server.url("master.m3u8"),
        &options(),
        CancellationToken::new(),
    )
    .await
    .unwrap();
    assert_eq!(info.extension(&Choice::Auto, &Choice::Auto, false), "ts");
    assert_eq!(info.extension(&Choice::Auto, &Choice::Auto, true), "mp4");

    // fMP4 segments (with an init section) joined as they are make an MP4.
    server.put("f/init.mp4", b"\0\0\0\x10ftypisom\0\0\0\0".to_vec());
    server.put("f/a.m4s", segment(1, 100));
    server.put_typed(
        "f/index.m3u8",
        "#EXTM3U\n#EXT-X-MAP:URI=\"init.mp4\"\n#EXTINF:2,\na.m4s\n#EXT-X-ENDLIST\n",
        MPEGURL,
    );
    let info = inspect(
        &server.url("f/index.m3u8"),
        &options(),
        CancellationToken::new(),
    )
    .await
    .unwrap();
    assert_eq!(info.extension(&Choice::Auto, &Choice::Auto, false), "mp4");
    let dir = temp_dir("hls-extension");
    let (path, _) = completed(
        run(request(&server, "f/index.m3u8", &dir.join("clip")))
            .await
            .unwrap(),
    );
    assert_eq!(
        path,
        dir.join("clip.mp4"),
        "the forecast matches the result"
    );
}

#[tokio::test]
async fn a_changed_extension_never_replaces_another_file() {
    let server = Server::start().await;
    let segments = media_playlist(&server, "p", 2, 9);
    let dir = temp_dir("hls-no-overwrite");
    // The caller asked for .mp4; without ffmpeg the result is .ts, and a
    // different file already has that name.
    std::fs::write(dir.join("video.ts"), b"precious").unwrap();
    std::fs::write(dir.join("video (1).ts"), b"also precious").unwrap();
    let (path, _) = completed(
        run(request(&server, "p/index.m3u8", &dir.join("video.mp4")))
            .await
            .unwrap(),
    );
    assert_eq!(path, dir.join("video (2).ts"));
    assert_eq!(std::fs::read(&path).unwrap(), segments.concat());
    assert_eq!(std::fs::read(dir.join("video.ts")).unwrap(), b"precious");
    assert_eq!(
        std::fs::read(dir.join("video (1).ts")).unwrap(),
        b"also precious"
    );

    // A file at exactly the requested path is the caller's to replace.
    std::fs::write(dir.join("exact.ts"), b"old").unwrap();
    let (path, _) = completed(
        run(request(&server, "p/index.m3u8", &dir.join("exact.ts")))
            .await
            .unwrap(),
    );
    assert_eq!(path, dir.join("exact.ts"));
    assert_eq!(std::fs::read(&path).unwrap(), segments.concat());
}

#[tokio::test]
async fn streams_from_the_internet_cannot_reach_the_local_network() {
    let site = Server::start().await;
    let device = Server::start().await;
    device.put("key", b"0123456789abcdef".to_vec());
    device.put("seg.ts", segment(1, 100));
    site.put("ok.ts", segment(2, 100));
    let playlists = [
        // A segment at a private address written as an IP.
        "#EXTM3U\n#EXTINF:2,\nhttp://192.168.1.20/seg.ts\n#EXT-X-ENDLIST\n".to_string(),
        // A key on another local origin (the device).
        format!(
            "#EXTM3U\n#EXT-X-KEY:METHOD=AES-128,URI=\"{}\"\n#EXTINF:2,\nok.ts\n#EXT-X-ENDLIST\n",
            device.url("key")
        ),
        // An init section, reached through a name for the local machine.
        format!(
            "#EXTM3U\n#EXT-X-MAP:URI=\"http://localhost:{}/seg.ts\"\n#EXTINF:2,\nok.ts\n#EXT-X-ENDLIST\n",
            device.url("x").port().unwrap()
        ),
    ];
    let dir = temp_dir("hls-private");
    for (i, text) in playlists.iter().enumerate() {
        site.put_typed(&format!("p{i}.m3u8"), text.clone(), MPEGURL);
        let mut req = request(&site, &format!("p{i}.m3u8"), &dir.join(format!("{i}.ts")));
        // The test servers run on 127.0.0.1; `Block` makes the site count as
        // one on the internet, as `Auto` does for a real website.
        req.private_network = PrivateNetwork::Block;
        let error = run(req).await.unwrap_err();
        assert!(matches!(error, Error::PrivateNetwork), "{i}: {error}");
        assert_eq!(
            error.to_string(),
            "This video points to a device on your own network, which Wirecatch blocks for safety."
        );
    }
    assert!(device.seen().is_empty(), "the device was never contacted");

    // With the default (`Auto`) a playlist on the local network itself may
    // point to other local addresses, as a media server at home does.
    let auto = request(&site, "p1.m3u8", &dir.join("auto.ts"));
    let error = run(auto).await.unwrap_err();
    assert!(matches!(error, Error::Decrypt { .. }), "{error}");
    assert_eq!(device.requests("key"), 1);
}
