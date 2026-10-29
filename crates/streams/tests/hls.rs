//! HLS downloads end to end, from a local server.

use std::fmt::Write as _;
use std::sync::Arc;
use std::time::Duration;

use aes::Aes128;
use cbc::cipher::block_padding::Pkcs7;
use cbc::cipher::{BlockEncryptMut, KeyIvInit};
use wirecatch_engine::header::{COOKIE, HeaderValue};
use wirecatch_engine::numeric::approx_f64;
use wirecatch_engine::units::MIB_USIZE;
use wirecatch_engine::{CancellationToken, RateLimiter};
use wirecatch_streams::{
    Choice, Error, KeyProblem, Outcome, PrivateNetwork, Role, Stage, StreamKind, StreamProgress,
    download, inspect,
};
use wirecatch_test_support::files::FileServer;
use wirecatch_test_support::{PATIENCE, TestResult, private_dir, ts_segment};

mod common;
use common::{options, request, run};

/// The bytes of a playlist's segments, in order.
type Segments = Vec<Vec<u8>>;

const MPEGURL: &str = "application/vnd.apple.mpegurl";

fn completed(outcome: Outcome) -> (std::path::PathBuf, u64) {
    match outcome {
        Outcome::Completed { path, bytes, .. } => (path, bytes),
        other => panic!("expected completion, got {other:?}"),
    }
}

