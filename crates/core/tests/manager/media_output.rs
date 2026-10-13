//! Output formats for page videos (D24): saving as another format, subtitles
//! and chapters, the "Converting" step, a conversion that fails or is paused,
//! "Keep the original", and the choices remembered for the next window.

use std::path::Path;

use wirecatch_core::CoreError;
use wirecatch_protocol::DownloadId;
use wirecatch_protocol::app::{
    AddonState, Category, DownloadStatus, FixAction, MediaChoice, MediaSelection, OutputFormat,
    ProcessingStep, StreamAction, UiEvent,
};
use wirecatch_protocol::extension::MediaKind;

use crate::common::{Harness, TestResult, extension, test_data};
use crate::media_support::{
    FileServer, Releases, harness, install, media_capture, next_event, publish, real_ffmpeg,
    tell_tool, ytdlp_calls,
};
use wirecatch_engine::units::{KIB, KIB_USIZE};

/// Test data that starts like an MP4 file.
fn mp4_data(len: usize) -> Vec<u8> {
    let mut data = test_data(len);
    data[..8].copy_from_slice(b"\0\0\0\x18ftyp");
    data
}

/// Test data that starts like a WebM file.
fn webm_data(len: usize) -> Vec<u8> {
    let mut data = test_data(len);
    data[..8].copy_from_slice(b"\x1A\x45\xDF\xA3webm");
    data
}

/// The format files of [`page_info`], keyed by format id.
const FORMATS: [(&str, &str); 4] = [
    ("251", "fmt/a251.webm"),
    ("140", "fmt/a140.m4a"),
    ("313", "fmt/v313.webm"),
    ("137", "fmt/v137.mp4"),
];

/// A YouTube-like page: 2160p only as VP9, 1080p as H.264, audio as Opus and
/// AAC; English subtitles, German auto-generated ones (and a machine
/// translation, which is not offered), two chapters and a thumbnail.
fn page_info(server: &FileServer) -> String {
    server.put("fmt/a251.webm", webm_data(40 * KIB_USIZE));
    server.put("fmt/a140.m4a", mp4_data(30 * KIB_USIZE));
    server.put("fmt/v313.webm", webm_data(200 * KIB_USIZE));
    server.put("fmt/v137.mp4", mp4_data(150 * KIB_USIZE));
    server.put(
        "subs/en.vtt",
        "WEBVTT\n\n00:00:00.000 --> 00:00:01.000\nHello\n",
    );
    server.put(
        "subs/de.vtt",
        "WEBVTT\n\n00:00:00.000 --> 00:00:01.000\nHallo\n",
    );
    server.put("thumb.jpg", b"\xFF\xD8\xFF\xE0 a picture".to_vec());
    let url = |path: &str| server.url(path).to_string();
    serde_json::json!({
        "title": "Cats: the movie",
        "uploader": "Cat Channel",
        "duration": 2.0,
        "thumbnail": url("thumb.jpg"),
        "formats": [
            {"format_id": "251", "ext": "webm", "vcodec": "none", "acodec": "opus", "abr": 160,
             "protocol": "https", "url": url("fmt/a251.webm"), "filesize": 40 * KIB},
            {"format_id": "140", "ext": "m4a", "vcodec": "none", "acodec": "mp4a.40.2", "abr": 128,
             "protocol": "https", "url": url("fmt/a140.m4a"), "filesize": 30 * KIB},
            {"format_id": "313", "ext": "webm", "vcodec": "vp9", "acodec": "none", "height": 2160, "tbr": 12000,
             "protocol": "https", "url": url("fmt/v313.webm"), "filesize": 200 * KIB},
            {"format_id": "137", "ext": "mp4", "vcodec": "avc1.640028", "acodec": "none", "height": 1080, "tbr": 4000,
             "protocol": "https", "url": url("fmt/v137.mp4"), "filesize": 150 * KIB},
        ],
        "subtitles": {
            "en": [{"ext": "json3", "url": url("subs/en.json3")}, {"ext": "vtt", "url": url("subs/en.vtt"), "name": "English"}],
            "live_chat": [{"ext": "json", "url": url("chat")}],
        },
        "automatic_captions": {
            "de-orig": [{"ext": "vtt", "url": url("subs/de.vtt"), "name": "German (Original)"}],
            "fr": [{"ext": "vtt", "url": format!("{}?tlang=fr", url("subs/de.vtt")), "name": "French"}],
        },
        "chapters": [
            {"start_time": 0, "end_time": 1, "title": "Start"},
            {"start_time": 1, "end_time": 2, "title": "End"},
        ],
    })
    .to_string()
}

