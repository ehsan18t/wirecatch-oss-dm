//! What the desktop's download windows need from the core (D23): an answer returns the new
//! download's id, Download later adds it paused, media being read can be listed, one video is
//! asked about once however it was linked, and a question knows when its download is already
//! in the list.

use wirecatch_protocol::app::{
    AddonState, CaptureConfirmation, DownloadStatus, MediaChoice, MediaSelection, OutputFormat,
    UiEvent,
};
use wirecatch_protocol::extension::{CaptureSource, MediaKind, NewDownload, WithdrawOutcome};

use crate::common::{TestResult, fast_server, tune};
use crate::media_support::{
    FileServer, Releases, harness, install, media_capture, next_event, publish, tell_tool,
};

fn file_capture(url: String, handover: &str) -> NewDownload {
    NewDownload {
        handover_id: handover.to_string(),
        url,
        referrer: Some("https://example.com/page".into()),
        page_url: Some("https://example.com/page".into()),
        suggested_name: Some("report.pdf".into()),
        mime: None,
        size_hint: Some(1024),
        cookies: None,
        user_agent: None,
        extra_headers: vec![],
        source: CaptureSource::BrowserDownload,
        queue: None,
        media: None,
    }
}

fn pick(option: &str, later: bool) -> MediaSelection {
    MediaSelection {
        option_id: option.into(),
        folder: None,
        file_name: None,
        queue: None,
        later,
        output: OutputFormat::default(),
        audio_kbps: None,
        subtitles: Vec::new(),
        subtitle_files: false,
        chapters: false,
    }
}

/// What yt-dlp says about the one test video, whatever link it was given.
fn video_info(server: &FileServer) -> String {
    server.put("fmt/small.mp4", b"small video".to_vec());
    serde_json::json!({
        "id": "abc123",
        "extractor_key": "Youtube",
        "title": "One video",
        "formats": [
            {"format_id": "18", "ext": "mp4", "vcodec": "avc1", "acodec": "mp4a", "height": 360,
             "tbr": 500, "protocol": "https", "url": server.url("fmt/small.mp4").as_str()},
        ]
    })
    .to_string()
}

