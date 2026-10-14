//! Media captures: streams and pages become a quality choice, the choice becomes
//! a download, separate video and audio are joined, and page media is read again
//! after a restart.

use std::path::Path;
use std::time::Duration;

use wirecatch_core::CoreError;
use wirecatch_protocol::app::{
    AddonId, AddonState, Category, DownloadStatus, FixAction, MediaChoice, MediaSelection,
    OutputFormat, UiEvent, WaitReason,
};
use wirecatch_protocol::extension::{AddAction, MediaKind, NewDownload};

use crate::common::{Harness, TestResult, extension, test_data, wait_until};
use crate::media_support::{
    FileServer, Releases, args, harness, harness_refreshing, install, media_capture, next_event,
    publish, real_ffmpeg, tell_tool, use_real_ffmpeg, ytdlp_calls,
};
use wirecatch_engine::units::{KIB, KIB_USIZE};

fn select(option: &str) -> MediaSelection {
    MediaSelection {
        option_id: option.into(),
        folder: None,
        file_name: None,
        queue: None,
        later: false,
        output: OutputFormat::default(),
        audio_kbps: None,
        subtitles: Vec::new(),
        subtitle_files: false,
        chapters: false,
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

const SEGMENT: usize = 50 * KIB_USIZE;

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
    for (n, quality) in (0u8..).zip(["720", "360", "audio"]) {
        server.put(
            &format!("hls/{quality}/index.m3u8"),
            format!(
                "#EXTM3U\n#EXT-X-VERSION:3\n#EXT-X-TARGETDURATION:4\n#EXT-X-MEDIA-SEQUENCE:0\n#EXTINF:4.0,\nseg0.ts\n#EXTINF:4.0,\nseg1.ts\n{end}"
            ),
        );
        for seg in 0u8..2 {
            let data: Vec<u8> = test_data(SEGMENT)
                .into_iter()
                .map(|b| b.wrapping_add(n * 2 + seg))
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
async fn an_hls_capture_becomes_a_choice_and_then_a_file() -> TestResult {
    let server = FileServer::start().await;
    let best = hls(&server, false, false);
    let h = Harness::new().await?;
    let mut ui = h.manager.subscribe();
    let capture = media_capture(
        server.url("hls/master.m3u8"),
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
    assert_eq!(
        h.manager.pending_media_choices(),
        [] as [wirecatch_protocol::app::MediaChoice; 0]
    );
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
    let seen = server.seen_at("hls/master.m3u8");
    assert!(seen.iter().any(|s| s.headers.get("cookie").is_some()));
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn joining_separate_audio_needs_the_add_on() -> TestResult {
    let server = FileServer::start().await;
    hls(&server, false, true);
    let h = Harness::new().await?;
    let mut ui = h.manager.subscribe();
    let capture = media_capture(server.url("hls/master.m3u8"), MediaKind::Hls, None);
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
    assert_eq!(
        h.manager.pending_media_choices(),
        [] as [wirecatch_protocol::app::MediaChoice; 0]
    );
    assert!(matches!(
        h.manager.dismiss_media_choice(choice.id),
        Err(CoreError::ConfirmationNotFound(_))
    ));
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_live_stream_says_so_in_the_list() -> TestResult {
    let server = FileServer::start().await;
    hls(&server, true, false);
    let h = Harness::new().await?;
    let capture = media_capture(server.url("hls/master.m3u8"), MediaKind::Hls, Some("Live"));
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
    assert_eq!(
        h.manager.pending_media_choices(),
        [] as [wirecatch_protocol::app::MediaChoice; 0]
    );
    // An unfinished video stays where its parts are.
    let elsewhere = h.folders.home.join("Elsewhere");
    let moved = h
        .manager
        .set_download_folder(row.id, &elsewhere.to_string_lossy())
        .await;
    assert!(matches!(moved, Err(CoreError::WrongState(_))), "{moved:?}");
    Ok(())
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
             "protocol": "https", "url": server.url("fmt/audio.m4a").as_str(), "filesize": audio.len(), "http_headers": headers},
            {"format_id": "137", "ext": "mp4", "vcodec": "avc1.640028", "acodec": "none", "height": 1080, "tbr": 4000,
             "protocol": "https", "url": server.url("fmt/video.mp4").as_str(), "filesize": video.len(), "http_headers": headers},
            {"format_id": "18", "ext": "mp4", "vcodec": "avc1", "acodec": "mp4a", "height": 360, "tbr": 500,
             "protocol": "https", "url": server.url("fmt/small.mp4").as_str()},
        ]
    })
    .to_string()
}

/// Whether each yt-dlp call so far had the browser's cookies.
fn cookie_use(h: &Harness) -> TestResult<Vec<bool>> {
    ytdlp_calls(h)?
        .iter()
        .map(|call| Ok(args(call)?.contains(&"--cookies".to_string())))
        .collect()
}

/// YouTube is read without the browser's sign-in first: with a signed-in
/// browser's cookies yt-dlp gets video addresses YouTube often refuses.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn youtube_is_read_without_the_browser_sign_in_first() -> TestResult {
    let server = FileServer::start().await;
    let h = harness(publish(&server, &Releases::fake()?, "r")?).await?;
    assert_eq!(install(&h).await?.state, AddonState::Installed);
    let (video, audio) = (mp4_data(1024), mp4_data(1024));
    tell_tool(&h, "info.json", &page_info(&server, &video, &audio))?;
    let mut ui = h.manager.subscribe();
    h.manager
        .add_capture(media_capture(
            "https://www.youtube.com/watch?v=Y_nLA7iE2Uk",
            MediaKind::Page,
            None,
        ))
        .await
        .unwrap();
    choice_requested(&mut ui).await;
    assert_eq!(cookie_use(&h)?, [false]);
    Ok(())
}

/// A video that needs the sign-in (age-restricted, members-only, private) is
/// read again with the browser's cookies when reading it without them fails.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn youtube_uses_the_browser_sign_in_when_the_video_needs_it() -> TestResult {
    let server = FileServer::start().await;
    let h = harness(publish(&server, &Releases::fake()?, "r")?).await?;
    assert_eq!(install(&h).await?.state, AddonState::Installed);
    let (video, audio) = (mp4_data(1024), mp4_data(1024));
    tell_tool(&h, "info.json", &page_info(&server, &video, &audio))?;
    tell_tool(
        &h,
        "fail-without-cookies.txt",
        "[youtube] Y_nLA7iE2Uk: Sign in to confirm your age",
    )?;
    let mut ui = h.manager.subscribe();
    h.manager
        .add_capture(media_capture(
            "https://youtu.be/Y_nLA7iE2Uk",
            MediaKind::Page,
            None,
        ))
        .await
        .unwrap();
    choice_requested(&mut ui).await;
    assert_eq!(cookie_use(&h)?, [false, true]);
    Ok(())
}

/// A failure that is not about signing in (a removed video) is not tried again with the browser's cookies: they would not help, and are kept from the add-on when they need not be shared.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn youtube_is_not_read_again_with_the_sign_in_for_other_failures() -> TestResult {
    let server = FileServer::start().await;
    let h = harness(publish(&server, &Releases::fake()?, "r")?).await?;
    assert_eq!(install(&h).await?.state, AddonState::Installed);
    tell_tool(
        &h,
        "fail-without-cookies.txt",
        "[youtube] Y_nLA7iE2Uk: Video unavailable",
    )?;
    h.manager
        .add_capture(media_capture(
            "https://youtu.be/Y_nLA7iE2Uk",
            MediaKind::Page,
            None,
        ))
        .await
        .unwrap();
    let row = wait_until("the failed row", || h.manager.list_downloads().pop()).await;
    let row = h.wait_status(row.id, DownloadStatus::Failed).await;
    assert!(
        row.error
            .as_ref()
            .is_some_and(|e| e.message.contains("Video unavailable")),
        "{:?}",
        row.error
    );
    assert_eq!(cookie_use(&h)?, [false]);
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_page_is_read_with_yt_dlp_and_video_and_audio_are_joined() -> TestResult {
    let server = FileServer::start().await;
    let h = harness(publish(&server, &Releases::fake()?, "r")?).await?;
    assert_eq!(install(&h).await?.state, AddonState::Installed);
    let (video, audio) = (mp4_data(300 * KIB_USIZE), mp4_data(100 * KIB_USIZE));
    tell_tool(&h, "info.json", &page_info(&server, &video, &audio))?;
    // Long enough for the "Processing" status to be seen.
    tell_tool(&h, "ffmpeg-delay-ms", "800")?;
    let mut ui = h.manager.subscribe();
    let page = server.url("watch/1").to_string();
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

    let calls = ytdlp_calls(&h)?;
    assert_eq!(calls.len(), 1);
    check_page_reading(&calls[0], &page)?;

    h.manager
        .choose_media(choice.id, select("137+140"))
        .await
        .unwrap();
    let row = h.manager.list_downloads().remove(0);
    assert_eq!(row.media.as_ref().unwrap().quality, "1080p");
    h.wait_status(row.id, DownloadStatus::Processing).await;
    let done = h.wait_status(row.id, DownloadStatus::Completed).await;
    assert_eq!(
        extension(&done.file_name),
        Some("mp4"),
        "{}",
        done.file_name
    );
    check_joined_by_copying(&done.path)?;
    assert!(!Path::new(&format!("{}.wcmedia", done.path)).exists());
    // The format's own headers were sent with the format, never the page's cookies.
    for seen in server.seen_at("fmt/video.mp4") {
        assert_eq!(seen.headers.get("x-format").unwrap(), "yes");
        assert_eq!(seen.headers.get("user-agent").unwrap(), "yt-dlp-agent");
        assert!(seen.headers.get("cookie").is_none());
    }
    // The page was read again when the download started (addresses expire).
    assert_eq!(ytdlp_calls(&h)?.len(), 2);
    Ok(())
}

/// Checks that yt-dlp read the page with an argument vector Wirecatch
/// controls, and with the browser's cookies in a file bound to the page's
/// host, readable only by the user, and deleted right after.
fn check_page_reading(call: &serde_json::Value, page: &str) -> TestResult {
    let argv = args(call)?;
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
        .ok_or("no --js-runtimes")?;
    assert!(
        deno.strip_prefix("deno:")
            .is_some_and(|path| Path::new(path).is_file()),
        "{deno}"
    );
    assert_eq!(argv[argv.len() - 2..], ["--".to_string(), page.to_string()]);
    assert!(argv.contains(&"User-Agent:TestBrowser/2".to_string()));
    let cookies = call["cookies"].as_str().ok_or("no cookies were passed")?;
    assert!(cookies.starts_with("# Netscape HTTP Cookie File\n"));
    assert!(cookies.contains("127.0.0.1\tFALSE\t/\tFALSE\t"));
    assert!(cookies.contains("\tsession\tabc123\n") && cookies.contains("\ttheme\tdark\n"));
    if cfg!(unix) {
        assert_eq!(call["cookies_mode"].as_u64(), Some(0o600));
    }
    let cookies_path = call["cookies_path"].as_str().ok_or("no cookies file")?;
    assert!(
        !Path::new(cookies_path).exists(),
        "the cookies file was left behind"
    );
    Ok(())
}

/// Checks that the fake ffmpeg, which writes its arguments into the file at
/// `path`, copied both parts without re-encoding, each read as the format it
/// was recognized as, from a file only.
fn check_joined_by_copying(path: &str) -> TestResult {
    let joined = std::fs::read_to_string(path)?;
    let lines: Vec<&str> = joined.lines().collect();
    assert!(lines.windows(2).any(|w| w == ["-c", "copy"]), "{joined}");
    assert_eq!(lines.iter().filter(|l| **l == "-i").count(), 2);
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
    Ok(())
}

/// Statuses a row went through, from the events received so far.
fn statuses_seen(
    ui: &mut tokio::sync::broadcast::Receiver<UiEvent>,
    id: wirecatch_protocol::DownloadId,
) -> Vec<(DownloadStatus, Option<String>)> {
    let mut seen = Vec::new();
    loop {
        match ui.try_recv() {
            Ok(UiEvent::DownloadsChanged(rows)) => seen.extend(
                rows.into_iter()
                    .filter(|r| r.id == id)
                    .map(|r| (r.status, r.error.map(|e| e.message))),
            ),
            Ok(_) | Err(tokio::sync::broadcast::error::TryRecvError::Lagged(_)) => {}
            Err(_) => return seen,
        }
    }
}

/// Starts the 1080p video and audio of `page_info` from page `watch/<n>`.
async fn start_page_video(
    h: &Harness,
    server: &FileServer,
    n: u32,
) -> TestResult<wirecatch_protocol::DownloadId> {
    let mut ui = h.manager.subscribe();
    h.manager
        .add_capture(media_capture(
            server.url(&format!("watch/{n}")),
            MediaKind::Page,
            None,
        ))
        .await?;
    let choice = choice_requested(&mut ui).await;
    h.manager.choose_media(choice.id, select("137+140")).await?;
    Ok(h.manager.list_downloads()[0].id)
}

const QUICK: [Duration; 3] = [
    Duration::from_millis(50),
    Duration::from_millis(50),
    Duration::from_millis(50),
];

/// A video server sometimes refuses a good address for a while: the page is
/// read again for fresh addresses and the download goes on by itself, without
/// the row ever saying it failed and without downloading the finished audio
/// again.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_refused_format_address_is_read_again_and_the_download_goes_on() -> TestResult {
    let server = FileServer::start().await;
    let h = harness_refreshing(publish(&server, &Releases::fake()?, "r")?, QUICK.to_vec()).await?;
    assert_eq!(install(&h).await?.state, AddonState::Installed);
    let (video, audio) = (mp4_data(300 * KIB_USIZE), mp4_data(20 * KIB_USIZE));
    tell_tool(&h, "info.json", &page_info(&server, &video, &audio))?;
    server.fail("fmt/video.mp4", 1, 403);
    let mut ui = h.manager.subscribe();

    let id = start_page_video(&h, &server, 20).await?;
    let done = h.wait_status(id, DownloadStatus::Completed).await;

    assert!(Path::new(&done.path).is_file());
    // Read for the choice, at the start, and once more after the refusal.
    assert_eq!(ytdlp_calls(&h)?.len(), 3);
    let statuses: Vec<u16> = server
        .seen_at("fmt/video.mp4")
        .iter()
        .map(|s| s.status)
        .collect();
    assert_eq!(statuses.first(), Some(&403));
    assert!(statuses[1..].iter().all(|s| *s == 206), "{statuses:?}");
    // The audio finished in the first attempt and was kept.
    assert_eq!(server.requests("fmt/audio.m4a"), 1);
    for (status, error) in statuses_seen(&mut ui, id) {
        assert_ne!(status, DownloadStatus::Failed, "{error:?}");
        assert_eq!(error, None);
    }
    Ok(())
}