fn selection(option: &str, output: OutputFormat) -> MediaSelection {
    MediaSelection {
        option_id: option.into(),
        folder: None,
        file_name: None,
        queue: None,
        later: false,
        output,
        audio_kbps: None,
        subtitles: Vec::new(),
        subtitle_files: false,
        chapters: false,
    }
}

async fn ready(server: &FileServer) -> TestResult<Harness> {
    let h = harness(publish(server, &Releases::fake()?, "r")?).await?;
    assert_eq!(install(&h).await?.state, AddonState::Installed);
    tell_tool(&h, "info.json", &page_info(server))?;
    Ok(h)
}

async fn choice_for(h: &Harness, server: &FileServer, n: u32) -> TestResult<MediaChoice> {
    let mut ui = h.manager.subscribe();
    h.manager
        .add_capture(media_capture(
            server.url(&format!("watch/{n}")),
            MediaKind::Page,
            None,
        ))
        .await?;
    Ok(next_event(&mut ui, "a media choice", |e| match e {
        UiEvent::MediaChoiceRequested(c) => Some(c.clone()),
        _ => None,
    })
    .await)
}

/// Requests made for each format file, in [`FORMATS`] order.
fn format_requests(server: &FileServer) -> Vec<usize> {
    FORMATS
        .iter()
        .map(|(_, path)| server.requests(path))
        .collect()
}

/// The lines the fake ffmpeg wrote into a file: its arguments.
fn ffmpeg_args(path: &str) -> TestResult<Vec<String>> {
    Ok(std::fs::read_to_string(path)?
        .lines()
        .map(str::to_string)
        .collect())
}

fn has_pair(args: &[String], a: &str, b: &str) -> bool {
    args.windows(2).any(|w| w[0] == a && w[1] == b)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_page_offers_formats_subtitles_and_chapters() -> TestResult {
    let server = FileServer::start().await;
    let h = ready(&server).await?;
    let choice = choice_for(&h, &server, 1).await?;
    let subtitles: Vec<(&str, &str, bool)> = choice
        .subtitles
        .iter()
        .map(|s| (s.id.as_str(), s.name.as_str(), s.auto))
        .collect();
    assert_eq!(
        subtitles,
        [
            ("sub:en", "English", false),
            ("auto:de-orig", "German", true)
        ]
    );
    assert!(choice.chapters);
    assert_eq!(choice.duration_ms, Some(2_000));
    let uhd = &choice.options[0];
    assert_eq!(uhd.label, "2160p, WEBM");
    assert_eq!(uhd.quality, "2160p");
    let mp4 = uhd
        .outputs
        .iter()
        .find(|f| f.format == OutputFormat::Mp4)
        .unwrap();
    assert_eq!(
        (mp4.video, mp4.audio),
        (StreamAction::Encode, StreamAction::Copy)
    );
    assert_eq!(mp4.video_codec.as_deref(), Some("H.264"));
    let full_hd = &choice.options[1];
    let video: Vec<(OutputFormat, StreamAction)> = full_hd
        .outputs
        .iter()
        .map(|f| (f.format, f.video))
        .collect();
    // 1080p comes as H.264 only: MP4 takes it as is, WebM needs VP9.
    assert_eq!(
        video,
        [
            (OutputFormat::Original, StreamAction::Copy),
            (OutputFormat::Mp4, StreamAction::Copy),
            (OutputFormat::Mkv, StreamAction::Copy),
            (OutputFormat::Webm, StreamAction::Encode)
        ]
    );
    // Asking for a format the row does not offer, or a subtitle the video
    // does not have, is refused and the question stays.
    let refused = h
        .manager
        .choose_media(choice.id, selection(&uhd.id, OutputFormat::Wav))
        .await
        .unwrap_err();
    assert!(matches!(refused, CoreError::Invalid(_)), "{refused}");
    let mut odd = selection(&uhd.id, OutputFormat::Mkv);
    odd.subtitles = vec!["sub:fr".into()];
    assert!(h.manager.choose_media(choice.id, odd).await.is_err());
    assert_eq!(h.manager.pending_media_choices().len(), 1);
    Ok(())
}

/// MP4 from a VP9-only quality: the matching AAC is downloaded (not the
/// Opus), the video is re-encoded, subtitles and chapters go in, the row shows
/// "Converting" with its progress, and the choices are remembered.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_video_saved_as_mp4_is_converted_with_subtitles_and_chapters() -> TestResult {
    let server = FileServer::start().await;
    let h = ready(&server).await?;
    tell_tool(&h, "ffmpeg-delay-ms", "1500")?;
    let choice = choice_for(&h, &server, 2).await?;
    let uhd = choice.options[0].id.clone();
    let mut pick = selection(&uhd, OutputFormat::Mp4);
    pick.subtitles = vec!["sub:en".into(), "auto:de-orig".into()];
    pick.chapters = true;
    let id = h.manager.choose_media(choice.id, pick).await.unwrap();

    let converting = h
        .wait_for(id, "the conversion with its progress", |r| {
            r.processing
                .is_some_and(|p| p.step == ProcessingStep::Converting && p.per_mille == Some(500))
        })
        .await;
    assert_eq!(converting.status, DownloadStatus::Processing);
    let done = h.wait_status(id, DownloadStatus::Completed).await;
    assert_eq!(
        extension(&done.file_name),
        Some("mp4"),
        "{}",
        done.file_name
    );
    assert_eq!(done.processing, None);
    assert_eq!(done.category, Category::Video);
    let args = ffmpeg_args(&done.path)?;
    // VP9 video and AAC audio went in; the Opus was never downloaded.
    assert_eq!(
        format_requests(&server)[0],
        0,
        "the Opus audio was downloaded"
    );
    assert!(format_requests(&server)[1] > 0 && format_requests(&server)[2] > 0);
    // The fake ffmpeg cannot be asked for its encoders, so the add-on's
    // software encoder is used.
    assert!(has_pair(&args, "-c:v:0", "libopenh264"), "{args:?}");
    assert!(has_pair(&args, "-c:a", "copy"), "{args:?}");
    assert!(has_pair(&args, "-c:s", "mov_text"), "{args:?}");
    assert_eq!(
        args.windows(2).filter(|w| w == &["-f", "webvtt"]).count(),
        2,
        "{args:?}"
    );
    assert!(has_pair(
        &args,
        "-metadata:s:s:1",
        "title=German (auto-generated)"
    ));
    assert!(has_pair(&args, "-f", "ffmetadata"), "{args:?}");
    assert!(has_pair(&args, "-f", "mp4"), "{args:?}");
    assert!(server.requests("subs/en.vtt") > 0);
    assert!(server.requests("subs/de.vtt") > 0);
    assert_eq!(
        server.requests("thumb.jpg"),
        0,
        "a video gets no cover picture"
    );
    assert!(!Path::new(&format!("{}.wcmedia", done.path)).exists());

    // The next video window starts from these choices.
    let video = h.manager.get_settings().media.video;
    assert_eq!(video.format, OutputFormat::Mp4);
    assert_eq!(video.subtitle_languages, ["en", "de"]);
    assert!(video.auto_subtitles && video.chapters && !video.subtitle_files);
    assert_eq!(
        h.manager.get_settings().media.audio.format,
        OutputFormat::Original
    );
    Ok(())
}

