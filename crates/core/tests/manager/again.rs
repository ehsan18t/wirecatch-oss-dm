//! A link the browser sends again while it is still a question or a download in progress joins
//! it, and its window comes to the front, instead of a second question or download (D89).

use wirecatch_engine::units::{KIB, KIB_USIZE};
use wirecatch_protocol::app::{Repeated, UiEvent};
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
        user_agent: None,
        extra_headers: vec![],
        source: CaptureSource::BrowserDownload,
        queue: None,
        media: None,
    }
}

async fn slow_server() -> TestServer {
    TestServer::start(
        test_data(256 * KIB_USIZE),
        Config {
            ranges: true,
            per_connection_bps: Some(16 * KIB),
            ..Config::default()
        },
    )
    .await
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
    let server = slow_server().await;
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
    let server = slow_server().await;
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
    let server = slow_server().await;
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
    let server = slow_server().await;
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