/// When every try with fresh addresses is refused too, the row fails with a
/// message that says what happened, without blaming an expired address.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_format_address_refused_every_time_ends_with_a_plain_error() -> TestResult {
    let server = FileServer::start().await;
    let h = harness_refreshing(publish(&server, &Releases::fake()?, "r")?, QUICK.to_vec()).await?;
    assert_eq!(install(&h).await?.state, AddonState::Installed);
    let (video, audio) = (mp4_data(300 * KIB_USIZE), mp4_data(20 * KIB_USIZE));
    tell_tool(&h, "info.json", &page_info(&server, &video, &audio))?;
    server.fail("fmt/video.mp4", 1000, 403);

    let id = start_page_video(&h, &server, 21).await?;
    let failed = h.wait_status(id, DownloadStatus::Failed).await;

    // The choice, the first attempt and three more, each with a fresh read.
    assert_eq!(ytdlp_calls(&h)?.len(), 1 + 1 + QUICK.len());
    assert_eq!(server.requests("fmt/video.mp4"), 1 + QUICK.len());
    assert_eq!(server.requests("fmt/audio.m4a"), 1);
    let error = failed.error.unwrap();
    assert_eq!(error.fix, Some(FixAction::Retry));
    assert!(error.message.contains("answered 403"), "{}", error.message);
    assert!(
        error.message.contains("fresh addresses from the page"),
        "{}",
        error.message
    );
    assert!(!error.message.contains("expire"), "{}", error.message);
    assert!(!error.message.contains('\u{2014}'), "{}", error.message);
    assert!(error.message.ends_with('.'), "{}", error.message);
    Ok(())
}