/// Audio only as MP3 at 320 kbps: the best audio (Opus) is re-encoded, with
/// title, artist and the video's picture; subtitles go next to it as `.srt`.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn audio_saved_as_mp3_gets_tags_a_cover_and_subtitle_files() -> TestResult {
    let server = FileServer::start().await;
    let h = ready(&server).await?;
    let choice = choice_for(&h, &server, 3).await?;
    let audio = choice.options.iter().find(|o| o.audio_only).unwrap();
    let mut pick = selection(&audio.id, OutputFormat::Mp3);
    pick.audio_kbps = Some(320);
    pick.subtitles = vec!["sub:en".into()];
    let id = h.manager.choose_media(choice.id, pick).await.unwrap();
    let done = h.wait_status(id, DownloadStatus::Completed).await;
    assert_eq!(
        extension(&done.file_name),
        Some("mp3"),
        "{}",
        done.file_name
    );
    assert_eq!(done.category, Category::Audio);
    let args = ffmpeg_args(&done.path)?;
    assert!(has_pair(&args, "-c:a", "libmp3lame"), "{args:?}");
    assert!(has_pair(&args, "-b:a", "320k"), "{args:?}");
    assert!(has_pair(&args, "-f", "jpeg_pipe"), "{args:?}");
    assert!(has_pair(&args, "-disposition:v:0", "attached_pic"));
    assert!(has_pair(&args, "-f", "mp3"), "{args:?}");
    assert_eq!(server.requests("fmt/a251.webm"), 1);
    assert_eq!(server.requests("fmt/a140.m4a"), 0);
    // The subtitles, as SubRip next to the MP3.
    let srt = Path::new(&done.path).with_extension("en.srt");
    let made = ffmpeg_args(&srt.to_string_lossy())?;
    assert!(has_pair(&made, "-f", "srt"), "{made:?}");
    assert!(has_pair(&made, "-f", "webvtt"), "{made:?}");

    let audio = h.manager.get_settings().media.audio;
    assert_eq!(audio.format, OutputFormat::Mp3);
    assert_eq!(audio.audio_kbps, Some(320));
    assert_eq!(audio.subtitle_languages, ["en"]);
    assert!(
        !audio.auto_subtitles,
        "auto-generated ones were offered and left"
    );
    Ok(())
}

