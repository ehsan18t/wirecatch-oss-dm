//! Media captures: streams and pages become a quality choice, the choice becomes
//! a download, separate video and audio are joined, and page media is read again
//! after a restart.

use std::path::Path;

use wirecatch_core::CoreError;
use wirecatch_protocol::app::{
    AddonState, Category, DownloadStatus, FixAction, MediaChoice, MediaSelection, UiEvent,
};
use wirecatch_protocol::extension::{AddAction, MediaKind};

use crate::common::{Harness, test_data, wait_until};
use crate::media_support::{
    FileServer, Releases, args, harness, install, media_capture, next_event, publish, real_ffmpeg,
    tell_tool, ytdlp_calls,
};

fn select(option: &str) -> MediaSelection {
    MediaSelection {
        option_id: option.into(),
        folder: None,
        file_name: None,
        queue: None,
        later: false,
    }
}

/// `len` bytes of test data that start like an MP4 file, so the join hands
/// them to (the fake) ffmpeg instead of refusing them as an unknown format.
fn mp4_data(len: usize) -> Vec<u8> {
    let mut data = test_data(len);
    data[..8].copy_from_slice(b"\0\0\0\x18ftyp");
    data
}

async fn choice_requested(ui: &mut tokio::sync::broadcast::Receiver<UiEvent>) -> MediaChoice {
    next_event(ui, "a media choice", |e| match e {
        UiEvent::MediaChoiceRequested(c) => Some(c.clone()),
        _ => None,
    })
    .await
}

const SEGMENT: usize = 50 * 1024;