/// Pausing while the download waits to read the page again stops the wait at
/// once; resuming later goes on with the finished parts kept.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn pausing_while_waiting_to_read_the_page_again_stops_it() -> TestResult {
    let server = FileServer::start().await;
    let long = vec![Duration::from_secs(120)];
    let h = harness_refreshing(publish(&server, &Releases::fake()?, "r")?, long).await?;
    assert_eq!(install(&h).await?.state, AddonState::Installed);
    let (video, audio) = (mp4_data(300 * KIB_USIZE), mp4_data(20 * KIB_USIZE));
    tell_tool(&h, "info.json", &page_info(&server, &video, &audio))?;
    server.fail("fmt/video.mp4", 1000, 403);

    let id = start_page_video(&h, &server, 22).await?;
    wait_until("the refusal", || {
        (server.requests("fmt/video.mp4") > 0).then_some(())
    })
    .await;
    // Waiting: the row says it is connecting, not that it failed.
    tokio::time::sleep(Duration::from_millis(400)).await;
    let waiting = h.manager.list_downloads().remove(0);
    assert_eq!(waiting.status, DownloadStatus::Connecting);
    assert_eq!(waiting.error, None);
    assert_eq!(ytdlp_calls(&h)?.len(), 2);
    // It also says when the next try is: two minutes after the refusal, by the
    // manager's clock (a manual one here, so exactly two minutes from now).
    let now = wirecatch_core::Clock::unix_millis(h.clock.as_ref());
    match waiting.waiting {
        Some(WaitReason::ServerRefused { retry_at }) => assert_eq!(retry_at, now + 120_000),
        other => panic!("expected the refusal as the wait reason, got {other:?}"),
    }

    h.manager.pause(id).unwrap();
    // Far sooner than the two minutes it was waiting for.
    h.wait_status(id, DownloadStatus::Paused).await;
    assert_eq!(h.manager.list_downloads().remove(0).waiting, None);
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(
        ytdlp_calls(&h)?.len(),
        2,
        "the page was read again after the pause"
    );

    // Once the server takes the address again, resuming finishes the video
    // without fetching the finished audio again.
    server.fail("fmt/video.mp4", 0, 403);
    h.manager.resume(id).unwrap();
    h.wait_status(id, DownloadStatus::Completed).await;
    assert_eq!(server.requests("fmt/audio.m4a"), 1);
    Ok(())
}