async fn next_choice(ui: &mut tokio::sync::broadcast::Receiver<UiEvent>) -> MediaChoice {
    next_event(ui, "a media choice", |e| match e {
        UiEvent::MediaChoiceRequested(c) => Some(c.clone()),
        _ => None,
    })
    .await
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_confirmed_file_returns_its_id_and_later_adds_it_paused() -> TestResult {
    let server = fast_server(1024).await?;
    let h = crate::common::Harness::new().await?;
    tune(&h.manager, |s| s.extension.ask_before_download = true)?;

    h.manager
        .add_capture(file_capture(server.url("a.pdf").to_string(), "one"))
        .await
        .unwrap();
    let first = h.manager.pending_confirmations().remove(0);
    assert!(h.manager.earlier_download(first.id).is_none());
    let later = CaptureConfirmation {
        later: true,
        ..CaptureConfirmation::default()
    };
    let id = h.manager.confirm_capture(first.id, later).await.unwrap();
    let row = h.row(id);
    assert_eq!(
        row.status,
        DownloadStatus::Paused,
        "Download later never starts it"
    );
    assert_eq!(
        h.manager.pending_confirmations(),
        [] as [wirecatch_protocol::app::PendingCapture; 0]
    );

    // The same link again: the question knows it is in the list already.
    h.manager
        .add_capture(file_capture(server.url("a.pdf#again").to_string(), "two"))
        .await
        .unwrap();
    let again = h.manager.pending_confirmations().remove(0);
    assert_eq!(h.manager.earlier_download(again.id).map(|r| r.id), Some(id));
    let started = h
        .manager
        .confirm_capture(again.id, CaptureConfirmation::default())
        .await
        .unwrap();
    assert_ne!(started, id);
    h.wait_status(started, DownloadStatus::Completed).await;
    // A question that is no longer open has nothing to compare.
    assert!(h.manager.earlier_download(again.id).is_none());
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn two_links_to_one_video_are_asked_about_once() -> TestResult {
    let server = FileServer::start().await;
    let h = harness(publish(&server, &Releases::fake()?, "r")?).await?;
    assert_eq!(install(&h).await?.state, AddonState::Installed);
    tell_tool(&h, "info.json", &video_info(&server))?;
    let mut ui = h.manager.subscribe();

    let first_link = server.url("watch?v=abc123").to_string();
    let second_link = server.url("watch?v=abc123&t=30s").to_string();
    h.manager
        .add_capture(media_capture(&first_link, MediaKind::Page, None))
        .await
        .unwrap();
    let choice = next_choice(&mut ui).await;
    h.manager
        .add_capture(media_capture(&second_link, MediaKind::Page, None))
        .await
        .unwrap();
    // The second link is read, found to be the same video, and folded into the first question.
    let folded = next_event(&mut ui, "the second question to close", |e| match e {
        UiEvent::MediaChoiceRequested(c) => panic!("asked twice about one video: {c:?}"),
        UiEvent::ConfirmationClosed { id } if *id != choice.id => Some(*id),
        _ => None,
    })
    .await;
    assert_eq!(h.manager.pending_media_choices(), vec![choice.clone()]);
    assert_eq!(
        h.manager.media_being_read(),
        [] as [wirecatch_protocol::app::MediaReading; 0]
    );

    // One answer covers both browser captures.
    let id = h
        .manager
        .choose_media(choice.id, pick("18", true))
        .await
        .unwrap();
    assert_eq!(h.row(id).status, DownloadStatus::Paused, "Download later");
    assert_eq!(
        h.manager.pending_media_choices(),
        [] as [wirecatch_protocol::app::MediaChoice; 0]
    );
    for link in [&first_link, &second_link] {
        let outcome = h
            .manager
            .withdraw_capture(&format!("media-{link}"))
            .unwrap();
        assert_eq!(outcome, WithdrawOutcome::AlreadyStarted, "{link}");
    }
    assert!(h.manager.earlier_download(folded).is_none());

    // Captured once more after the answer: a new question, which knows the video is in the
    // list already, though this link was never downloaded.
    let third_link = server.url("watch?v=abc123&pp=xyz").to_string();
    h.manager
        .add_capture(media_capture(&third_link, MediaKind::Page, None))
        .await
        .unwrap();
    let again = next_choice(&mut ui).await;
    assert_ne!(again.id, choice.id);
    assert_eq!(h.manager.earlier_download(again.id).map(|r| r.id), Some(id));
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_page_being_read_is_listed_until_its_choice_comes() -> TestResult {
    let server = FileServer::start().await;
    let h = harness(publish(&server, &Releases::fake()?, "r")?).await?;
    assert_eq!(install(&h).await?.state, AddonState::Installed);
    tell_tool(&h, "info.json", &video_info(&server))?;
    tell_tool(&h, "delay-ms", "1500")?;
    let mut ui = h.manager.subscribe();
    let link = server.url("watch?v=abc123").to_string();
    h.manager
        .add_capture(media_capture(&link, MediaKind::Page, None))
        .await
        .unwrap();
    let reading = next_event(&mut ui, "the reading to start", |e| match e {
        UiEvent::MediaAnalysing { id, url } => Some((*id, url.clone())),
        _ => None,
    })
    .await;
    let listed = h.manager.media_being_read();
    assert_eq!(listed.len(), 1);
    assert_eq!((listed[0].id, listed[0].url.clone()), reading);
    let choice = next_choice(&mut ui).await;
    assert_eq!(choice.id, reading.0);
    assert_eq!(
        h.manager.media_being_read(),
        [] as [wirecatch_protocol::app::MediaReading; 0]
    );
    Ok(())
}