/// An HLS stream: a master playlist with 720p and 360p, each two segments, and
/// optionally separate audio. Returns the 720p segments' bytes.
fn hls(server: &FileServer, live: bool, separate_audio: bool) -> Vec<u8> {
    let audio = if separate_audio {
        "#EXT-X-MEDIA:TYPE=AUDIO,GROUP-ID=\"aud\",NAME=\"English\",DEFAULT=YES,URI=\"audio/index.m3u8\"\n"
    } else {
        ""
    };
    let group = if separate_audio { ",AUDIO=\"aud\"" } else { "" };
    server.put(
        "hls/master.m3u8",
        format!(
            "#EXTM3U\n{audio}#EXT-X-STREAM-INF:BANDWIDTH=2000000,RESOLUTION=1280x720{group}\n720/index.m3u8\n#EXT-X-STREAM-INF:BANDWIDTH=800000,RESOLUTION=640x360{group}\n360/index.m3u8\n"
        ),
    );
    let end = if live { "" } else { "#EXT-X-ENDLIST\n" };
    let mut best = Vec::new();
    for (n, quality) in ["720", "360", "audio"].iter().enumerate() {
        server.put(
            &format!("hls/{quality}/index.m3u8"),
            format!(
                "#EXTM3U\n#EXT-X-VERSION:3\n#EXT-X-TARGETDURATION:4\n#EXT-X-MEDIA-SEQUENCE:0\n#EXTINF:4.0,\nseg0.ts\n#EXTINF:4.0,\nseg1.ts\n{end}"
            ),
        );
        for seg in 0..2 {
            let data: Vec<u8> = test_data(SEGMENT)
                .into_iter()
                .map(|b| b.wrapping_add((n * 2 + seg) as u8))
                .collect();
            if n == 0 {
                best.extend_from_slice(&data);
            }
            server.put(&format!("hls/{quality}/seg{seg}.ts"), data);
        }
    }
    best
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_hls_capture_becomes_a_choice_and_then_a_file() {
    let server = FileServer::start().await;
    let best = hls(&server, false, false);
    let h = Harness::new().await;
    let mut ui = h.manager.subscribe();
    let capture = media_capture(
        &server.url("hls/master.m3u8"),
        MediaKind::Hls,
        Some("Holiday clip"),
    );
    let reply = h.manager.add_capture(capture.clone()).await.unwrap();
    assert_eq!(reply.action, AddAction::AwaitingConfirmation);
    assert!(reply.id.is_none());

    // The UI hears that the stream is being read, then gets the choice under the same id.
    let (reading, url) = next_event(&mut ui, "MediaAnalysing", |e| match e {
        UiEvent::MediaAnalysing { id, url } => Some((*id, url.clone())),
        _ => None,
    })
    .await;
    assert!(url.ends_with("hls/master.m3u8"), "{url}");
    let choice = choice_requested(&mut ui).await;
    assert_eq!(choice.id, reading);
    assert_eq!(choice.title, "Holiday clip");
    let labels: Vec<&str> = choice.options.iter().map(|o| o.label.as_str()).collect();
    assert_eq!(labels, ["720p, TS", "360p, TS"]);
    assert!(choice.options.iter().all(|o| !o.needs_ffmpeg));
    assert_eq!(choice.options[0].height, Some(720));
    assert_eq!(h.manager.pending_media_choices(), vec![choice.clone()]);
    // The same capture again joins the choice already waiting.
    let again = h.manager.add_capture(capture).await.unwrap();
    assert_eq!(again.action, AddAction::AwaitingConfirmation);
    assert_eq!(h.manager.pending_media_choices().len(), 1);

    h.manager
        .choose_media(choice.id, select(&choice.options[0].id))
        .await
        .unwrap();
    let closed = next_event(&mut ui, "the choice closes", |e| match e {
        UiEvent::ConfirmationClosed { id } => Some(*id),
        _ => None,
    })
    .await;
    assert_eq!(closed, choice.id);
    assert!(h.manager.pending_media_choices().is_empty());
    let row = h.manager.list_downloads().remove(0);
    assert_eq!(row.category, Category::Video);
    let media = row.media.clone().unwrap();
    assert_eq!(
        (media.quality.as_str(), media.source),
        ("720p", MediaKind::Hls)
    );
    let done = h.wait_status(row.id, DownloadStatus::Completed).await;
    assert!(
        done.file_name.starts_with("Holiday clip"),
        "{}",
        done.file_name
    );
    assert_eq!(std::fs::read(&done.path).unwrap(), best);
    assert_eq!(done.done, best.len() as u64);
    // The browser's cookies went to the playlist's own server.
    let seen = server.requests("hls/master.m3u8");
    assert!(seen.iter().any(|s| s.headers.get("cookie").is_some()));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn joining_separate_audio_needs_the_add_on() {
    let server = FileServer::start().await;
    hls(&server, false, true);
    let h = Harness::new().await;
    let mut ui = h.manager.subscribe();
    let capture = media_capture(&server.url("hls/master.m3u8"), MediaKind::Hls, None);
    h.manager.add_capture(capture).await.unwrap();
    let choice = choice_requested(&mut ui).await;
    let video = &choice.options[0];
    assert!(video.needs_ffmpeg, "{video:?}");
    assert_eq!(video.label, "720p, MP4");
    let audio = choice.options.iter().find(|o| o.audio_only).unwrap();
    assert!(!audio.needs_ffmpeg);

    let error = h
        .manager
        .choose_media(choice.id, select(&video.id))
        .await
        .unwrap_err();
    assert!(matches!(error, CoreError::AddonRequired), "{error}");
    let (addon, url, id) = next_event(&mut ui, "AddonRequired", |e| match e {
        UiEvent::AddonRequired { addon, url, id } => Some((addon.clone(), url.clone(), *id)),
        _ => None,
    })
    .await;
    assert_eq!(id, choice.id, "the choice that waits for the add-on");

    assert_eq!(addon.state, AddonState::NotInstalled);
    assert!(url.ends_with("master.m3u8"));
    // The choice is still there to make again.
    assert_eq!(h.manager.pending_media_choices().len(), 1);
    // An unknown option is refused, and a dismissed choice is gone.
    assert!(
        h.manager
            .choose_media(choice.id, select("nope"))
            .await
            .is_err()
    );
    h.manager.dismiss_media_choice(choice.id).unwrap();
    assert!(h.manager.pending_media_choices().is_empty());
    assert!(matches!(
        h.manager.dismiss_media_choice(choice.id),
        Err(CoreError::ConfirmationNotFound(_))
    ));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_live_stream_says_so_in_the_list() {
    let server = FileServer::start().await;
    hls(&server, true, false);
    let h = Harness::new().await;
    let capture = media_capture(&server.url("hls/master.m3u8"), MediaKind::Hls, Some("Live"));
    h.manager.add_capture(capture).await.unwrap();
    let row = wait_until("the failed row", || h.manager.list_downloads().pop()).await;
    let row = h.wait_status(row.id, DownloadStatus::Failed).await;
    let error = row.error.unwrap();
    assert!(
        error.message.contains("live broadcast"),
        "{}",
        error.message
    );
    assert_eq!(error.fix, None);
    assert!(h.manager.pending_media_choices().is_empty());
    // An unfinished video stays where its parts are.
    let elsewhere = h.folders.home.join("Elsewhere");
    let moved = h
        .manager
        .set_download_folder(row.id, &elsewhere.to_string_lossy())
        .await;
    assert!(matches!(moved, Err(CoreError::WrongState(_))), "{moved:?}");
}

/// The fake yt-dlp's description of a page: 1080p video and audio as separate
/// files, and a 360p file with both.
fn page_info(server: &FileServer, video: &[u8], audio: &[u8]) -> String {
    server.put("fmt/video.mp4", video.to_vec());
    server.put("fmt/audio.m4a", audio.to_vec());
    server.put("fmt/small.mp4", b"small".to_vec());
    let headers = serde_json::json!({"User-Agent": "yt-dlp-agent", "X-Format": "yes"});
    serde_json::json!({
        "title": "Cats: the movie",
        "formats": [
            {"format_id": "140", "ext": "m4a", "vcodec": "none", "acodec": "mp4a.40.2", "abr": 128,
             "protocol": "https", "url": server.url("fmt/audio.m4a"), "filesize": audio.len(), "http_headers": headers},
            {"format_id": "137", "ext": "mp4", "vcodec": "avc1.640028", "acodec": "none", "height": 1080, "tbr": 4000,
             "protocol": "https", "url": server.url("fmt/video.mp4"), "filesize": video.len(), "http_headers": headers},
            {"format_id": "18", "ext": "mp4", "vcodec": "avc1", "acodec": "mp4a", "height": 360, "tbr": 500,
             "protocol": "https", "url": server.url("fmt/small.mp4")},
        ]
    })
    .to_string()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_page_is_read_with_yt_dlp_and_video_and_audio_are_joined() {
    let server = FileServer::start().await;
    let h = harness(publish(&server, &Releases::fake(), "r")).await;
    assert_eq!(install(&h).await.state, AddonState::Installed);
    let (video, audio) = (mp4_data(300 * 1024), mp4_data(100 * 1024));
    tell_tool(&h, "info.json", &page_info(&server, &video, &audio));
    // Long enough for the "Processing" status to be seen.
    tell_tool(&h, "ffmpeg-delay-ms", "800");
    let mut ui = h.manager.subscribe();
    let page = server.url("watch/1");
    h.manager
        .add_capture(media_capture(&page, MediaKind::Page, Some("ignored title")))
        .await
        .unwrap();
    let choice = choice_requested(&mut ui).await;
    assert_eq!(choice.title, "Cats: the movie");
    let labels: Vec<&str> = choice.options.iter().map(|o| o.label.as_str()).collect();
    assert_eq!(labels, ["1080p, MP4", "360p, MP4", "Audio only, M4A"]);
    let best = &choice.options[0];
    assert_eq!(best.id, "137+140");
    assert!(best.needs_ffmpeg);
    assert_eq!(best.size, Some((video.len() + audio.len()) as u64));

    // yt-dlp ran with an argument vector Wirecatch controls.
    let calls = ytdlp_calls(&h);
    assert_eq!(calls.len(), 1);
    let call = &calls[0];
    let argv = args(call);
    for expected in [
        "--ignore-config",
        "--no-playlist",
        "--no-js-runtimes",
        "-J",
        "--cookies",
    ] {
        assert!(
            argv.contains(&expected.to_string()),
            "{expected} missing from {argv:?}"
        );
    }
    let deno = argv
        .windows(2)
        .find(|w| w[0] == "--js-runtimes")
        .map(|w| w[1].clone())
        .unwrap();
    assert!(
        deno.starts_with("deno:") && Path::new(&deno[5..]).is_file(),
        "{deno}"
    );
    assert_eq!(argv[argv.len() - 2..], ["--".to_string(), page.clone()]);
    assert!(argv.contains(&"User-Agent:TestBrowser/2".to_string()));
    // The cookies went in a file bound to the page's host, readable only by
    // the user, and deleted right after.
    let cookies = call["cookies"].as_str().unwrap();
    assert!(cookies.starts_with("# Netscape HTTP Cookie File\n"));
    assert!(cookies.contains("127.0.0.1\tFALSE\t/\tFALSE\t"));
    assert!(cookies.contains("\tsession\tabc123\n") && cookies.contains("\ttheme\tdark\n"));
    if cfg!(unix) {
        assert_eq!(call["cookies_mode"].as_u64(), Some(0o600));
    }
    let cookies_path = call["cookies_path"].as_str().unwrap();
    assert!(
        !Path::new(cookies_path).exists(),
        "the cookies file was left behind"
    );

    h.manager
        .choose_media(choice.id, select("137+140"))
        .await
        .unwrap();
    let row = h.manager.list_downloads().remove(0);
    assert_eq!(row.media.as_ref().unwrap().quality, "1080p");
    h.wait_status(row.id, DownloadStatus::Processing).await;
    let done = h.wait_status(row.id, DownloadStatus::Completed).await;
    assert!(done.file_name.ends_with(".mp4"), "{}", done.file_name);
    // The fake ffmpeg wrote its arguments: a copy of both parts, no re-encoding.
    let joined = std::fs::read_to_string(&done.path).unwrap();
    let lines: Vec<&str> = joined.lines().collect();
    assert!(lines.windows(2).any(|w| w == ["-c", "copy"]), "{joined}");
    assert_eq!(lines.iter().filter(|l| **l == "-i").count(), 2);
    // Each input is read as the format it was recognized as, from a file only.
    assert_eq!(
        lines
            .windows(2)
            .filter(|w| w == &["-protocol_whitelist", "file"])
            .count(),
        2,
        "{joined}"
    );
    assert_eq!(
        lines.windows(2).filter(|w| w == &["-f", "mov"]).count(),
        2,
        "{joined}"
    );
    assert!(!Path::new(&format!("{}.wcmedia", done.path)).exists());
    // The format's own headers were sent with the format, never the page's cookies.
    for seen in server.requests("fmt/video.mp4") {
        assert_eq!(seen.headers.get("x-format").unwrap(), "yes");
        assert_eq!(seen.headers.get("user-agent").unwrap(), "yt-dlp-agent");
        assert!(seen.headers.get("cookie").is_none());
    }
    // The page was read again when the download started (addresses expire).
    assert_eq!(ytdlp_calls(&h).len(), 2);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn page_media_is_read_again_after_a_restart_and_continues() {
    let server = FileServer::start().await;
    let mut h = harness(publish(&server, &Releases::fake(), "r")).await;
    assert_eq!(install(&h).await.state, AddonState::Installed);
    let (video, audio) = (mp4_data(600 * 1024), mp4_data(20 * 1024));
    tell_tool(&h, "info.json", &page_info(&server, &video, &audio));
    server.throttle("fmt/video.mp4", Some(150 * 1024));
    let mut ui = h.manager.subscribe();
    h.manager
        .add_capture(media_capture(&server.url("watch/2"), MediaKind::Page, None))
        .await
        .unwrap();
    let choice = choice_requested(&mut ui).await;
    h.manager
        .choose_media(choice.id, select("137+140"))
        .await
        .unwrap();
    let id = h.manager.list_downloads()[0].id;
    h.wait_for(id, "part of the video", |r| r.done >= 150 * 1024)
        .await;
    h.restart().await;
    server.throttle("fmt/video.mp4", None);
    let done = h.wait_status(id, DownloadStatus::Completed).await;
    assert!(Path::new(&done.path).is_file());
    // Read at the choice, at the first start and again after the restart.
    assert!(ytdlp_calls(&h).len() >= 3, "{}", ytdlp_calls(&h).len());
    // The video continued where it stopped instead of starting over.
    let resumed = server.requests("fmt/video.mp4").iter().any(|s| {
        s.headers
            .get("range")
            .and_then(|r| r.to_str().ok())
            .is_some_and(|r| r != "bytes=0-")
    });
    assert!(resumed, "the video was downloaded again from the start");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_page_waits_for_the_add_on_and_continues_once_it_is_installed() {
    let server = FileServer::start().await;
    let h = harness(publish(&server, &Releases::fake(), "r")).await;
    tell_tool(&h, "info.json", &page_info(&server, b"v", b"a"));
    let mut ui = h.manager.subscribe();
    let page = server.url("watch/3");
    let reply = h
        .manager
        .add_capture(media_capture(&page, MediaKind::Page, None))
        .await
        .unwrap();
    assert_eq!(reply.action, AddAction::AwaitingConfirmation);
    let (addon, url, id) = next_event(&mut ui, "AddonRequired", |e| match e {
        UiEvent::AddonRequired { addon, url, id } => Some((addon.clone(), url.clone(), *id)),
        _ => None,
    })
    .await;
    // The id is the waiting page's, so the prompt can drop exactly that page.
    assert_eq!(h.manager.pending_addon_requests()[0].id, id);

    assert_eq!((addon.id.as_str(), url.as_str()), ("media", page.as_str()));
    assert!(addon.size.is_some_and(|s| s > 0));
    assert!(h.manager.pending_media_choices().is_empty());

    h.manager.install_addon("media").await.unwrap();
    let choice = choice_requested(&mut ui).await;
    assert_eq!(
        choice.page_url.as_deref(),
        Some("https://videos.example/watch")
    );
    assert_eq!(h.manager.pending_media_choices(), vec![choice]);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn formats_wirecatch_cannot_fetch_are_left_to_yt_dlp() {
    let server = FileServer::start().await;
    let h = harness(publish(&server, &Releases::fake(), "r")).await;
    assert_eq!(install(&h).await.state, AddonState::Installed);
    let info = serde_json::json!({
        "title": "Dash only",
        "formats": [
            {"format_id": "dash-v", "ext": "mp4", "vcodec": "avc1", "acodec": "none", "height": 720,
             "protocol": "http_dash_segments", "url": server.url("x.mpd")},
            {"format_id": "dash-a", "ext": "m4a", "vcodec": "none", "acodec": "mp4a",
             "protocol": "http_dash_segments", "url": server.url("x.mpd")},
        ]
    });
    tell_tool(&h, "info.json", &info.to_string());
    let mut ui = h.manager.subscribe();
    h.manager
        .add_capture(media_capture(&server.url("watch/4"), MediaKind::Page, None))
        .await
        .unwrap();
    let choice = choice_requested(&mut ui).await;
    assert_eq!(choice.options[0].id, "dash-v+dash-a");
    h.manager
        .choose_media(choice.id, select("dash-v+dash-a"))
        .await
        .unwrap();
    let id = h.manager.list_downloads()[0].id;
    let done = h.wait_status(id, DownloadStatus::Completed).await;
    assert_eq!(
        std::fs::read_to_string(&done.path).unwrap(),
        "downloaded by fake yt-dlp: dash-v+dash-a"
    );
    let download_call = ytdlp_calls(&h).pop().unwrap();
    let argv = args(&download_call);
    assert!(argv.windows(2).any(|w| w == ["-f", "dash-v+dash-a"]));
    assert!(argv.contains(&"--progress-template".to_string()));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_page_yt_dlp_cannot_read_explains_why_in_the_list() {
    let server = FileServer::start().await;
    let h = harness(publish(&server, &Releases::fake(), "r")).await;
    assert_eq!(install(&h).await.state, AddonState::Installed);
    tell_tool(&h, "fail.txt", "[site] 42: Video unavailable.");
    let mut ui = h.manager.subscribe();
    h.manager
        .add_capture(media_capture(
            &server.url("watch/5"),
            MediaKind::Page,
            Some("Gone"),
        ))
        .await
        .unwrap();
    // The reading the UI was told about ends, so its note can go.
    let reading = next_event(&mut ui, "MediaAnalysing", |e| match e {
        UiEvent::MediaAnalysing { id, .. } => Some(*id),
        _ => None,
    })
    .await;
    next_event(&mut ui, "the reading closed", |e| match e {
        UiEvent::ConfirmationClosed { id } if *id == reading => Some(()),
        _ => None,
    })
    .await;
    let row = wait_until("the failed row", || h.manager.list_downloads().pop()).await;
    let row = h.wait_status(row.id, DownloadStatus::Failed).await;
    let error = row.error.unwrap();
    assert!(
        error.message.contains("[site] 42: Video unavailable"),
        "{}",
        error.message
    );
    assert!(!error.message.contains("ERROR"));
    assert_eq!(error.fix, Some(FixAction::Retry));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_withdrawn_media_capture_closes_its_question() {
    let server = FileServer::start().await;
    hls(&server, false, false);
    let h = Harness::new().await;
    let mut ui = h.manager.subscribe();
    let capture = media_capture(&server.url("hls/master.m3u8"), MediaKind::Hls, None);
    h.manager.add_capture(capture.clone()).await.unwrap();
    let choice = choice_requested(&mut ui).await;
    h.manager
        .withdraw_capture(&capture.handover_id)
        .await
        .unwrap();
    let closed = next_event(&mut ui, "closed", |e| match e {
        UiEvent::ConfirmationClosed { id } => Some(*id),
        _ => None,
    })
    .await;
    assert_eq!(closed, choice.id);
    assert!(h.manager.pending_media_choices().is_empty());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_direct_media_file_downloads_like_any_file() {
    let server = FileServer::start().await;
    server.put("clips/talk.mp4", test_data(10 * 1024));
    let h = Harness::new().await;
    let mut capture = media_capture(&server.url("clips/talk.mp4"), MediaKind::Direct, None);
    capture.mime = Some("video/mp4".into());
    let added = h.manager.add_capture(capture).await.unwrap();
    let id = added.id.unwrap();
    let done = h.wait_status(id, DownloadStatus::Completed).await;
    assert_eq!(done.category, Category::Video);
    assert_eq!(done.media.unwrap().source, MediaKind::Direct);
}

/// The real ffmpeg (set `WIRECATCH_TEST_FFMPEG`) joins real video and audio.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn real_ffmpeg_joins_real_video_and_audio() {
    let Some(ffmpeg) = real_ffmpeg() else {
        eprintln!("skipped: set WIRECATCH_TEST_FFMPEG to a real ffmpeg to run this test");
        return;
    };
    let scratch = tempfile::tempdir().unwrap();
    let make = |args: &[&str], out: &str| {
        let path = scratch.path().join(out);
        let status = std::process::Command::new(&ffmpeg)
            .args(["-hide_banner", "-loglevel", "error", "-y"])
            .args(args)
            .arg(&path)
            .status()
            .unwrap();
        assert!(status.success(), "could not make {out}");
        std::fs::read(path).unwrap()
    };
    let video = make(
        &[
            "-f",
            "lavfi",
            "-i",
            "testsrc=duration=1:size=64x64:rate=10",
            "-c:v",
            "mpeg4",
            "-an",
        ],
        "v.mp4",
    );
    let audio = make(
        &["-f", "lavfi", "-i", "sine=duration=1", "-c:a", "aac", "-vn"],
        "a.m4a",
    );
    let server = FileServer::start().await;
    let releases = Releases::with_ffmpeg(&std::fs::read(&ffmpeg).unwrap());
    let h = harness(publish(&server, &releases, "r")).await;
    assert_eq!(install(&h).await.state, AddonState::Installed);
    tell_tool(&h, "info.json", &page_info(&server, &video, &audio));
    let mut ui = h.manager.subscribe();
    h.manager
        .add_capture(media_capture(&server.url("watch/6"), MediaKind::Page, None))
        .await
        .unwrap();
    let choice = choice_requested(&mut ui).await;
    h.manager
        .choose_media(choice.id, select("137+140"))
        .await
        .unwrap();
    let id = h.manager.list_downloads()[0].id;
    let done = h.wait_status(id, DownloadStatus::Completed).await;
    let probe = std::process::Command::new(&ffmpeg)
        .args(["-hide_banner", "-i"])
        .arg(&done.path)
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&probe.stderr);
    assert!(text.contains("Video: mpeg4"), "{text}");
    assert!(text.contains("Audio: aac"), "{text}");
}

/// A site names the extension of what it serves; a program extension must
/// never reach the disk (yt-dlp's CVE-2024-38519).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_page_cannot_save_a_program() {
    let server = FileServer::start().await;
    let h = harness(publish(&server, &Releases::fake(), "r")).await;
    assert_eq!(install(&h).await.state, AddonState::Installed);
    server.put("fmt/payload.exe", b"MZ not a video".to_vec());
    let info = serde_json::json!({
        "title": "Invoice",
        "formats": [
            {"format_id": "1", "ext": "exe", "vcodec": "avc1", "acodec": "mp4a", "height": 720,
             "protocol": "https", "url": server.url("fmt/payload.exe")},
            {"format_id": "2", "ext": "bat", "vcodec": "avc1", "acodec": "mp4a", "height": 360,
             "protocol": "http_dash_segments", "url": server.url("x.mpd")},
        ]
    });
    tell_tool(&h, "info.json", &info.to_string());
    // When yt-dlp downloads by itself, it names the file after the site too.
    tell_tool(&h, "download-ext", "exe");
    let mut ui = h.manager.subscribe();
    h.manager
        .add_capture(media_capture(&server.url("watch/7"), MediaKind::Page, None))
        .await
        .unwrap();
    let choice = choice_requested(&mut ui).await;
    for option in &choice.options {
        assert!(
            !option.label.to_lowercase().contains("exe"),
            "{}",
            option.label
        );
        assert!(
            !option.label.to_lowercase().contains("bat"),
            "{}",
            option.label
        );
    }
    for (n, option) in ["1", "2"].into_iter().enumerate() {
        if n == 1 {
            h.manager
                .add_capture(media_capture(&server.url("watch/8"), MediaKind::Page, None))
                .await
                .unwrap();
        }
        let choice = if n == 0 {
            choice.clone()
        } else {
            choice_requested(&mut ui).await
        };
        h.manager
            .choose_media(choice.id, select(option))
            .await
            .unwrap();
        let id = wait_until("the new row", || {
            h.manager
                .list_downloads()
                .into_iter()
                .find(|r| r.url.ends_with(&format!("watch/{}", 7 + n)))
        })
        .await
        .id;
        let done = h.wait_status(id, DownloadStatus::Completed).await;
        let saved = Path::new(&done.path);
        assert_eq!(
            saved.extension().and_then(|e| e.to_str()),
            Some("mp4"),
            "{}",
            done.path
        );
        let folder = saved.parent().unwrap();
        let programs: Vec<String> = std::fs::read_dir(folder)
            .unwrap()
            .filter_map(Result::ok)
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| n.ends_with(".exe") || n.ends_with(".bat"))
            .collect();
        assert!(programs.is_empty(), "{programs:?}");
    }
}

/// A page on the internet whose format points into the local network is
/// refused, and the local server is never contacted.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_page_on_the_internet_cannot_send_wirecatch_into_the_local_network() {
    let server = FileServer::start().await;
    let h = harness(publish(&server, &Releases::fake(), "r")).await;
    assert_eq!(install(&h).await.state, AddonState::Installed);
    tell_tool(&h, "info.json", &page_info(&server, b"video", b"audio"));
    let mut ui = h.manager.subscribe();
    // The fake yt-dlp never fetches the page; its address only says where the
    // page is: on the internet.
    h.manager
        .add_capture(media_capture(
            "http://93.184.215.14/watch?v=1",
            MediaKind::Page,
            None,
        ))
        .await
        .unwrap();
    let choice = choice_requested(&mut ui).await;
    h.manager
        .choose_media(choice.id, select("137+140"))
        .await
        .unwrap();
    let id = h.manager.list_downloads()[0].id;
    let failed = h.wait_status(id, DownloadStatus::Failed).await;
    let error = failed.error.unwrap();
    assert!(
        error.message.contains("your own network"),
        "{}",
        error.message
    );
    assert!(server.requests("fmt/video.mp4").is_empty());
    assert!(server.requests("fmt/audio.m4a").is_empty());
}

/// Stopping yt-dlp stops everything it started too.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn stopping_yt_dlp_stops_what_it_started() {
    let server = FileServer::start().await;
    let h = harness(publish(&server, &Releases::fake(), "r")).await;
    assert_eq!(install(&h).await.state, AddonState::Installed);
    tell_tool(&h, "info.json", &page_info(&server, b"v", b"a"));
    tell_tool(&h, "spawn-child", "1");
    tell_tool(&h, "delay-ms", "30000");
    let capture = media_capture(&server.url("watch/10"), MediaKind::Page, None);
    h.manager.add_capture(capture.clone()).await.unwrap();
    let heartbeat = crate::media_support::tool_dir(&h).join("heartbeat");
    let beat = || {
        std::fs::read_to_string(&heartbeat)
            .ok()
            .and_then(|t| t.trim().parse::<u32>().ok())
    };
    wait_until("the started program beats", || beat().filter(|b| *b > 2)).await;
    // The browser takes the capture back while yt-dlp is still reading.
    h.manager
        .withdraw_capture(&capture.handover_id)
        .await
        .unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(800)).await;
    let before = beat();
    tokio::time::sleep(std::time::Duration::from_millis(600)).await;
    assert_eq!(
        beat(),
        before,
        "the program yt-dlp started is still running"
    );

    // yt-dlp ran in Wirecatch's own folder, without plugins.
    let call = ytdlp_calls(&h).remove(0);
    assert!(args(&call).contains(&"--no-plugin-dirs".to_string()));
    let cwd = call["cwd"].as_str().unwrap().to_string();
    assert!(
        Path::new(&cwd).starts_with(h.root.path().join("data")),
        "{cwd}"
    );
}

/// Pages waiting for the add-on can be listed again (a reloaded window offers
/// the install again) and dismissed.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn pages_waiting_for_the_add_on_are_listed_and_can_be_dismissed() {
    let server = FileServer::start().await;
    let h = harness(publish(&server, &Releases::fake(), "r")).await;
    let page = server.url("watch/11");
    h.manager
        .add_capture(media_capture(&page, MediaKind::Page, None))
        .await
        .unwrap();
    let waiting = h.manager.pending_addon_requests();
    assert_eq!(waiting.len(), 1);
    assert_eq!(
        (waiting[0].url.as_str(), waiting[0].addon.id.as_str()),
        (page.as_str(), "media")
    );
    h.manager.dismiss_media_choice(waiting[0].id).unwrap();
    assert!(h.manager.pending_addon_requests().is_empty());
}

/// When the install the person started fails, the waiting pages say so in the
/// list instead of waiting for good.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_failed_install_turns_waiting_pages_into_failed_downloads() {
    let server = FileServer::start().await;
    let mut sources = publish(&server, &Releases::fake(), "bad");
    sources.deno.sha256 = crate::media_support::sha256(b"not it");
    let h = harness(sources).await;
    h.manager
        .add_capture(media_capture(
            &server.url("watch/12"),
            MediaKind::Page,
            Some("Waiting"),
        ))
        .await
        .unwrap();
    assert!(matches!(install(&h).await.state, AddonState::Failed { .. }));
    let row = wait_until("the failed row", || h.manager.list_downloads().pop()).await;
    let row = h.wait_status(row.id, DownloadStatus::Failed).await;
    let error = row.error.unwrap();
    assert!(
        error.message.contains("could not be installed"),
        "{}",
        error.message
    );
    assert_eq!(error.fix, Some(FixAction::Retry));
    assert!(h.manager.pending_addon_requests().is_empty());
}

/// A page that waited half an hour is not turned into a question once the
/// add-on arrives: it becomes a failed download instead.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_stale_wait_never_pops_a_question_later() {
    let server = FileServer::start().await;
    let h = harness(publish(&server, &Releases::fake(), "r")).await;
    tell_tool(&h, "info.json", &page_info(&server, b"v", b"a"));
    h.manager
        .add_capture(media_capture(
            &server.url("watch/13"),
            MediaKind::Page,
            None,
        ))
        .await
        .unwrap();
    h.clock.advance(chrono::TimeDelta::minutes(31));
    let mut ui = h.manager.subscribe();
    assert_eq!(install(&h).await.state, AddonState::Installed);
    let row = wait_until("the failed row", || h.manager.list_downloads().pop()).await;
    let row = h.wait_status(row.id, DownloadStatus::Failed).await;
    assert!(row.error.unwrap().message.contains("waited"));
    assert!(h.manager.pending_media_choices().is_empty());
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    while let Ok(event) = ui.try_recv() {
        assert!(
            !matches!(event, UiEvent::MediaChoiceRequested(_)),
            "a stale question was asked"
        );
    }
    assert!(ytdlp_calls(&h).is_empty(), "the stale page was read");
}