/// yt-dlp's `http_chunk_size` for a format: no request for that format asks
/// for more.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_formats_chunk_size_limits_every_request_for_it() -> TestResult {
    let server = FileServer::start().await;
    let h = harness(publish(&server, &Releases::fake()?, "r")?).await?;
    assert_eq!(install(&h).await?.state, AddonState::Installed);
    let (video, audio) = (mp4_data(300 * KIB_USIZE), mp4_data(20 * KIB_USIZE));
    let mut info: serde_json::Value =
        serde_json::from_str(&page_info(&server, &video, &audio)).unwrap();
    let chunk = 64 * KIB;
    for format in info["formats"].as_array_mut().unwrap() {
        format["downloader_options"] = serde_json::json!({"http_chunk_size": chunk});
    }
    tell_tool(&h, "info.json", &info.to_string())?;

    let id = start_page_video(&h, &server, 23).await?;
    h.wait_status(id, DownloadStatus::Completed).await;

    let requests = server.seen_at("fmt/video.mp4");
    assert!(requests.len() >= 5, "{} requests", requests.len());
    for seen in requests {
        let (start, end) = seen.range().expect("a request without a range");
        let end = end.expect("a request for a range with no end");
        assert!(end - start <= chunk, "asked for {} bytes", end - start);
    }
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn page_media_is_read_again_after_a_restart_and_continues() -> TestResult {
    let server = FileServer::start().await;
    let mut h = harness(publish(&server, &Releases::fake()?, "r")?).await?;
    assert_eq!(install(&h).await?.state, AddonState::Installed);
    let (video, audio) = (mp4_data(600 * KIB_USIZE), mp4_data(20 * KIB_USIZE));
    tell_tool(&h, "info.json", &page_info(&server, &video, &audio))?;
    server.throttle("fmt/video.mp4", Some(150 * KIB));
    let mut ui = h.manager.subscribe();
    h.manager
        .add_capture(media_capture(server.url("watch/2"), MediaKind::Page, None))
        .await
        .unwrap();
    let choice = choice_requested(&mut ui).await;
    h.manager
        .choose_media(choice.id, select("137+140"))
        .await
        .unwrap();
    let id = h.manager.list_downloads()[0].id;
    h.wait_for(id, "part of the video", |r| r.done >= 150 * KIB)
        .await;
    h.restart().await?;
    server.throttle("fmt/video.mp4", None);
    let done = h.wait_status(id, DownloadStatus::Completed).await;
    assert!(Path::new(&done.path).is_file());
    // Read at the choice, at the first start and again after the restart.
    assert!(ytdlp_calls(&h)?.len() >= 3, "{}", ytdlp_calls(&h)?.len());
    // The video continued where it stopped instead of starting over.
    let resumed = server.seen_at("fmt/video.mp4").iter().any(|s| {
        s.headers
            .get("range")
            .and_then(|r| r.to_str().ok())
            .is_some_and(|r| r != "bytes=0-")
    });
    assert!(resumed, "the video was downloaded again from the start");
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_page_waits_for_the_add_on_and_continues_once_it_is_installed() -> TestResult {
    let server = FileServer::start().await;
    let h = harness(publish(&server, &Releases::fake()?, "r")?).await?;
    tell_tool(&h, "info.json", &page_info(&server, b"v", b"a"))?;
    let mut ui = h.manager.subscribe();
    let page = server.url("watch/3").to_string();
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

    assert_eq!((addon.id, url.as_str()), (AddonId::Media, page.as_str()));
    assert!(addon.size.is_some_and(|s| s > 0));
    assert_eq!(
        h.manager.pending_media_choices(),
        [] as [wirecatch_protocol::app::MediaChoice; 0]
    );

    h.manager.install_addon(AddonId::Media).await.unwrap();
    let choice = choice_requested(&mut ui).await;
    assert_eq!(
        choice.page_url.as_deref(),
        Some("https://videos.test/watch")
    );
    assert_eq!(h.manager.pending_media_choices(), vec![choice]);
    Ok(())
}

/// Runs a page whose two formats only yt-dlp can fetch, the video in `vcodec`, and returns the arguments yt-dlp downloaded with.
async fn left_to_yt_dlp(vcodec: &str, page: &str) -> TestResult<Vec<String>> {
    let server = FileServer::start().await;
    let h = harness(publish(&server, &Releases::fake()?, "r")?).await?;
    assert_eq!(install(&h).await?.state, AddonState::Installed);
    let info = serde_json::json!({
        "title": "Dash only",
        "formats": [
            {"format_id": "dash-v", "ext": "mp4", "vcodec": vcodec, "acodec": "none", "height": 720,
             "protocol": "http_dash_segments", "url": server.url("x.mpd").as_str()},
            {"format_id": "dash-a", "ext": "m4a", "vcodec": "none", "acodec": "mp4a",
             "protocol": "http_dash_segments", "url": server.url("x.mpd").as_str()},
        ]
    });
    tell_tool(&h, "info.json", &info.to_string())?;
    let mut ui = h.manager.subscribe();
    h.manager
        .add_capture(media_capture(server.url(page), MediaKind::Page, None))
        .await?;
    let choice = choice_requested(&mut ui).await;
    assert_eq!(choice.options[0].id, "dash-v+dash-a");
    h.manager
        .choose_media(choice.id, select("dash-v+dash-a"))
        .await?;
    let id = h.manager.list_downloads()[0].id;
    let done = h.wait_status(id, DownloadStatus::Completed).await;
    assert_eq!(
        std::fs::read_to_string(&done.path)?,
        r#"downloaded by fake yt-dlp: b*[format_id="dash-v"]+b*[format_id="dash-a"]"#
    );
    let download_call = ytdlp_calls(&h)?
        .pop()
        .ok_or("yt-dlp was not run to download")?;
    args(&download_call)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn formats_wirecatch_cannot_fetch_are_left_to_yt_dlp() -> TestResult {
    let argv = left_to_yt_dlp("avc1", "watch/4").await?;
    let selection = r#"b*[format_id="dash-v"]+b*[format_id="dash-a"]"#;
    assert!(argv.windows(2).any(|w| w == ["-f", selection]), "{argv:?}");
    assert!(argv.contains(&"--progress-template".to_string()));
    assert!(
        argv.windows(2)
            .any(|w| w == ["--merge-output-format", "mp4"]),
        "{argv:?}"
    );
    Ok(())
}

/// yt-dlp would put AV1 with AAC into MP4; the offer said MKV, as the shared container policy picks (D24, D31).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn yt_dlp_joins_formats_into_the_container_the_offer_named() -> TestResult {
    let argv = left_to_yt_dlp("av01.0.08M.08", "watch/14").await?;
    assert!(
        argv.windows(2)
            .any(|w| w == ["--merge-output-format", "mkv"]),
        "{argv:?}"
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_page_yt_dlp_cannot_read_explains_why_in_the_list() -> TestResult {
    let server = FileServer::start().await;
    let h = harness(publish(&server, &Releases::fake()?, "r")?).await?;
    assert_eq!(install(&h).await?.state, AddonState::Installed);
    tell_tool(&h, "fail.txt", "[site] 42: Video unavailable.")?;
    let mut ui = h.manager.subscribe();
    h.manager
        .add_capture(media_capture(
            server.url("watch/5"),
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
    Ok(())
}

/// A page description longer than Wirecatch reads is refused as such, without the advice to try again, which would only repeat it.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_page_described_at_a_length_past_the_limit_is_refused_as_such() -> TestResult {
    let server = FileServer::start().await;
    let h = harness(publish(&server, &Releases::fake()?, "r")?).await?;
    assert_eq!(install(&h).await?.state, AddonState::Installed);
    let limit = usize::try_from(wirecatch_streams::process::MAX_STDOUT)?;
    let description = format!(r#"{{"title": "{}"}}"#, "a".repeat(limit));
    tell_tool(&h, "info.json", &description)?;
    h.manager
        .add_capture(media_capture(server.url("watch/15"), MediaKind::Page, None))
        .await?;
    let row = wait_until("the failed row", || h.manager.list_downloads().pop()).await;
    let row = h.wait_status(row.id, DownloadStatus::Failed).await;
    let error = row.error.ok_or("the failed row has no error")?;
    assert!(error.message.contains("at a length"), "{}", error.message);
    assert_eq!(error.fix, None);
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_withdrawn_media_capture_closes_its_question() -> TestResult {
    let server = FileServer::start().await;
    hls(&server, false, false);
    let h = Harness::new().await?;
    let mut ui = h.manager.subscribe();
    let capture = media_capture(server.url("hls/master.m3u8"), MediaKind::Hls, None);
    h.manager.add_capture(capture.clone()).await.unwrap();
    let choice = choice_requested(&mut ui).await;
    h.manager.withdraw_capture(&capture.handover_id).unwrap();
    let closed = next_event(&mut ui, "closed", |e| match e {
        UiEvent::ConfirmationClosed { id } => Some(*id),
        _ => None,
    })
    .await;
    assert_eq!(closed, choice.id);
    assert_eq!(
        h.manager.pending_media_choices(),
        [] as [wirecatch_protocol::app::MediaChoice; 0]
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_direct_media_file_downloads_like_any_file() -> TestResult {
    let server = FileServer::start().await;
    server.put("clips/talk.mp4", test_data(10 * KIB_USIZE));
    let h = Harness::new().await?;
    let mut capture = media_capture(server.url("clips/talk.mp4"), MediaKind::Direct, None);
    capture.mime = Some("video/mp4".into());
    let added = h.manager.add_capture(capture).await.unwrap();
    let id = added.id.unwrap();
    let done = h.wait_status(id, DownloadStatus::Completed).await;
    assert_eq!(done.category, Category::Video);
    assert_eq!(done.media.unwrap().source, MediaKind::Direct);
    Ok(())
}

/// The real ffmpeg (set `WIRECATCH_TEST_FFMPEG`) joins real video and audio.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn real_ffmpeg_joins_real_video_and_audio() -> TestResult {
    let Some(ffmpeg) = real_ffmpeg() else {
        eprintln!("skipped: set WIRECATCH_TEST_FFMPEG to a real ffmpeg to run this test");
        return Ok(());
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
    let h = harness(publish(&server, &Releases::fake()?, "r")?).await?;
    assert_eq!(install(&h).await?.state, AddonState::Installed);
    use_real_ffmpeg(&h, &ffmpeg)?;
    tell_tool(&h, "info.json", &page_info(&server, &video, &audio))?;
    let mut ui = h.manager.subscribe();
    h.manager
        .add_capture(media_capture(server.url("watch/6"), MediaKind::Page, None))
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
    Ok(())
}

/// A site names the extension of what it serves; a program extension must
/// never reach the disk (yt-dlp's CVE-2024-38519).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_page_cannot_save_a_program() -> TestResult {
    let server = FileServer::start().await;
    let h = harness(publish(&server, &Releases::fake()?, "r")?).await?;
    assert_eq!(install(&h).await?.state, AddonState::Installed);
    server.put("fmt/payload.exe", b"MZ not a video".to_vec());
    let info = serde_json::json!({
        "title": "Invoice",
        "formats": [
            {"format_id": "1", "ext": "exe", "vcodec": "avc1", "acodec": "mp4a", "height": 720,
             "protocol": "https", "url": server.url("fmt/payload.exe").as_str()},
            {"format_id": "2", "ext": "bat", "vcodec": "avc1", "acodec": "mp4a", "height": 360,
             "protocol": "http_dash_segments", "url": server.url("x.mpd").as_str()},
        ]
    });
    tell_tool(&h, "info.json", &info.to_string())?;
    // When yt-dlp downloads by itself, it names the file after the site too.
    tell_tool(&h, "download-ext", "exe")?;
    let mut ui = h.manager.subscribe();
    h.manager
        .add_capture(media_capture(server.url("watch/7"), MediaKind::Page, None))
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
                .add_capture(media_capture(server.url("watch/8"), MediaKind::Page, None))
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
            .filter(|n| matches!(extension(n), Some("exe" | "bat")))
            .collect();
        assert!(programs.is_empty(), "{programs:?}");
    }
    Ok(())
}

/// A page on the internet whose format points into the local network is
/// refused, and the local server is never contacted.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_page_on_the_internet_cannot_send_wirecatch_into_the_local_network() -> TestResult {
    let server = FileServer::start().await;
    let h = harness(publish(&server, &Releases::fake()?, "r")?).await?;
    assert_eq!(install(&h).await?.state, AddonState::Installed);
    tell_tool(&h, "info.json", &page_info(&server, b"video", b"audio"))?;
    let mut ui = h.manager.subscribe();
    // The fake yt-dlp never fetches the page; its address only says where the
    // page is: on the internet.
    h.manager
        .add_capture(from_website(media_capture(
            "http://93.184.215.14/watch?v=1",
            MediaKind::Page,
            None,
        )))
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
    assert_eq!(server.requests("fmt/video.mp4"), 0);
    assert_eq!(server.requests("fmt/audio.m4a"), 0);
    Ok(())
}

/// `url` with its host written as a name that resolves to this computer, as a
/// website's name can when its owner makes it answer with a local address.
fn by_name(mut url: wirecatch_engine::Url) -> TestResult<wirecatch_engine::Url> {
    url.set_host(Some("localhost"))?;
    Ok(url)
}

/// `capture` as a page on the internet sends it.
fn from_website(capture: NewDownload) -> NewDownload {
    let page = "https://videos.example.com/watch";
    NewDownload {
        referrer: Some(page.into()),
        page_url: Some(page.into()),
        frame_url: None,
        ..capture
    }
}

/// A website's stream or page whose name resolves to the local network is
/// declined before anything is read or offered (CORE-3).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_websites_media_whose_name_answers_local_is_refused() -> TestResult {
    let server = FileServer::start().await;
    hls(&server, false, false);
    let h = Harness::new().await?;
    for (link, kind) in [
        (by_name(server.url("hls/master.m3u8"))?, MediaKind::Hls),
        (server.url("hls/master.m3u8"), MediaKind::Hls),
        (by_name(server.url("watch?v=1"))?, MediaKind::Page),
    ] {
        let error = h
            .manager
            .add_capture(from_website(media_capture(link, kind, None)))
            .await
            .unwrap_err();
        assert!(matches!(error, CoreError::NotStarted(_)), "{error:?}");
    }
    assert_eq!(h.manager.pending_media_choices().len(), 0);
    assert_eq!(h.manager.list_downloads().len(), 0);
    assert_eq!(server.requests("hls/master.m3u8"), 0);
    Ok(())
}

/// Stopping yt-dlp stops everything it started too.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn stopping_yt_dlp_stops_what_it_started() -> TestResult {
    let server = FileServer::start().await;
    let h = harness(publish(&server, &Releases::fake()?, "r")?).await?;
    assert_eq!(install(&h).await?.state, AddonState::Installed);
    tell_tool(&h, "info.json", &page_info(&server, b"v", b"a"))?;
    tell_tool(&h, "spawn-child", "1")?;
    tell_tool(&h, "delay-ms", "30000")?;
    let capture = media_capture(server.url("watch/10"), MediaKind::Page, None);
    h.manager.add_capture(capture.clone()).await.unwrap();
    let heartbeat = crate::media_support::tool_dir(&h)?.join("heartbeat");
    let beat = || {
        std::fs::read_to_string(&heartbeat)
            .ok()
            .and_then(|t| t.trim().parse::<u32>().ok())
    };
    wait_until("the started program beats", || beat().filter(|b| *b > 2)).await;
    // The browser takes the capture back while yt-dlp is still reading.
    h.manager.withdraw_capture(&capture.handover_id).unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(800)).await;
    let before = beat();
    tokio::time::sleep(std::time::Duration::from_millis(600)).await;
    assert_eq!(
        beat(),
        before,
        "the program yt-dlp started is still running"
    );

    // yt-dlp ran in Wirecatch's own folder, without plugins.
    let call = ytdlp_calls(&h)?.remove(0);
    assert!(args(&call)?.contains(&"--no-plugin-dirs".to_string()));
    let cwd = call["cwd"].as_str().unwrap().to_string();
    assert!(
        Path::new(&cwd).starts_with(h.root.path().join("data")),
        "{cwd}"
    );
    Ok(())
}

/// Pages waiting for the add-on can be listed again (a reloaded window offers
/// the install again) and dismissed.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn pages_waiting_for_the_add_on_are_listed_and_can_be_dismissed() -> TestResult {
    let server = FileServer::start().await;
    let h = harness(publish(&server, &Releases::fake()?, "r")?).await?;
    let page = server.url("watch/11").to_string();
    h.manager
        .add_capture(media_capture(&page, MediaKind::Page, None))
        .await
        .unwrap();
    let waiting = h.manager.pending_addon_requests();
    assert_eq!(waiting.len(), 1);
    assert_eq!(
        (waiting[0].url.as_str(), waiting[0].addon.id),
        (page.as_str(), AddonId::Media)
    );
    h.manager.dismiss_media_choice(waiting[0].id).unwrap();
    assert_eq!(
        h.manager.pending_addon_requests(),
        [] as [wirecatch_protocol::app::AddonRequestItem; 0]
    );
    Ok(())
}

/// When the install the person started fails, the waiting pages say so in the
/// list instead of waiting for good.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_failed_install_turns_waiting_pages_into_failed_downloads() -> TestResult {
    let server = FileServer::start().await;
    let mut sources = publish(&server, &Releases::fake()?, "bad")?;
    sources.deno = sources.deno.for_release(
        sources.deno.version(),
        wirecatch_core::ReleaseFile::new(
            sources.deno.release().url().clone(),
            crate::media_support::digest(b"not it"),
            sources.deno.release().size(),
        )?,
    );
    let h = harness(sources).await?;
    h.manager
        .add_capture(media_capture(
            server.url("watch/12"),
            MediaKind::Page,
            Some("Waiting"),
        ))
        .await
        .unwrap();
    assert!(matches!(
        install(&h).await?.state,
        AddonState::Failed { .. }
    ));
    let row = wait_until("the failed row", || h.manager.list_downloads().pop()).await;
    let row = h.wait_status(row.id, DownloadStatus::Failed).await;
    let error = row.error.unwrap();
    assert!(
        error.message.contains("could not be installed"),
        "{}",
        error.message
    );
    assert_eq!(error.fix, Some(FixAction::Retry));
    assert_eq!(
        h.manager.pending_addon_requests(),
        [] as [wirecatch_protocol::app::AddonRequestItem; 0]
    );
    Ok(())
}