/// A conversion that fails keeps the download: Try again converts again
/// without reading the page or downloading anything, and works.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_failed_conversion_keeps_the_download_and_try_again_only_converts() -> TestResult {
    let server = FileServer::start().await;
    let h = ready(&server).await?;
    tell_tool(&h, "ffmpeg-fail.txt", "Error: the encoder broke")?;
    let choice = choice_for(&h, &server, 4).await?;
    let uhd = choice.options[0].id.clone();
    let id = h
        .manager
        .choose_media(choice.id, selection(&uhd, OutputFormat::Mp4))
        .await
        .unwrap();
    let failed = h.wait_status(id, DownloadStatus::Failed).await;
    let error = failed.error.unwrap();
    assert_eq!(error.fix, Some(FixAction::KeepOriginal));
    assert!(
        error.message.contains("could not save it as MP4")
            && error.message.contains("the encoder broke")
            && error.message.contains("(MKV)"),
        "{}",
        error.message
    );
    assert!(!error.message.contains('\u{2014}'));
    let (calls, requests) = (ytdlp_calls(&h)?.len(), format_requests(&server));

    std::fs::remove_file(crate::media_support::tool_dir(&h)?.join("ffmpeg-fail.txt")).unwrap();
    h.manager.retry(id).unwrap();
    let done = h.wait_status(id, DownloadStatus::Completed).await;
    assert_eq!(extension(&done.file_name), Some("mp4"));
    assert_eq!(ytdlp_calls(&h)?.len(), calls, "the page was read again");
    assert_eq!(
        format_requests(&server),
        requests,
        "something was downloaded again"
    );
    Ok(())
}

/// "Keep the original" after a failed conversion saves the downloaded
/// streams as they came (VP9 with AAC: MKV), without converting.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn keep_the_original_saves_the_download_as_it_came() -> TestResult {
    let server = FileServer::start().await;
    let h = ready(&server).await?;
    tell_tool(&h, "ffmpeg-fail.txt", "Error: the encoder broke")?;
    let choice = choice_for(&h, &server, 5).await?;
    let uhd = choice.options[0].id.clone();
    let mut pick = selection(&uhd, OutputFormat::Mp4);
    pick.subtitles = vec!["sub:en".into()];
    let id = h.manager.choose_media(choice.id, pick).await.unwrap();
    h.wait_status(id, DownloadStatus::Failed).await;
    let requests = format_requests(&server);
    // Only a download waiting for it can be kept as it came.
    assert!(matches!(
        h.manager.keep_original(DownloadId(id.0 + 1000)),
        Err(CoreError::DownloadNotFound(_))
    ));
    h.manager.keep_original(id).unwrap();
    let done = h.wait_status(id, DownloadStatus::Completed).await;
    assert_eq!(
        extension(&done.file_name),
        Some("mkv"),
        "{}",
        done.file_name
    );
    let args = ffmpeg_args(&done.path)?;
    assert!(has_pair(&args, "-c", "copy"), "{args:?}");
    assert!(
        !args.iter().any(|a| a == "-map_chapters"),
        "it was converted"
    );
    assert_eq!(format_requests(&server), requests);
    assert!(
        h.manager.keep_original(id).is_err(),
        "nothing waits any more"
    );
    Ok(())
}

/// Pausing during the conversion stops ffmpeg; resuming converts again from
/// the downloaded parts.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_paused_conversion_starts_again_without_downloading() -> TestResult {
    let server = FileServer::start().await;
    let h = ready(&server).await?;
    tell_tool(&h, "ffmpeg-delay-ms", "30000")?;
    let choice = choice_for(&h, &server, 6).await?;
    let uhd = choice.options[0].id.clone();
    let id = h
        .manager
        .choose_media(choice.id, selection(&uhd, OutputFormat::Mp4))
        .await
        .unwrap();
    h.wait_for(id, "the conversion", |r| {
        r.processing
            .is_some_and(|p| p.step == ProcessingStep::Converting)
    })
    .await;
    let (calls, requests) = (ytdlp_calls(&h)?.len(), format_requests(&server));
    h.manager.pause(id).unwrap();
    let paused = h.wait_status(id, DownloadStatus::Paused).await;
    assert_eq!(paused.processing, None);
    std::fs::remove_file(crate::media_support::tool_dir(&h)?.join("ffmpeg-delay-ms")).unwrap();
    h.manager.resume(id).unwrap();
    let done = h.wait_status(id, DownloadStatus::Completed).await;
    assert!(Path::new(&done.path).is_file());
    assert_eq!(ytdlp_calls(&h)?.len(), calls, "the page was read again");
    assert_eq!(
        format_requests(&server),
        requests,
        "something was downloaded again"
    );
    Ok(())
}