/// A password in a video's link is never shown while the video is read or
/// offered; the stream is still read from the real link.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_password_in_a_video_link_is_never_shown() {
    let server = FileServer::start().await;
    hls(&server, false, false);
    let h = Harness::new().await;
    let mut ui = h.manager.subscribe();
    let url = server
        .url("hls/master.m3u8")
        .replacen("http://", "http://ada:s3cret@", 1);
    let mut capture = media_capture(&url, MediaKind::Hls, Some("Clip"));
    capture.page_url = Some("https://ada:pagepass@example.com/watch".into());
    h.manager.add_capture(capture).await.unwrap();
    let (_, analysing) = next_event(&mut ui, "MediaAnalysing", |e| match e {
        UiEvent::MediaAnalysing { id, url } => Some((*id, url.clone())),
        _ => None,
    })
    .await;
    assert!(analysing.starts_with("http://ada:***@"), "{analysing}");
    let choice = choice_requested(&mut ui).await;
    assert_eq!(
        choice.page_url.as_deref(),
        Some("https://ada:***@example.com/watch")
    );
    for listed in h.manager.pending_media_choices() {
        let text = serde_json::to_string(&listed).unwrap();
        assert!(
            !text.contains("s3cret") && !text.contains("pagepass"),
            "{text}"
        );
    }
}