/// A page that waited half an hour is not turned into a question once the
/// add-on arrives: it becomes a failed download instead.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_stale_wait_never_pops_a_question_later() -> TestResult {
    let server = FileServer::start().await;
    let h = harness(publish(&server, &Releases::fake()?, "r")?).await?;
    tell_tool(&h, "info.json", &page_info(&server, b"v", b"a"))?;
    h.manager
        .add_capture(media_capture(server.url("watch/13"), MediaKind::Page, None))
        .await
        .unwrap();
    h.clock.advance(chrono::TimeDelta::minutes(31));
    let mut ui = h.manager.subscribe();
    assert_eq!(install(&h).await?.state, AddonState::Installed);
    let row = wait_until("the failed row", || h.manager.list_downloads().pop()).await;
    let row = h.wait_status(row.id, DownloadStatus::Failed).await;
    assert!(row.error.unwrap().message.contains("waited"));
    assert_eq!(
        h.manager.pending_media_choices(),
        [] as [wirecatch_protocol::app::MediaChoice; 0]
    );
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    while let Ok(event) = ui.try_recv() {
        assert!(
            !matches!(event, UiEvent::MediaChoiceRequested(_)),
            "a stale question was asked"
        );
    }
    assert!(ytdlp_calls(&h)?.is_empty(), "the stale page was read");
    Ok(())
}

/// A password in a video's link is never shown while the video is read or
/// offered; the stream is still read from the real link.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_password_in_a_video_link_is_never_shown() -> TestResult {
    let server = FileServer::start().await;
    hls(&server, false, false);
    let h = Harness::new().await?;
    let mut ui = h.manager.subscribe();
    let url = server
        .url("hls/master.m3u8")
        .as_str()
        .replacen("http://", "http://ada:s3cret@", 1);
    let mut capture = media_capture(&url, MediaKind::Hls, Some("Clip"));
    capture.page_url = Some("https://ada:pagepass@videos.test/watch".into());
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
        Some("https://ada:***@videos.test/watch")
    );
    for listed in h.manager.pending_media_choices() {
        let text = serde_json::to_string(&listed).unwrap();
        assert!(
            !text.contains("s3cret") && !text.contains("pagepass"),
            "{text}"
        );
    }
    Ok(())
}