/// Two seconds of VP9 video and of Opus audio, each in a WebM file, made with
/// the real `ffmpeg` in `dir`.
fn sample_streams(ffmpeg: &Path, dir: &Path) -> TestResult<(Vec<u8>, Vec<u8>)> {
    let make = |args: &[&str], out: &str| -> TestResult<Vec<u8>> {
        let path = dir.join(out);
        let status = std::process::Command::new(ffmpeg)
            .args(["-hide_banner", "-loglevel", "error", "-y"])
            .args(args)
            .arg(&path)
            .status()?;
        assert!(status.success(), "could not make {out}");
        Ok(std::fs::read(path)?)
    };
    let video = make(
        &[
            "-f",
            "lavfi",
            "-i",
            "testsrc2=size=160x120:rate=15:duration=2",
            "-c:v",
            "libvpx-vp9",
            "-deadline",
            "realtime",
            "-cpu-used",
            "8",
            "-b:v",
            "200k",
            "-an",
        ],
        "v.webm",
    )?;
    let audio = make(
        &[
            "-f",
            "lavfi",
            "-i",
            "sine=duration=2",
            "-c:a",
            "libopus",
            "-vn",
        ],
        "a.webm",
    )?;
    Ok((video, audio))
}

/// A real ffmpeg (set `WIRECATCH_TEST_FFMPEG`) turns a VP9 and Opus page
/// video into an MP4 with H.264 and AAC, with the subtitles and chapters, using
/// whichever encoder works on this computer.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn real_ffmpeg_saves_a_page_video_as_mp4() -> TestResult {
    let Some(ffmpeg) = real_ffmpeg() else {
        eprintln!("skipped: set WIRECATCH_TEST_FFMPEG to a real ffmpeg to run this test");
        return Ok(());
    };
    let scratch = tempfile::tempdir()?;
    let (video, audio) = sample_streams(&ffmpeg, scratch.path())?;
    let server = FileServer::start().await;
    let releases = Releases::with_ffmpeg(&std::fs::read(&ffmpeg).unwrap())?;
    let h = harness(publish(&server, &releases, "r")?).await?;
    assert_eq!(install(&h).await?.state, AddonState::Installed);
    server.put("fmt/v.webm", video);
    server.put("fmt/a.webm", audio);
    server.put(
        "subs/en.vtt",
        "WEBVTT\n\n00:00:00.000 --> 00:00:01.000\nHello\n",
    );
    let info = serde_json::json!({
        "title": "Real", "duration": 2.0,
        "formats": [
            {"format_id": "251", "ext": "webm", "vcodec": "none", "acodec": "opus", "abr": 64,
             "protocol": "https", "url": server.url("fmt/a.webm").as_str()},
            {"format_id": "313", "ext": "webm", "vcodec": "vp9", "acodec": "none", "height": 120, "tbr": 200,
             "protocol": "https", "url": server.url("fmt/v.webm").as_str()},
        ],
        "subtitles": {"en": [{"ext": "vtt", "url": server.url("subs/en.vtt").as_str(), "name": "English"}]},
        "chapters": [{"start_time": 0, "end_time": 1, "title": "One"}, {"start_time": 1, "end_time": 2, "title": "Two"}],
    });
    tell_tool(&h, "info.json", &info.to_string())?;
    let choice = choice_for(&h, &server, 7).await?;
    let mut pick = selection(&choice.options[0].id, OutputFormat::Mp4);
    pick.subtitles = vec!["sub:en".into()];
    pick.chapters = true;
    let id = h.manager.choose_media(choice.id, pick).await.unwrap();
    let done = h.wait_status(id, DownloadStatus::Completed).await;
    assert_eq!(
        extension(&done.file_name),
        Some("mp4"),
        "{}",
        done.file_name
    );
    let probe = std::process::Command::new(&ffmpeg)
        .args(["-hide_banner", "-i"])
        .arg(&done.path)
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&probe.stderr);
    assert!(text.contains("Video: h264"), "{text}");
    assert!(text.contains("Audio: aac"), "{text}");
    assert!(text.contains("Subtitle: mov_text"), "{text}");
    assert!(text.contains("Chapter #0:1"), "{text}");
    Ok(())
}