/// Waits until `count` segments of the download `task` runs are done; fails if it ends first or takes longer than [`PATIENCE`].
async fn segments_done(
    progress: &StreamProgress,
    task: &tokio::task::JoinHandle<Result<Outcome, Error>>,
    count: usize,
) {
    let deadline = tokio::time::Instant::now() + PATIENCE;
    while progress.segments_done() < count {
        assert!(
            !task.is_finished(),
            "the download ended with {} of {count} segments done",
            progress.segments_done()
        );
        assert!(
            tokio::time::Instant::now() < deadline,
            "{} of {count} segments done after {PATIENCE:?}",
            progress.segments_done()
        );
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
}

/// Serves a media playlist of `count` segments under `prefix`, returning
/// the segments' bytes.
fn media_playlist(
    server: &FileServer,
    prefix: &str,
    count: usize,
    seed: u64,
) -> TestResult<Segments> {
    let mut text = String::from("#EXTM3U\n#EXT-X-VERSION:3\n#EXT-X-TARGETDURATION:4\n");
    let mut segments = Vec::new();
    for i in 0..count {
        let data = ts_segment(seed * 1000 + i as u64, 3000 + i * 17);
        server.put(&format!("{prefix}/seg{i}.ts"), data.clone());
        write!(text, "#EXTINF:4.0,\nseg{i}.ts\n")?;
        segments.push(data);
    }
    text.push_str("#EXT-X-ENDLIST\n");
    server.put_typed(&format!("{prefix}/index.m3u8"), text, MPEGURL);
    Ok(segments)
}

/// Serves a master playlist of two qualities, returning each one's segments.
fn master(server: &FileServer) -> TestResult<(Segments, Segments)> {
    let low = media_playlist(server, "low", 3, 1)?;
    let high = media_playlist(server, "high", 3, 2)?;
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
    Ok((low, high))
}

/// Track ids were list positions, and core keeps the chosen id to continue after a restart. A refreshed master playlist that lists its variants in another order then silently gave another quality.
#[tokio::test]
async fn a_chosen_quality_survives_a_playlist_that_lists_variants_in_another_order() {
    let server = FileServer::start().await;
    let (low, _high) = master(&server).unwrap();
    let info = inspect(
        &server.url("master.m3u8"),
        &options(),
        PrivateNetwork::Auto,
        CancellationToken::new(),
    )
    .await
    .unwrap();
    let chosen = info
        .video
        .iter()
        .find(|v| v.height == Some(360))
        .unwrap()
        .id
        .clone();

    // The refreshed playlist lists the qualities the other way round.
    server.put_typed(
        "master.m3u8",
        "#EXTM3U\n\
         #EXT-X-STREAM-INF:BANDWIDTH=3000000,RESOLUTION=1920x1080,CODECS=\"avc1.640028,mp4a.40.2\"\n\
         high/index.m3u8\n\
         #EXT-X-STREAM-INF:BANDWIDTH=500000,RESOLUTION=640x360,CODECS=\"avc1.4d401e,mp4a.40.2\",FRAME-RATE=25\n\
         low/index.m3u8\n",
        MPEGURL,
    );
    let dir = private_dir("hls-reordered").unwrap();
    let mut req = request(server.url("master.m3u8"), dir.join("v.mp4"));
    req.video = Choice::Track(chosen);
    let (path, _) = completed(run(req).await.unwrap());

    assert_eq!(
        std::fs::read(path).unwrap(),
        low.concat(),
        "still the 360p quality"
    );
    dir.remove().unwrap();
}

#[tokio::test]
async fn master_playlists_are_inspected_and_the_chosen_variant_downloaded() {
    let server = FileServer::start().await;
    let (low, high) = master(&server).unwrap();

    let info = inspect(
        &server.url("master.m3u8"),
        &options(),
        PrivateNetwork::Auto,
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
    let dir = private_dir("hls-auto").unwrap();
    let (path, bytes) = completed(
        run(request(server.url("master.m3u8"), dir.join("auto.mp4")))
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
    let mut req = request(server.url("master.m3u8"), dir.join("low.mp4"));
    req.video = Choice::Track(info.video[0].id.clone());
    let (path, _) = completed(run(req).await.unwrap());
    assert_eq!(std::fs::read(path).unwrap(), low.concat());

    let mut req = request(server.url("master.m3u8"), dir.join("x.mp4"));
    req.video = Choice::Track("v9".into());
    assert!(matches!(
        run(req).await,
        Err(Error::NoSuchTrack {
            kind: Role::Video,
            ..
        })
    ));
    dir.remove().unwrap();
}

fn encrypt(plain: &[u8], key: &[u8; 16], iv: &[u8; 16]) -> Vec<u8> {
    cbc::Encryptor::<Aes128>::new(key.into(), iv.into()).encrypt_padded_vec_mut::<Pkcs7>(plain)
}

#[tokio::test]
async fn aes_128_segments_are_decrypted_with_explicit_and_sequence_ivs() {
    let server = FileServer::start().await;
    let key1: [u8; 16] = *b"0123456789abcdef";
    let key2: [u8; 16] = *b"fedcba9876543210";
    let explicit = 0xa0a1_a2a3_a4a5_a6a7_a8a9_aaab_acad_aeaf_u128.to_be_bytes();
    server.put("keys/1", key1.to_vec());
    server.put("keys/2", key2.to_vec());
    let plain: Vec<Vec<u8>> = (0..5u16)
        .map(|i| ts_segment(50 + u64::from(i), 1000 + 333 * usize::from(i)))
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
    let dir = private_dir("hls-aes").unwrap();
    let (path, _) = completed(
        run(request(server.url("enc/index.m3u8"), dir.join("v.ts")))
            .await
            .unwrap(),
    );
    assert_eq!(std::fs::read(path).unwrap(), plain.concat());
    assert_eq!(server.requests("keys/1"), 1, "each key is fetched once");
    assert_eq!(server.requests("keys/2"), 1);
    dir.remove().unwrap();
}

#[tokio::test]
async fn a_wrong_key_is_a_clear_error() {
    let server = FileServer::start().await;
    server.put("k", b"0123456789abcdef".to_vec());
    server.put("short", b"short".to_vec());
    server.put(
        "s0.ts",
        encrypt(&ts_segment(1, 500), b"another key 1234", &[0; 16]),
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
    let dir = private_dir("hls-wrong-key").unwrap();
    let error = run(request(server.url("wrong.m3u8"), dir.join("a.ts")))
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
    let error = run(request(server.url("short.m3u8"), dir.join("b.ts")))
        .await
        .unwrap_err();
    assert!(
        matches!(
            error,
            Error::Key {
                problem: KeyProblem::Length(5),
                ..
            }
        ),
        "{error}"
    );
    dir.remove().unwrap();
}

#[tokio::test]
async fn byte_ranges_and_init_sections_rebuild_the_file() {
    let server = FileServer::start().await;
    // One fMP4 file holding an init section and three fragments.
    let mut file = b"\0\0\0\x10ftypisom\0\0\0\0".to_vec();
    file.extend(ts_segment(9, 700));
    let init_len = file.len();
    let fragments = [
        ts_segment(10, 1000),
        ts_segment(11, 1500),
        ts_segment(12, 800),
    ];
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
    let dir = private_dir("hls-ranges").unwrap();
    let (path, _) = completed(
        run(request(server.url("fmp4/index.m3u8"), dir.join("v.mp4")))
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
    dir.remove().unwrap();
}

#[tokio::test]
async fn parallel_downloads_join_in_playlist_order() {
    let server = FileServer::start().await;
    let segments = media_playlist(&server, "many", 40, 3).unwrap();
    // Uneven answer times, so segments finish out of order.
    for i in (0..40).step_by(3) {
        server.delay(&format!("many/seg{i}.ts"), Duration::from_millis(60));
    }
    let dir = private_dir("hls-parallel").unwrap();
    let mut req = request(server.url("many/index.m3u8"), dir.join("v.ts"));
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
    dir.remove().unwrap();
}

#[tokio::test]
async fn resuming_after_a_cancel_fetches_only_the_missing_segments() {
    let server = FileServer::start().await;
    let segments = media_playlist(&server, "r", 30, 4).unwrap();
    // The first ten come; the rest hang until the resume, so the cancel always leaves twenty.
    for i in 10..30 {
        server.hang(&format!("r/seg{i}.ts"), true);
    }
    let dir = private_dir("hls-resume").unwrap();
    let output = dir.join("v.mp4");
    let mut req = request(server.url("r/index.m3u8"), &output);
    req.options.connections = 2;

    let progress = Arc::new(StreamProgress::default());
    let cancel = CancellationToken::new();
    let task = tokio::spawn(download(req.clone(), progress.clone(), cancel.clone()));
    segments_done(&progress, &task, 10).await;
    cancel.cancel();
    let left = match task.await.unwrap().unwrap() {
        Outcome::Paused { segments_left, .. } => segments_left,
        other => panic!("{other:?}"),
    };
    assert_eq!(left, 20, "the hung segments are left");
    assert!(
        dir.join("v.mp4.wcstream").is_dir(),
        "finished segments are kept"
    );
    let before = server.seen().len();
    for i in 10..30 {
        server.hang(&format!("r/seg{i}.ts"), false);
    }

    let progress = Arc::new(StreamProgress::default());
    let outcome = download(req, progress.clone(), CancellationToken::new())
        .await
        .unwrap();
    let (path, _) = completed(outcome);
    assert_eq!(std::fs::read(path).unwrap(), segments.concat());
    let again: Vec<_> = server.seen()[before..]
        .iter()
        .filter(|s| s.has_extension("ts"))
        .map(|s| s.path.clone())
        .collect();
    assert_eq!(again.len(), left, "only the missing segments: {again:?}");
    let mut unique = again.clone();
    unique.sort();
    unique.dedup();
    assert_eq!(unique.len(), again.len());
    dir.remove().unwrap();
}

/// Signed links carry a fresh token in every segment query. Refreshing the link, which the errors for an expired link ask for, used to change the plan's fingerprint and delete every segment already downloaded.
#[tokio::test]
async fn resuming_with_a_refreshed_link_keeps_the_segments() {
    let server = FileServer::start().await;
    let segments = media_playlist(&server, "t", 30, 5).unwrap();
    for i in 0..30 {
        server.delay(&format!("t/seg{i}.ts"), Duration::from_millis(40));
    }
    let signed = |token: &str| {
        let mut text = String::from("#EXTM3U\n#EXT-X-VERSION:3\n#EXT-X-TARGETDURATION:4\n");
        for i in 0..30 {
            write!(text, "#EXTINF:4.0,\nseg{i}.ts?token={token}\n").unwrap();
        }
        text.push_str("#EXT-X-ENDLIST\n");
        text
    };
    server.put_typed("t/index.m3u8", signed("first"), MPEGURL);
    let dir = private_dir("hls-refreshed").unwrap();
    let output = dir.join("v.mp4");
    let mut req = request(server.url("t/index.m3u8"), &output);
    req.options.connections = 2;

    let progress = Arc::new(StreamProgress::default());
    let cancel = CancellationToken::new();
    let task = tokio::spawn(download(req.clone(), progress.clone(), cancel.clone()));
    segments_done(&progress, &task, 10).await;
    cancel.cancel();
    let left = match task.await.unwrap().unwrap() {
        Outcome::Paused { segments_left, .. } => segments_left,
        other => panic!("{other:?}"),
    };
    let before = server.seen().len();

    // The person refreshes the link: every segment now carries another token.
    server.put_typed("t/index.m3u8", signed("second"), MPEGURL);
    let outcome = download(
        req,
        Arc::new(StreamProgress::default()),
        CancellationToken::new(),
    )
    .await
    .unwrap();
    let (path, _) = completed(outcome);
    assert_eq!(std::fs::read(path).unwrap(), segments.concat());
    let fetched = server.seen()[before..]
        .iter()
        .filter(|s| s.has_extension("ts"))
        .count();
    assert_eq!(fetched, left, "only the missing segments are fetched again");
    dir.remove().unwrap();
}

#[tokio::test]
async fn resuming_checks_segment_sizes() {
    let server = FileServer::start().await;
    let segments = media_playlist(&server, "s", 4, 5).unwrap();
    let dir = private_dir("hls-sizes").unwrap();
    let output = dir.join("v.ts");
    // Pause while the server holds the last segment, leaving three finished.
    let req = request(server.url("s/index.m3u8"), &output);
    let progress = Arc::new(StreamProgress::default());
    let cancel = CancellationToken::new();
    server.hang("s/seg3.ts", true);
    let task = tokio::spawn(download(req.clone(), progress.clone(), cancel.clone()));
    segments_done(&progress, &task, 3).await;
    cancel.cancel();
    assert!(matches!(
        task.await.unwrap().unwrap(),
        Outcome::Paused {
            segments_left: 1,
            ..
        }
    ));

    server.hang("s/seg3.ts", false);

    // Damage one finished segment: it must be fetched again.
    let work = dir.join("v.ts.wcstream").join("t0");
    std::fs::write(work.join("000001.seg"), b"truncated").unwrap();
    let before = server.seen().len();
    let (path, _) = completed(run(req).await.unwrap());
    assert_eq!(std::fs::read(path).unwrap(), segments.concat());
    let mut again: Vec<_> = server.seen()[before..]
        .iter()
        .filter(|s| s.has_extension("ts"))
        .map(|s| s.path.clone())
        .collect();
    again.sort();
    assert_eq!(again, ["/s/seg1.ts", "/s/seg3.ts"]);
    dir.remove().unwrap();
}

#[tokio::test]
async fn live_streams_are_refused_with_a_plain_message() {
    let server = FileServer::start().await;
    server.put("live/seg0.ts", ts_segment(1, 100));
    server.put_typed(
        "live/index.m3u8",
        "#EXTM3U\n#EXT-X-TARGETDURATION:4\n#EXT-X-MEDIA-SEQUENCE:100\n#EXTINF:4,\nseg0.ts\n",
        MPEGURL,
    );
    let info = inspect(
        &server.url("live/index.m3u8"),
        &options(),
        PrivateNetwork::Auto,
        CancellationToken::new(),
    )
    .await
    .unwrap();
    assert!(info.live);
    let dir = private_dir("hls-live").unwrap();
    let error = run(request(server.url("live/index.m3u8"), dir.join("v.ts")))
        .await
        .unwrap_err();
    assert!(matches!(error, Error::Live));
    assert_eq!(
        error.to_string(),
        "This video is a live broadcast. Wirecatch can download it after the broadcast ends."
    );
    assert_eq!(server.requests("live/seg0.ts"), 0);
    dir.remove().unwrap();
}

#[tokio::test]
async fn credentials_stay_with_the_playlist_origin() {
    let site = FileServer::start().await;
    let cdn = FileServer::start().await;
    let key: [u8; 16] = *b"kkkkkkkkkkkkkkkk";
    cdn.put("key", key.to_vec());
    let plain = [ts_segment(1, 900), ts_segment(2, 900)];
    let mut text = String::from("#EXTM3U\n");
    writeln!(
        text,
        "#EXT-X-KEY:METHOD=AES-128,URI=\"{}\",IV=0x01",
        cdn.url("key")
    )
    .unwrap();
    for (i, p) in plain.iter().enumerate() {
        let mut iv = [0u8; 16];
        iv[15] = 1;
        cdn.put(&format!("seg{i}.ts"), encrypt(p, &key, &iv));
        write!(text, "#EXTINF:2,\n{}\n", cdn.url(&format!("seg{i}.ts"))).unwrap();
    }
    text.push_str("#EXT-X-ENDLIST\n");
    site.put_typed("watch/index.m3u8", text, MPEGURL);

    let dir = private_dir("hls-origin").unwrap();
    let mut req = request(site.url("watch/index.m3u8"), dir.join("v.ts"));
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
    dir.remove().unwrap();
}

#[tokio::test]
async fn a_failing_segment_is_retried_and_then_reported_clearly() {
    let server = FileServer::start().await;
    let segments = media_playlist(&server, "f", 5, 6).unwrap();
    server.fail("f/seg2.ts", 2, 503);
    let dir = private_dir("hls-fail").unwrap();
    let (path, _) = completed(
        run(request(server.url("f/index.m3u8"), dir.join("a.ts")))
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
    let mut req = request(server.url("f/index.m3u8"), dir.join("b.ts"));
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
    dir.remove().unwrap();
}

#[tokio::test]
async fn separate_audio_without_ffmpeg_is_refused_before_downloading() {
    let server = FileServer::start().await;
    media_playlist(&server, "v", 2, 7).unwrap();
    media_playlist(&server, "a", 2, 8).unwrap();
    server.put_typed(
        "av.m3u8",
        "#EXTM3U\n#EXT-X-MEDIA:TYPE=AUDIO,GROUP-ID=\"aud\",NAME=\"English\",LANGUAGE=\"en\",DEFAULT=YES,URI=\"a/index.m3u8\"\n\
         #EXT-X-STREAM-INF:BANDWIDTH=1000,AUDIO=\"aud\"\nv/index.m3u8\n",
        MPEGURL,
    );
    let info = inspect(
        &server.url("av.m3u8"),
        &options(),
        PrivateNetwork::Auto,
        CancellationToken::new(),
    )
    .await
    .unwrap();
    assert_eq!(info.audio.len(), 1);
    assert!(info.audio[0].default);
    let dir = private_dir("hls-noffmpeg").unwrap();
    let error = run(request(server.url("av.m3u8"), dir.join("v.mp4")))
        .await
        .unwrap_err();
    assert!(matches!(error, Error::NeedsFfmpeg));
    assert_eq!(
        error.to_string(),
        "Joining video and audio needs the ffmpeg add-on. Install it and try again."
    );
    assert!(!server.seen().iter().any(|s| s.has_extension("ts")));

    // Video alone, leaving the separate audio out, works without ffmpeg.
    let mut req = request(server.url("av.m3u8"), dir.join("v.mp4"));
    req.audio = Choice::Skip;
    completed(run(req).await.unwrap());
    dir.remove().unwrap();
}

#[tokio::test]
async fn links_that_are_not_streams_are_explained() {
    let server = FileServer::start().await;
    server.put_typed("page.html", "<html><body>hi</body></html>", "text/html");
    let error = inspect(
        &server.url("page.html"),
        &options(),
        PrivateNetwork::Auto,
        CancellationToken::new(),
    )
    .await
    .unwrap_err();
    assert!(matches!(error, Error::NotAStream), "{error}");
    let error = inspect(
        &server.url("missing.m3u8"),
        &options(),
        PrivateNetwork::Auto,
        CancellationToken::new(),
    )
    .await
    .unwrap_err();
    assert!(matches!(error, Error::Request(_)), "{error}");
    assert!(error.to_string().contains("404"), "{error}");
}

#[tokio::test]
async fn huge_playlists_are_refused_by_their_limits() {
    let server = FileServer::start().await;
    // 200 001 short segment lines: under the size limit, over the segment limit.
    let mut text = String::from("#EXTM3U\n");
    for _ in 0..=200_000 {
        text.push_str("#EXTINF:1,\ns.ts\n");
    }
    server.put_typed("many.m3u8", text, MPEGURL);
    // Over 8 MiB: refused by size before parsing.
    let mut big = String::from("#EXTM3U\n");
    while big.len() <= 8 * MIB_USIZE {
        big.push_str("#EXTINF:1,\nsome-rather-long-segment-name-to-fill-the-playlist.ts\n");
    }
    server.put_typed("big.m3u8", big, MPEGURL);

    // Each refusal is its own error, which only the limit can give; the bound
    // catches a parse that never ends, not a slow machine.
    let refusal = |playlist: &str| {
        let url = server.url(playlist);
        async move {
            let options = options();
            let inspected = inspect(
                &url,
                &options,
                PrivateNetwork::Auto,
                CancellationToken::new(),
            );
            tokio::time::timeout(PATIENCE, inspected)
                .await
                .expect("refused within PATIENCE")
                .unwrap_err()
        }
    };
    let error = refusal("many.m3u8").await;
    assert!(matches!(error, Error::TooManySegments), "{error}");
    let error = refusal("big.m3u8").await;
    assert!(
        matches!(
            error,
            Error::Request(wirecatch_engine::Error::TooLarge { .. })
        ),
        "{error}"
    );
}

#[tokio::test]
async fn the_extension_is_known_in_advance() {
    let server = FileServer::start().await;
    master(&server).unwrap();
    let info = inspect(
        &server.url("master.m3u8"),
        &options(),
        PrivateNetwork::Auto,
        CancellationToken::new(),
    )
    .await
    .unwrap();
    assert_eq!(info.extension(&Choice::Auto, &Choice::Auto, false), "ts");
    assert_eq!(info.extension(&Choice::Auto, &Choice::Auto, true), "mp4");

    // fMP4 segments (with an init section) joined as they are make an MP4.
    server.put("f/init.mp4", b"\0\0\0\x10ftypisom\0\0\0\0".to_vec());
    server.put("f/a.m4s", ts_segment(1, 100));
    server.put_typed(
        "f/index.m3u8",
        "#EXTM3U\n#EXT-X-MAP:URI=\"init.mp4\"\n#EXTINF:2,\na.m4s\n#EXT-X-ENDLIST\n",
        MPEGURL,
    );
    let info = inspect(
        &server.url("f/index.m3u8"),
        &options(),
        PrivateNetwork::Auto,
        CancellationToken::new(),
    )
    .await
    .unwrap();
    assert_eq!(info.extension(&Choice::Auto, &Choice::Auto, false), "mp4");
    let dir = private_dir("hls-extension").unwrap();
    let (path, _) = completed(
        run(request(server.url("f/index.m3u8"), dir.join("clip")))
            .await
            .unwrap(),
    );
    assert_eq!(
        path,
        dir.join("clip.mp4"),
        "the forecast matches the result"
    );
    dir.remove().unwrap();
}

#[tokio::test]
async fn a_changed_extension_never_replaces_another_file() {
    let server = FileServer::start().await;
    let segments = media_playlist(&server, "p", 2, 9).unwrap();
    let dir = private_dir("hls-no-overwrite").unwrap();
    // The caller asked for .mp4; without ffmpeg the result is .ts, and a
    // different file already has that name.
    std::fs::write(dir.join("video.ts"), b"precious").unwrap();
    std::fs::write(dir.join("video (1).ts"), b"also precious").unwrap();
    let (path, _) = completed(
        run(request(server.url("p/index.m3u8"), dir.join("video.mp4")))
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
        run(request(server.url("p/index.m3u8"), dir.join("exact.ts")))
            .await
            .unwrap(),
    );
    assert_eq!(path, dir.join("exact.ts"));
    assert_eq!(std::fs::read(&path).unwrap(), segments.concat());
    dir.remove().unwrap();
}

#[tokio::test]
async fn streams_from_the_internet_cannot_reach_the_local_network() {
    let site = FileServer::start().await;
    let device = FileServer::start().await;
    device.put("key", b"0123456789abcdef".to_vec());
    device.put("seg.ts", ts_segment(1, 100));
    site.put("ok.ts", ts_segment(2, 100));
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
    let dir = private_dir("hls-private").unwrap();
    for (i, text) in playlists.iter().enumerate() {
        site.put_typed(&format!("p{i}.m3u8"), text.clone(), MPEGURL);
        let mut req = request(site.url(&format!("p{i}.m3u8")), dir.join(format!("{i}.ts")));
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
    let auto = request(site.url("p1.m3u8"), dir.join("auto.ts"));
    let error = run(auto).await.unwrap_err();
    assert!(matches!(error, Error::Decrypt { .. }), "{error}");
    assert_eq!(device.requests("key"), 1);
    dir.remove().unwrap();
}

#[tokio::test]
async fn no_more_than_the_configured_connections_are_used() {
    let server = FileServer::start().await;
    let segments = media_playlist(&server, "c", 12, 11).unwrap();
    // Slow answers keep every connection busy, so the limit is reached.
    for i in 0..12 {
        server.delay(&format!("c/seg{i}.ts"), Duration::from_millis(80));
    }
    let dir = private_dir("hls-connections").unwrap();
    let mut req = request(server.url("c/index.m3u8"), dir.join("v.ts"));
    req.options.connections = 3;
    let (path, _) = completed(run(req).await.unwrap());
    assert_eq!(std::fs::read(path).unwrap(), segments.concat());
    assert_eq!(
        server.max_active(),
        3,
        "exactly the configured number at most"
    );
    dir.remove().unwrap();
}

#[tokio::test]
async fn speed_limits_apply_to_streams() {
    let server = FileServer::start().await;
    // Ten segments of 30 kB: 300 kB in all.
    let mut text = String::from("#EXTM3U\n");
    let mut segments = Vec::new();
    for i in 0..10 {
        let data = ts_segment(500 + i, 30_000);
        server.put(&format!("r/{i}.ts"), data.clone());
        write!(text, "#EXTINF:2,\n{i}.ts\n").unwrap();
        segments.push(data);
    }
    text.push_str("#EXT-X-ENDLIST\n");
    server.put_typed("r/index.m3u8", text, MPEGURL);
    let total = u64::try_from(segments.iter().map(Vec::len).sum::<usize>()).unwrap();
    let dir = private_dir("hls-rate").unwrap();

    // 200 kB/s shared by all four connections: 300 kB takes 1.5 s, minus at
    // most the limiter's quarter-second burst. Only this lower bound is
    // checked: a busy machine makes any download slower, never a limited one
    // faster.
    let mut req = request(server.url("r/index.m3u8"), dir.join("slow.ts"));
    req.options.connections = 4;
    let limiter = Arc::new(RateLimiter::new(Some(200_000)));
    req.options.rate_limiters = vec![limiter];
    let start = std::time::Instant::now();
    let (path, _) = completed(run(req).await.unwrap());
    let capped = start.elapsed();
    assert_eq!(std::fs::read(path).unwrap(), segments.concat());
    let expected = Duration::from_secs_f64(approx_f64(total) / 200_000.0);
    assert!(
        capped >= expected.mul_f64(0.75),
        "{total} bytes took {capped:?} at 200 kB/s"
    );
    dir.remove().unwrap();
}

/// A segment duration too large for `Duration` (about 1.8e19 seconds) once panicked while inspecting, and release builds abort on panic, which took down the whole app.
#[tokio::test]
async fn an_absurd_segment_duration_is_unknown_not_a_crash() {
    let server = FileServer::start().await;
    server.put("huge/seg0.ts", ts_segment(1, 100));
    server.put_typed(
        "huge/index.m3u8",
        "#EXTM3U\n#EXT-X-TARGETDURATION:4\n#EXTINF:1e300,\nseg0.ts\n#EXT-X-ENDLIST\n",
        MPEGURL,
    );
    let info = inspect(
        &server.url("huge/index.m3u8"),
        &options(),
        PrivateNetwork::Auto,
        CancellationToken::new(),
    )
    .await
    .unwrap();
    assert_eq!(info.duration, None);
}

// 1e18 seconds fits in a Duration, so it did not panic, but the video showed as billions of years
// long and the size estimate was absurd.
#[tokio::test]
async fn a_segment_longer_than_a_day_counts_as_unknown() {
    let server = FileServer::start().await;
    server.put("long/seg0.ts", ts_segment(1, 100));
    server.put("long/seg1.ts", ts_segment(2, 100));
    server.put_typed(
        "long/index.m3u8",
        "#EXTM3U\n#EXT-X-TARGETDURATION:4\n#EXTINF:1e18,\nseg0.ts\n#EXTINF:4,\nseg1.ts\n#EXT-X-ENDLIST\n",
        MPEGURL,
    );
    let info = inspect(
        &server.url("long/index.m3u8"),
        &options(),
        PrivateNetwork::Auto,
        CancellationToken::new(),
    )
    .await
    .unwrap();
    assert_eq!(info.duration, Some(Duration::from_secs(4)));
}
