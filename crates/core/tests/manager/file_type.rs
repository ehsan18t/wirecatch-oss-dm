//! A server's name that changes the file type of a name the person confirmed is asked about (D166).

use wirecatch_engine::units::KIB_USIZE;
use wirecatch_protocol::DownloadId;
use wirecatch_protocol::app::{
    CaptureConfirmation, DownloadStatus, DownloadSummary, FileTypeChoice, FileTypeQuestion, UiEvent,
};
use wirecatch_protocol::extension::NewDownload;

use crate::common::{Config, Harness, PATIENCE, TestResult, TestServer, test_data, tune};

const LEN: usize = 4 * KIB_USIZE;

async fn server_naming(name: &str) -> TestServer {
    TestServer::start(
        test_data(LEN),
        Config {
            ranges: true,
            content_disposition: Some(format!("attachment; filename=\"{name}\"")),
            ..Config::default()
        },
    )
    .await
}

fn capture_named(url: &str, name: &str) -> NewDownload {
    NewDownload {
        handover_id: "click".into(),
        page_url: Some("https://intranet.test/page".into()),
        suggested_name: Some(name.into()),
        ..wirecatch_test_support::downloads::capture(url)
    }
}

async fn confirmed_as_proposed(
    h: &Harness,
    server: &TestServer,
    name: &str,
) -> TestResult<DownloadId> {
    tune(&h.manager, |s| s.extension.ask_before_download = true)?;
    let url = server.url("get").to_string();
    h.manager.add_capture(capture_named(&url, name)).await?;
    let [asked] = h
        .manager
        .pending_confirmations()
        .try_into()
        .map_err(|_| "not one question")?;
    Ok(h.manager
        .confirm_capture(asked.id, CaptureConfirmation::default())
        .await?)
}

async fn asked(
    events: &mut tokio::sync::broadcast::Receiver<UiEvent>,
) -> TestResult<FileTypeQuestion> {
    let question = tokio::time::timeout(PATIENCE, async {
        loop {
            if let Ok(UiEvent::FileTypeRequested(question)) = events.recv().await {
                return question;
            }
        }
    })
    .await?;
    Ok(question)
}

async fn answered(
    h: &Harness,
    question: &FileTypeQuestion,
    choice: FileTypeChoice,
) -> TestResult<DownloadSummary> {
    h.manager.answer_file_type(question.id, choice)?;
    Ok(h.wait_status(question.download, DownloadStatus::Completed)
        .await)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_server_cannot_change_the_type_of_a_confirmed_name_unasked() -> TestResult {
    let server = server_naming("invoice.exe").await;
    let h = Harness::new().await?;
    let mut events = h.manager.subscribe();

    let id = confirmed_as_proposed(&h, &server, "invoice.pdf").await?;

    let question = asked(&mut events).await?;
    assert_eq!(question.download, id);
    assert_eq!(question.confirmed_name, "invoice.pdf");
    assert_eq!(question.server_name, "invoice.exe");
    assert_eq!(h.manager.pending_file_types(), vec![question.clone()]);
    let row = h.wait_status(id, DownloadStatus::Paused).await;
    assert_eq!(row.file_name, "invoice.pdf");
    assert_eq!(row.done, 0);
    assert!(!std::path::Path::new(&row.path).exists());

    let row = answered(&h, &question, FileTypeChoice::KeepConfirmed).await?;
    assert_eq!(row.file_name, "invoice.pdf");
    assert_eq!(std::fs::read(&row.path)?, test_data(LEN));
    assert_eq!(h.manager.pending_file_types(), vec![]);
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_servers_name_is_saved_when_the_person_chooses_it() -> TestResult {
    let server = server_naming("invoice.exe").await;
    let h = Harness::new().await?;
    let mut events = h.manager.subscribe();
    confirmed_as_proposed(&h, &server, "invoice.pdf").await?;

    let question = asked(&mut events).await?;

    let row = answered(&h, &question, FileTypeChoice::UseServers).await?;
    assert_eq!(row.file_name, "invoice.exe");
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cancelling_the_question_downloads_nothing() -> TestResult {
    let server = server_naming("invoice.exe").await;
    let h = Harness::new().await?;
    let mut events = h.manager.subscribe();
    let id = confirmed_as_proposed(&h, &server, "invoice.pdf").await?;

    let question = asked(&mut events).await?;
    h.manager
        .answer_file_type(question.id, FileTypeChoice::Cancel)?;

    let row = h.wait_status(id, DownloadStatus::Cancelled).await;
    assert_eq!(row.done, 0);
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn resuming_instead_of_answering_asks_again() -> TestResult {
    let server = server_naming("invoice.exe").await;
    let h = Harness::new().await?;
    let mut events = h.manager.subscribe();
    let id = confirmed_as_proposed(&h, &server, "invoice.pdf").await?;
    let first = asked(&mut events).await?;
    h.wait_status(id, DownloadStatus::Paused).await;

    h.manager.resume(id)?;

    let second = asked(&mut events).await?;
    assert_ne!(second.id, first.id);
    assert_eq!(h.manager.pending_file_types(), vec![second]);
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_restart_does_not_let_the_server_change_the_type() -> TestResult {
    let server = server_naming("invoice.exe").await;
    let mut h = Harness::new().await?;
    let mut events = h.manager.subscribe();
    let id = confirmed_as_proposed(&h, &server, "invoice.pdf").await?;
    asked(&mut events).await?;
    let row = h.wait_status(id, DownloadStatus::Paused).await;

    h.restart().await?;

    let row_after = h.wait_status(id, DownloadStatus::Paused).await;
    assert_eq!(row_after.file_name, "invoice.pdf");
    assert_eq!(h.manager.pending_file_types(), vec![]);
    assert!(!std::path::Path::new(&row.path).exists());

    let mut events = h.manager.subscribe();
    h.manager.resume(id)?;

    let question = asked(&mut events).await?;
    assert_eq!(question.confirmed_name, "invoice.pdf");
    assert_eq!(question.server_name, "invoice.exe");
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_server_may_still_improve_a_confirmed_name() -> TestResult {
    for (confirmed, server_name) in [("get", "report.pdf"), ("invoice.pdf", "March invoice.PDF")] {
        let server = server_naming(server_name).await;
        let h = Harness::new().await?;
        let id = confirmed_as_proposed(&h, &server, confirmed).await?;

        let row = h.wait_status(id, DownloadStatus::Completed).await;

        assert_eq!(row.file_name, server_name);
        assert_eq!(h.manager.pending_file_types(), vec![]);
    }
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_name_nobody_confirmed_still_follows_the_server() -> TestResult {
    let server = server_naming("invoice.exe").await;
    let h = Harness::new().await?;
    let url = server.url("get").to_string();

    let added = h
        .manager
        .add_capture(capture_named(&url, "invoice.pdf"))
        .await?;

    let row = h
        .wait_status(added.id.ok_or("no id")?, DownloadStatus::Completed)
        .await;
    assert_eq!(row.file_name, "invoice.exe");
    Ok(())
}
