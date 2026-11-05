//! A link the browser sends again while it is still a question or a download in progress joins
//! it, and its window comes to the front, instead of a second question or download (D89).

use wirecatch_engine::units::{KIB, KIB_USIZE};
use wirecatch_protocol::app::{
    DownloadStatus, FileExistsAnswer, FileExistsChoice, Repeated, UiEvent,
};
use wirecatch_protocol::extension::{AddAction, CaptureSource, NewDownload};

use crate::common::{Config, Harness, PATIENCE, TestResult, TestServer, test_data, tune};

/// The `n`th click on `url`: each sends a handover of its own.
fn click(url: &str, n: u32) -> NewDownload {
    NewDownload {
        handover_id: format!("click-{n}"),
        url: url.to_string(),
        referrer: None,
        page_url: Some("https://intranet.test/page".into()),
        frame_url: None,
        suggested_name: Some("report.pdf".into()),
        mime: None,
        size_hint: None,
        cookies: None,
        cookie_jar: None,
        user_agent: None,
        extra_headers: vec![],
        source: CaptureSource::BrowserDownload,
        queue: None,
        media: None,
    }
}

async fn holding_server() -> TestServer {
    let server = TestServer::start(
        test_data(256 * KIB_USIZE),
        Config {
            ranges: true,
            ..Config::default()
        },
    )
    .await;
    server.hold_bodies_after(16 * KIB);
    server
}

async fn repeated(events: &mut tokio::sync::broadcast::Receiver<UiEvent>) -> TestResult<Repeated> {
    let found = tokio::time::timeout(PATIENCE, async {
        loop {
            if let Ok(UiEvent::CaptureRepeated(repeated)) = events.recv().await {
                return repeated;
            }
        }
    })
    .await?;
    Ok(found)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_link_already_downloading_shows_that_download() -> TestResult {
    let server = holding_server().await;
    let h = Harness::new().await?;
    let url = server.url("report.pdf").to_string();
    let first = h.manager.add_capture(click(&url, 1)).await?;
    let mut events = h.manager.subscribe();

    let again = h.manager.add_capture(click(&url, 2)).await?;
    assert_eq!(again.id, first.id);
    assert_ne!(again.action, AddAction::AwaitingConfirmation);
    let id = first.id.ok_or("no download")?;
    assert_eq!(repeated(&mut events).await?, Repeated::Download(id));
    assert_eq!(h.manager.list_downloads().len(), 1);
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_link_waiting_for_confirmation_shows_that_question() -> TestResult {
    let server = holding_server().await;
    let h = Harness::new().await?;
    tune(&h.manager, |s| s.extension.ask_before_download = true)?;
    let url = server.url("report.pdf").to_string();
    h.manager.add_capture(click(&url, 1)).await?;
    let [asked] = h
        .manager
        .pending_confirmations()
        .try_into()
        .map_err(|_| "not one question")?;
    let mut events = h.manager.subscribe();

    h.manager.add_capture(click(&url, 2)).await?;
    assert_eq!(repeated(&mut events).await?, Repeated::Question(asked.id));
    assert_eq!(h.manager.pending_confirmations().len(), 1);
    Ok(())
}

/// Confirmed, its window shows its progress; clicking the link again shows that, not a second
/// question.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_confirmed_link_shows_its_download() -> TestResult {
    let server = holding_server().await;
    let h = Harness::new().await?;
    tune(&h.manager, |s| s.extension.ask_before_download = true)?;
    let url = server.url("report.pdf").to_string();
    h.manager.add_capture(click(&url, 1)).await?;
    let [asked] = h
        .manager
        .pending_confirmations()
        .try_into()
        .map_err(|_| "not one question")?;
    let id = h
        .manager
        .confirm_capture(
            asked.id,
            wirecatch_protocol::app::CaptureConfirmation::default(),
        )
        .await?;
    let mut events = h.manager.subscribe();

    let again = h.manager.add_capture(click(&url, 2)).await?;
    assert_eq!(again.id, Some(id));
    assert_eq!(repeated(&mut events).await?, Repeated::Download(id));
    assert_eq!(h.manager.pending_confirmations(), vec![]);
    assert_eq!(h.manager.list_downloads().len(), 1);
    Ok(())
}

/// CORE-4: the browser gives up waiting while the person confirms in the app. The app has it from
/// the moment they said yes, so the browser is told so and drops its copy; never both download.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_withdrawal_while_confirming_finds_it_taken() -> TestResult {
    let server = holding_server().await;
    let h = Harness::new().await?;
    tune(&h.manager, |s| s.extension.ask_before_download = true)?;
    let url = server.url("report.pdf").to_string();
    let capture = click(&url, 1);
    h.manager.add_capture(capture.clone()).await?;
    let [asked] = h
        .manager
        .pending_confirmations()
        .try_into()
        .map_err(|_| "not one question")?;

    // The withdrawal comes while the confirmation waits for its first step to finish.
    let (confirmed, withdrawn) = tokio::join!(biased;
        h.manager.confirm_capture(asked.id, wirecatch_protocol::app::CaptureConfirmation::default()),
        async { h.manager.withdraw_capture(&capture.handover_id) },
    );
    confirmed?;
    assert_eq!(
        withdrawn?,
        wirecatch_protocol::extension::WithdrawOutcome::AlreadyStarted
    );
    assert_eq!(h.manager.list_downloads().len(), 1);
    Ok(())
}

