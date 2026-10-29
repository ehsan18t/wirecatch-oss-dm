//! A download whose file is already in its folder (D88): a file that only shares the name gets a
//! new one, and a duplicate does what `when_file_exists` says.

use std::path::{Path, PathBuf};

use wirecatch_engine::units::{KIB, KIB_USIZE};
use wirecatch_protocol::app::{
    DownloadStatus, DownloadSummary, FileExistsAnswer, FileExistsChoice, FileExistsQuestion,
    UiEvent, WhenFileExists,
};

use crate::common::{Config, Harness, PATIENCE, TestResult, TestServer, test_data};

const NAME: &str = "report.pdf";
const LEN: usize = 4 * KIB_USIZE;

async fn server() -> TestServer {
    TestServer::start(
        test_data(LEN),
        Config {
            ranges: true,
            ..Config::default()
        },
    )
    .await
}

/// A folder holding `content` as [`NAME`].
fn folder_with(h: &Harness, content: &[u8]) -> TestResult<PathBuf> {
    let dir = h.root.path().join("saved");
    std::fs::create_dir_all(&dir)?;
    std::fs::write(dir.join(NAME), content)?;
    Ok(dir)
}

/// Something of the server's size that is not the server's file, so a replacement shows.
fn same_size() -> Vec<u8> {
    vec![b'x'; LEN]
}

async fn add_into(h: &Harness, link: &str, dir: &Path) -> TestResult<DownloadSummary> {
    h.add_with(link, |r| {
        r.folder = Some(dir.to_string_lossy().into_owned());
        r.file_name = Some(NAME.into());
    })
    .await
}

fn choose(h: &Harness, when: WhenFileExists) -> TestResult {
    let mut settings = h.manager.get_settings();
    settings.when_file_exists = when;
    h.manager.update_settings(settings)?;
    Ok(())
}

async fn asked(
    events: &mut tokio::sync::broadcast::Receiver<UiEvent>,
) -> TestResult<FileExistsQuestion> {
    let question = tokio::time::timeout(PATIENCE, async {
        loop {
            if let Ok(UiEvent::FileExistsRequested(question)) = events.recv().await {
                return question;
            }
        }
    })
    .await?;
    Ok(question)
}

fn answer(choice: FileExistsChoice, remember: bool) -> FileExistsAnswer {
    FileExistsAnswer { choice, remember }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_duplicate_waits_for_an_answer_by_default() -> TestResult {
    let server = server().await;
    let h = Harness::new().await?;
    let dir = folder_with(&h, &same_size())?;
    let mut events = h.manager.subscribe();
    let row = add_into(&h, server.url("report").as_str(), &dir).await?;

    let question = asked(&mut events).await?;
    assert_eq!(question.download, row.id);
    assert_eq!(question.file_name, NAME);
    assert_eq!(Path::new(&question.folder), dir);
    assert_eq!(question.size, LEN as u64);
    assert_eq!(h.manager.pending_file_exists(), vec![question.clone()]);
    let waiting = h.wait_status(row.id, DownloadStatus::Paused).await;
    assert_eq!(waiting.done, 0);
    assert_eq!(std::fs::read(dir.join(NAME))?, same_size());

    h.manager
        .answer_file_exists(question.id, answer(FileExistsChoice::KeepBoth, false))?;
    let done = h.wait_status(row.id, DownloadStatus::Completed).await;
    assert_eq!(Path::new(&done.path), dir.join("report (1).pdf"));
    assert_eq!(std::fs::read(&done.path)?, server.data);
    assert_eq!(std::fs::read(dir.join(NAME))?, same_size());
    assert_eq!(h.manager.pending_file_exists(), vec![]);
    assert_eq!(
        h.manager.get_settings().when_file_exists,
        WhenFileExists::Ask
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn replace_downloads_over_the_file_and_can_be_remembered() -> TestResult {
    let server = server().await;
    let h = Harness::new().await?;
    let dir = folder_with(&h, &same_size())?;
    let mut events = h.manager.subscribe();
    let row = add_into(&h, server.url("report").as_str(), &dir).await?;
    let question = asked(&mut events).await?;

    h.manager
        .answer_file_exists(question.id, answer(FileExistsChoice::Replace, true))?;
    let done = h.wait_status(row.id, DownloadStatus::Completed).await;
    assert_eq!(Path::new(&done.path), dir.join(NAME));
    assert_eq!(std::fs::read(dir.join(NAME))?, server.data);
    assert_eq!(
        h.manager.get_settings().when_file_exists,
        WhenFileExists::Replace
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cancel_cancels_the_download_and_is_never_remembered() -> TestResult {
    let server = server().await;
    let h = Harness::new().await?;
    let dir = folder_with(&h, &same_size())?;
    let mut events = h.manager.subscribe();
    let row = add_into(&h, server.url("report").as_str(), &dir).await?;
    let question = asked(&mut events).await?;

    h.manager
        .answer_file_exists(question.id, answer(FileExistsChoice::Cancel, true))?;
    h.wait_status(row.id, DownloadStatus::Cancelled).await;
    assert_eq!(std::fs::read(dir.join(NAME))?, same_size());
    assert_eq!(
        h.manager.get_settings().when_file_exists,
        WhenFileExists::Ask
    );
    // A question is answered once.
    assert!(
        h.manager
            .answer_file_exists(question.id, answer(FileExistsChoice::Replace, false))
            .is_err()
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn keep_existing_completes_without_downloading() -> TestResult {
    let server = server().await;
    let h = Harness::new().await?;
    choose(&h, WhenFileExists::KeepExisting)?;
    let dir = folder_with(&h, &same_size())?;
    let row = add_into(&h, server.url("report").as_str(), &dir).await?;

    let done = h.wait_status(row.id, DownloadStatus::Completed).await;
    assert_eq!(Path::new(&done.path), dir.join(NAME));
    assert_eq!((done.done, done.total), (LEN as u64, Some(LEN as u64)));
    assert_eq!(std::fs::read(dir.join(NAME))?, same_size());
    assert_eq!(h.manager.pending_file_exists(), vec![]);
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn replace_and_keep_both_need_no_question_once_chosen() -> TestResult {
    let server = server().await;
    let h = Harness::new().await?;
    let dir = folder_with(&h, &same_size())?;
    choose(&h, WhenFileExists::Replace)?;
    let replaced = add_into(&h, server.url("report").as_str(), &dir).await?;
    let replaced = h.wait_status(replaced.id, DownloadStatus::Completed).await;
    assert_eq!(Path::new(&replaced.path), dir.join(NAME));
    assert_eq!(std::fs::read(dir.join(NAME))?, server.data);

    choose(&h, WhenFileExists::KeepBoth)?;
    let kept = add_into(&h, server.url("report").as_str(), &dir).await?;
    let kept = h.wait_status(kept.id, DownloadStatus::Completed).await;
    assert_eq!(Path::new(&kept.path), dir.join("report (1).pdf"));
    assert_eq!(h.manager.pending_file_exists(), vec![]);
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_file_of_another_size_only_shares_the_name() -> TestResult {
    let server = server().await;
    let h = Harness::new().await?;
    let other = vec![b'x'; LEN + 1];
    let dir = folder_with(&h, &other)?;
    let row = add_into(&h, server.url("report").as_str(), &dir).await?;

    let done = h.wait_status(row.id, DownloadStatus::Completed).await;
    assert_eq!(Path::new(&done.path), dir.join("report (1).pdf"));
    assert_eq!(std::fs::read(dir.join(NAME))?, other);
    assert_eq!(h.manager.pending_file_exists(), vec![]);
    Ok(())
}

/// The list knows where its finished files came from: one from another link only shares the name,
/// the same link again is a duplicate.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_finished_download_from_another_link_only_shares_the_name() -> TestResult {
    let server = server().await;
    let h = Harness::new().await?;
    let dir = h.root.path().join("saved");
    let first = add_into(&h, server.url("a").as_str(), &dir).await?;
    h.wait_status(first.id, DownloadStatus::Completed).await;

    let other = add_into(&h, server.url("b").as_str(), &dir).await?;
    let other = h.wait_status(other.id, DownloadStatus::Completed).await;
    assert_eq!(Path::new(&other.path), dir.join("report (1).pdf"));

    let mut events = h.manager.subscribe();
    let again = add_into(&h, server.url("a").as_str(), &dir).await?;
    let question = asked(&mut events).await?;
    assert_eq!(question.download, again.id);
    Ok(())
}

/// Core's half of ENG-11: a file saved under the download's name after its name was chosen is
/// never replaced; the finished download takes the next free name.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_file_saved_meanwhile_is_never_replaced() -> TestResult {
    let server = TestServer::start(
        test_data(64 * KIB_USIZE),
        Config {
            ranges: true,
            per_connection_bps: Some(16 * KIB),
            ..Config::default()
        },
    )
    .await;
    let h = Harness::new().await?;
    let dir = h.root.path().join("saved");
    let row = add_into(&h, server.url("report").as_str(), &dir).await?;
    h.wait_for(row.id, "the first bytes", |r| r.done > 0).await;
    std::fs::write(dir.join(NAME), b"saved by another program")?;
    server.configure(|c| c.per_connection_bps = None);

    let done = h.wait_status(row.id, DownloadStatus::Completed).await;
    assert_eq!(Path::new(&done.path), dir.join("report (1).pdf"));
    assert_eq!(std::fs::read(&done.path)?, server.data);
    assert_eq!(std::fs::read(dir.join(NAME))?, b"saved by another program");
    Ok(())
}