/// Two clicks of one link at the same moment, with no question first, make one download. The join is decided when the path is taken, so it holds however the two interleave.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn two_clicks_at_once_make_one_download() -> TestResult {
    let server = holding_server().await;
    for round in 0..20 {
        let h = Harness::new().await?;
        let url = format!("{}?round={round}", server.url("report.pdf"));
        let (one, two) = tokio::join!(
            h.manager.add_capture(click(&url, 1)),
            h.manager.add_capture(click(&url, 2))
        );
        one?;
        two?;
        assert_eq!(h.manager.list_downloads().len(), 1, "round {round}");
        h.manager.shutdown().await;
    }
    Ok(())
}

/// The browser's copy may only go once a server has answered (`AddAction::Started`).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn joining_a_download_no_server_has_answered_is_not_called_started() -> TestResult {
    let server = holding_server().await;
    let h = Harness::new().await?;
    let url = server.url("report.pdf").to_string();
    server.hold();
    let first = {
        let (manager, click) = (h.manager.clone(), click(&url, 1));
        tokio::spawn(async move { manager.add_capture(click).await })
    };
    crate::common::wait_until("the first run's request", || {
        (server.arrived("report.pdf") > 0).then_some(())
    })
    .await;

    let joined = h.manager.add_capture(click(&url, 2)).await?;
    assert_eq!(joined.action, AddAction::Queued);

    server.release();
    assert_eq!(first.await??.action, AddAction::Started);
    Ok(())
}

/// The same link with another fragment joins the question already showing.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn another_fragment_joins_the_open_question() -> TestResult {
    let h = Harness::new().await?;
    tune(&h.manager, |s| s.extension.ask_before_download = true)?;
    h.manager
        .add_capture(click("https://example.com/report.pdf#page=1", 1))
        .await?;
    h.manager
        .add_capture(click("https://example.com/report.pdf#page=2", 2))
        .await?;
    assert_eq!(h.manager.pending_confirmations().len(), 1);
    Ok(())
}

/// With no confirmation window, a captured file already in the folder is asked about; the attempt stays and the browser's copy is not needed.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_finished_file_captured_again_keeps_its_question() -> TestResult {
    let server = TestServer::start(
        test_data(16 * KIB_USIZE),
        Config {
            ranges: true,
            content_disposition: Some("attachment; filename=\"report.pdf\"".into()),
            ..Config::default()
        },
    )
    .await;
    let h = Harness::new().await?;
    let url = server.url("get").to_string();
    let first = h.manager.add_capture(click(&url, 1)).await?;
    let first = h
        .wait_status(first.id.ok_or("no id")?, DownloadStatus::Completed)
        .await;

    let again = h.manager.add_capture(click(&url, 2)).await?;

    let [question] = &h.manager.pending_file_exists()[..] else {
        return Err("the question closed itself".into());
    };
    let id = again.id.ok_or("no id")?;
    assert_eq!(question.download, id);
    h.manager.answer_file_exists(
        question.id,
        FileExistsAnswer {
            choice: FileExistsChoice::KeepBoth,
            remember: false,
        },
    )?;
    let done = h.wait_status(id, DownloadStatus::Completed).await;
    assert_ne!(done.path, first.path);
    Ok(())
}
