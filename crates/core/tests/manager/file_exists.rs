//! A download whose file is already in its folder (D88, D96): `when_file_exists` decides, whatever the file is and wherever its name came from.

use std::path::{Path, PathBuf};

use wirecatch_engine::units::{KIB, KIB_USIZE};
use wirecatch_protocol::app::{
    Category, DownloadStatus, DownloadSummary, FileExistsAnswer, FileExistsChoice,
    FileExistsQuestion, UiEvent, WhenFileExists,
};

use wirecatch_core::{LinkFrom, RemoveFiles};

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
    assert!(question.same_file);
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

/// A different file with the same name is asked about too, and the question says it differs (D96).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_file_of_another_size_is_asked_about_as_a_different_file() -> TestResult {
    let server = server().await;
    let h = Harness::new().await?;
    let other = vec![b'x'; LEN + 1];
    let dir = folder_with(&h, &other)?;
    let mut events = h.manager.subscribe();
    let row = add_into(&h, server.url("report").as_str(), &dir).await?;

    let question = asked(&mut events).await?;
    assert_eq!(question.download, row.id);
    assert!(!question.same_file);
    assert_eq!(std::fs::read(dir.join(NAME))?, other);
    Ok(())
}

/// The list knows where its finished files came from: one from another link is a different file,
/// the same link again looks like the same file.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_finished_download_from_another_link_is_a_different_file() -> TestResult {
    let server = server().await;
    let h = Harness::new().await?;
    let dir = h.root.path().join("saved");
    let first = add_into(&h, server.url("a").as_str(), &dir).await?;
    h.wait_status(first.id, DownloadStatus::Completed).await;

    let mut events = h.manager.subscribe();
    let other = add_into(&h, server.url("b").as_str(), &dir).await?;
    let question = asked(&mut events).await?;
    assert_eq!((question.download, question.same_file), (other.id, false));
    h.manager
        .answer_file_exists(question.id, answer(FileExistsChoice::Cancel, false))?;

    let again = add_into(&h, server.url("a").as_str(), &dir).await?;
    let question = asked(&mut events).await?;
    assert_eq!((question.download, question.same_file), (again.id, true));
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

/// One question is open on the file, so a second link with that name never reaches its path.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_open_question_keeps_its_path_from_other_downloads() -> TestResult {
    let (bytes_one, bytes_two) = (vec![0xAA; LEN], vec![0xBB; LEN]);
    let one = TestServer::start(bytes_one.clone(), Config::default()).await;
    let two = TestServer::start(bytes_two.clone(), Config::default()).await;
    let h = Harness::new().await?;
    let dir = folder_with(&h, b"older file")?;
    let mut events = h.manager.subscribe();
    let first = add_into(&h, one.url("a").as_str(), &dir).await?;
    let question = asked(&mut events).await?;

    let second = add_into(&h, two.url("b").as_str(), &dir).await?;
    let second = h.wait_status(second.id, DownloadStatus::Completed).await;
    assert_ne!(Path::new(&second.path), dir.join(NAME));
    assert_eq!(std::fs::read(&second.path)?, bytes_two);
    assert_eq!(h.manager.pending_file_exists(), vec![question.clone()]);

    h.manager
        .answer_file_exists(question.id, answer(FileExistsChoice::Replace, false))?;
    let first = h.wait_status(first.id, DownloadStatus::Completed).await;
    assert_eq!(Path::new(&first.path), dir.join(NAME));
    assert_eq!(std::fs::read(dir.join(NAME))?, bytes_one);
    assert_eq!(std::fs::read(&second.path)?, bytes_two);
    Ok(())
}

/// A question that closes gives its path back: the next download with that name is asked.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_closed_question_gives_its_path_back() -> TestResult {
    let server = server().await;
    let h = Harness::new().await?;
    let dir = folder_with(&h, &same_size())?;
    let mut events = h.manager.subscribe();
    let first = add_into(&h, server.url("a").as_str(), &dir).await?;
    let question = asked(&mut events).await?;
    h.manager
        .answer_file_exists(question.id, answer(FileExistsChoice::Cancel, false))?;
    h.wait_status(first.id, DownloadStatus::Cancelled).await;

    let second = add_into(&h, server.url("b").as_str(), &dir).await?;
    let question = asked(&mut events).await?;
    assert_eq!(question.download, second.id);
    Ok(())
}

/// So does a question closed by cancelling the download.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cancelling_a_download_that_asks_gives_its_path_back() -> TestResult {
    let server = server().await;
    let h = Harness::new().await?;
    let dir = folder_with(&h, &same_size())?;
    let mut events = h.manager.subscribe();
    let first = add_into(&h, server.url("a").as_str(), &dir).await?;
    asked(&mut events).await?;
    h.wait_status(first.id, DownloadStatus::Paused).await;
    h.manager.cancel(first.id)?;
    h.wait_status(first.id, DownloadStatus::Cancelled).await;

    let second = add_into(&h, server.url("b").as_str(), &dir).await?;
    let question = asked(&mut events).await?;
    assert_eq!(question.download, second.id);
    Ok(())
}

/// Two finished rows on the file in the folder, from Keep the existing file.
async fn two_rows_of_one_file(
    h: &Harness,
    link: &str,
) -> TestResult<(DownloadSummary, DownloadSummary, PathBuf)> {
    choose(h, WhenFileExists::KeepExisting)?;
    let dir = folder_with(h, &same_size())?;
    let first = add_into(h, link, &dir).await?;
    let first = h.wait_status(first.id, DownloadStatus::Completed).await;
    let second = add_into(h, link, &dir).await?;
    let second = h.wait_status(second.id, DownloadStatus::Completed).await;
    assert_eq!(first.path, second.path);
    Ok((first, second, dir.join(NAME)))
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn moving_a_file_moves_every_finished_row_of_it() -> TestResult {
    let server = server().await;
    let h = Harness::new().await?;
    let (first, second, _) = two_rows_of_one_file(&h, server.url("a").as_str()).await?;
    let elsewhere = h.root.path().join("elsewhere");

    h.manager
        .set_download_folder(first.id, &elsewhere.to_string_lossy())
        .await?;

    let moved = elsewhere.join(NAME);
    assert_eq!(std::fs::read(&moved)?, same_size());
    for id in [first.id, second.id] {
        assert_eq!(Path::new(&h.manager.get_download(id)?.path), moved);
    }
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_file_is_deleted_with_the_last_finished_row_of_it() -> TestResult {
    let server = server().await;
    let h = Harness::new().await?;
    let (first, second, file) = two_rows_of_one_file(&h, server.url("a").as_str()).await?;

    h.manager.remove(&[first.id], RemoveFiles::Delete).await?;
    assert_eq!(std::fs::read(&file)?, same_size());

    h.manager.remove(&[second.id], RemoveFiles::Delete).await?;
    assert!(!file.exists());
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn removing_every_row_of_a_file_at_once_deletes_it() -> TestResult {
    let server = server().await;
    let h = Harness::new().await?;
    let (first, second, file) = two_rows_of_one_file(&h, server.url("a").as_str()).await?;

    h.manager
        .remove(&[first.id, second.id], RemoveFiles::Delete)
        .await?;
    assert!(!file.exists());
    Ok(())
}

/// The file stays for the download about to replace it, as it does for a finished row.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_file_another_download_holds_is_kept_when_a_finished_row_of_it_goes() -> TestResult {
    let first = server().await;
    let second = TestServer::start(vec![0xBB; LEN], Config::default()).await;
    let h = Harness::new().await?;
    let dir = h.root.path().join("saved");
    let finished = add_into(&h, first.url("a").as_str(), &dir).await?;
    h.wait_status(finished.id, DownloadStatus::Completed).await;
    let mut events = h.manager.subscribe();
    let replacing = add_into(&h, second.url("b").as_str(), &dir).await?;
    let question = asked(&mut events).await?;
    second.hold();
    h.manager
        .answer_file_exists(question.id, answer(FileExistsChoice::Replace, false))?;

    h.manager
        .remove(&[finished.id], RemoveFiles::Delete)
        .await?;
    assert_eq!(std::fs::read(dir.join(NAME))?, first.data);

    second.release();
    let done = h.wait_status(replacing.id, DownloadStatus::Completed).await;
    assert_eq!(std::fs::read(&done.path)?, second.data);
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_replace_still_waiting_is_not_completed_by_a_restart() -> TestResult {
    let server = server().await;
    let mut h = Harness::new().await?;
    let dir = folder_with(&h, b"the older file")?;
    let mut events = h.manager.subscribe();
    let row = add_into(&h, server.url("report").as_str(), &dir).await?;
    let question = asked(&mut events).await?;
    h.wait_status(row.id, DownloadStatus::Paused).await;
    h.manager
        .set_queue_paused(wirecatch_core::DEFAULT_QUEUE_ID, true)?;
    h.manager
        .answer_file_exists(question.id, answer(FileExistsChoice::Replace, false))?;
    h.wait_status(row.id, DownloadStatus::Queued).await;

    h.restart().await?;

    assert_ne!(
        h.row(row.id).status,
        DownloadStatus::Completed,
        "completed on the file it was to replace"
    );
    Ok(())
}

/// Resume all closes the question (D88.3), and a late answer never touches a finished row.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn resume_all_closes_the_question() -> TestResult {
    let server = server().await;
    let h = Harness::new().await?;
    let dir = folder_with(&h, &same_size())?;
    let mut events = h.manager.subscribe();
    let row = add_into(&h, server.url("report").as_str(), &dir).await?;
    let question = asked(&mut events).await?;
    h.wait_status(row.id, DownloadStatus::Paused).await;
    std::fs::remove_file(dir.join(NAME))?;
    h.manager.resume_all()?;
    h.wait_status(row.id, DownloadStatus::Completed).await;
    assert_eq!(h.manager.pending_file_exists(), vec![]);
    assert!(
        h.manager
            .answer_file_exists(question.id, answer(FileExistsChoice::Replace, false))
            .is_err()
    );
    assert_eq!(h.row(row.id).status, DownloadStatus::Completed);
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_new_link_closes_the_question() -> TestResult {
    let server = server().await;
    let h = Harness::new().await?;
    let dir = folder_with(&h, &same_size())?;
    let mut events = h.manager.subscribe();
    let row = add_into(&h, server.url("report").as_str(), &dir).await?;
    asked(&mut events).await?;
    h.manager
        .refresh_link(row.id, server.url("again").as_str(), LinkFrom::Person)?;
    assert!(
        !h.manager
            .pending_file_exists()
            .iter()
            .any(|q| q.url.ends_with("report"))
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_move_closes_the_question() -> TestResult {
    let server = server().await;
    let h = Harness::new().await?;
    let dir = folder_with(&h, &same_size())?;
    let mut events = h.manager.subscribe();
    let row = add_into(&h, server.url("report").as_str(), &dir).await?;
    let question = asked(&mut events).await?;
    h.wait_status(row.id, DownloadStatus::Paused).await;
    let elsewhere = h.root.path().join("elsewhere");
    h.manager
        .set_download_folder(row.id, &elsewhere.to_string_lossy())
        .await?;
    assert!(
        h.manager
            .answer_file_exists(question.id, answer(FileExistsChoice::Replace, false))
            .is_err()
    );
    assert_eq!(h.row(row.id).status, DownloadStatus::Paused);
    Ok(())
}

/// A server that names the file against what the link suggests.
async fn naming_server(name: &str) -> TestServer {
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

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn keep_existing_takes_the_category_of_the_servers_name() -> TestResult {
    let server = naming_server(NAME).await;
    let h = Harness::new().await?;
    choose(&h, WhenFileExists::KeepExisting)?;
    let dir = folder_with(&h, &same_size())?;
    let row = h
        .add_with(server.url("archive.zip").as_str(), |r| {
            r.folder = Some(dir.to_string_lossy().into_owned());
        })
        .await?;
    assert_eq!(row.category, Category::Archive);
    let done = h.wait_status(row.id, DownloadStatus::Completed).await;
    assert_eq!(done.category, Category::Document);
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn replace_takes_the_category_of_the_servers_type() -> TestResult {
    let server = TestServer::start(
        test_data(LEN),
        Config {
            ranges: true,
            content_disposition: Some("attachment; filename=\"talk\"".into()),
            content_type: Some("audio/mpeg".into()),
            ..Config::default()
        },
    )
    .await;
    let h = Harness::new().await?;
    choose(&h, WhenFileExists::Replace)?;
    let dir = h.root.path().join("saved");
    std::fs::create_dir_all(&dir)?;
    std::fs::write(dir.join("talk"), b"older")?;
    let row = h
        .add_with(server.url("archive.zip").as_str(), |r| {
            r.folder = Some(dir.to_string_lossy().into_owned());
        })
        .await?;
    let done = h.wait_status(row.id, DownloadStatus::Completed).await;
    assert_eq!(done.category, Category::Audio);
    assert_eq!(std::fs::read(dir.join("talk"))?, server.data);
    Ok(())
}

/// The folder a person picks is theirs, also when the guess already points at it.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn choosing_the_folder_of_the_guess_is_remembered() -> TestResult {
    let server = naming_server(NAME).await;
    let h = Harness::new().await?;
    let row = h
        .add_with(server.url("archive.zip").as_str(), |r| r.start = false)
        .await?;
    let guessed = Path::new(&row.path)
        .parent()
        .ok_or("no folder")?
        .to_path_buf();
    h.manager
        .set_download_folder(row.id, &guessed.to_string_lossy())
        .await?;
    h.manager.resume(row.id)?;
    let done = h.wait_status(row.id, DownloadStatus::Completed).await;
    assert_eq!(Path::new(&done.path), guessed.join(NAME));
    Ok(())
}

/// A partial file left beside the name does not hide the file from the setting.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_file_beside_a_leftover_resume_file_is_asked_about() -> TestResult {
    let server = server().await;
    let h = Harness::new().await?;
    let dir = folder_with(&h, &same_size())?;
    let [_, state] = crate::common::partial_files(&dir.join(NAME).to_string_lossy());
    std::fs::write(state, b"left behind")?;
    let mut events = h.manager.subscribe();
    let row = add_into(&h, server.url("report").as_str(), &dir).await?;
    let question = asked(&mut events).await?;
    assert_eq!(question.download, row.id);
    Ok(())
}

/// The engine continues the part file at an exact path whatever its link, so Replace must not meet another link's. Same size and no validators: only the link tells them apart.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn replace_never_continues_a_part_file_another_link_left() -> TestResult {
    const BIG: usize = 4 * 1024 * KIB_USIZE;
    // Far slower than any wait below: it cannot finish before the pause.
    let other = TestServer::start(
        vec![0xAA; BIG],
        Config {
            ranges: true,
            per_connection_bps: Some(16 * KIB),
            ..Config::default()
        },
    )
    .await;
    let wanted = TestServer::start(vec![0xBB; BIG], ranged()).await;
    let h = Harness::new().await?;
    let dir = h.root.path().join("saved");
    let left = add_into(&h, other.url("a").as_str(), &dir).await?;
    h.wait_for(left.id, "the other link's first bytes", |r| r.done > 0)
        .await;
    h.manager.pause(left.id)?;
    let left = h.wait_status(left.id, DownloadStatus::Paused).await;
    let [part, state] = crate::common::partial_files(&left.path);
    let kept = (std::fs::read(&part)?, std::fs::read(&state)?);
    assert!(kept.0.contains(&0xAA), "the part file holds no bytes yet");
    h.manager.remove(&[left.id], RemoveFiles::Delete).await?;
    std::fs::write(&part, &kept.0)?;
    std::fs::write(&state, &kept.1)?;
    std::fs::write(&left.path, b"older file")?;

    choose(&h, WhenFileExists::Replace)?;
    let row = add_into(&h, wanted.url("b").as_str(), &dir).await?;
    let done = h.wait_status(row.id, DownloadStatus::Completed).await;
    assert_eq!(done.path, left.path);
    assert!(
        std::fs::read(&done.path)? == wanted.data,
        "the file holds bytes of another link"
    );
    Ok(())
}

fn ranged() -> Config {
    Config {
        ranges: true,
        ..Config::default()
    }
}

/// An answer stays an answer across a restart.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_answered_replace_survives_a_restart() -> TestResult {
    let server = TestServer::start(
        // Far slower than any wait below: it cannot finish before the pause.
        test_data(4096 * KIB_USIZE),
        Config {
            ranges: true,
            per_connection_bps: Some(16 * KIB),
            ..Config::default()
        },
    )
    .await;
    let mut h = Harness::new().await?;
    let dir = folder_with(&h, b"the older file")?;
    let mut events = h.manager.subscribe();
    let row = add_into(&h, server.url("report").as_str(), &dir).await?;
    let question = asked(&mut events).await?;
    h.manager
        .answer_file_exists(question.id, answer(FileExistsChoice::Replace, false))?;
    h.wait_for(row.id, "some progress", |r| r.done > 0).await;
    h.manager.pause(row.id)?;
    h.wait_status(row.id, DownloadStatus::Paused).await;

    h.restart().await?;
    server.configure(|c| c.per_connection_bps = None);
    h.manager.resume(row.id)?;
    let done = h.wait_status(row.id, DownloadStatus::Completed).await;

    assert_eq!(
        Path::new(&done.path),
        dir.join(NAME),
        "the Replace was forgotten"
    );
    Ok(())
}

async fn add_named_by_the_link(
    h: &Harness,
    server: &TestServer,
    dir: &Path,
) -> TestResult<DownloadSummary> {
    h.add_with(server.url(NAME).as_str(), |r| {
        r.folder = Some(dir.to_string_lossy().into_owned());
    })
    .await
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_name_from_the_link_is_asked_about_too() -> TestResult {
    let server = server().await;
    let h = Harness::new().await?;
    let dir = folder_with(&h, &same_size())?;
    let mut events = h.manager.subscribe();
    let row = add_named_by_the_link(&h, &server, &dir).await?;
    assert_eq!(row.file_name, NAME);

    let question = asked(&mut events).await?;
    assert_eq!(question.download, row.id);
    assert_eq!(question.file_name, NAME);
    h.wait_status(row.id, DownloadStatus::Paused).await;
    assert_eq!(std::fs::read(dir.join(NAME))?, same_size());

    h.manager
        .answer_file_exists(question.id, answer(FileExistsChoice::KeepBoth, false))?;
    let done = h.wait_status(row.id, DownloadStatus::Completed).await;
    assert_eq!(Path::new(&done.path), dir.join("report (1).pdf"));
    assert_eq!(std::fs::read(dir.join(NAME))?, same_size());
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_name_from_the_link_replaces_when_the_setting_says_so() -> TestResult {
    let server = server().await;
    let h = Harness::new().await?;
    choose(&h, WhenFileExists::Replace)?;
    let dir = folder_with(&h, &same_size())?;
    let row = add_named_by_the_link(&h, &server, &dir).await?;
    let done = h.wait_status(row.id, DownloadStatus::Completed).await;
    assert_eq!(Path::new(&done.path), dir.join(NAME));
    assert_eq!(std::fs::read(dir.join(NAME))?, server.data);
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_name_from_the_link_takes_the_next_free_name_under_keep_both() -> TestResult {
    let server = server().await;
    let h = Harness::new().await?;
    choose(&h, WhenFileExists::KeepBoth)?;
    let dir = folder_with(&h, &same_size())?;
    let row = add_named_by_the_link(&h, &server, &dir).await?;
    let done = h.wait_status(row.id, DownloadStatus::Completed).await;
    assert_eq!(Path::new(&done.path), dir.join("report (1).pdf"));
    assert_eq!(std::fs::read(dir.join(NAME))?, same_size());
    Ok(())
}

/// A file in the folder it was added to says nothing about the folder it is moved to.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_name_from_the_link_stays_plain_in_a_folder_chosen_before_the_answer() -> TestResult {
    let server = server().await;
    server.hold();
    let h = Harness::new().await?;
    let dir = folder_with(&h, &same_size())?;
    let row = add_named_by_the_link(&h, &server, &dir).await?;
    let elsewhere = h.root.path().join("elsewhere");
    h.manager
        .set_download_folder(row.id, &elsewhere.to_string_lossy())
        .await?;
    server.release();
    let done = h.wait_status(row.id, DownloadStatus::Completed).await;
    assert_eq!(Path::new(&done.path), elsewhere.join(NAME));
    assert_eq!(std::fs::read(dir.join(NAME))?, same_size());
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_name_from_the_link_is_asked_about_again_when_a_cancelled_download_is_retried()
-> TestResult {
    let server = server().await;
    let mut h = Harness::new().await?;
    let dir = folder_with(&h, &same_size())?;
    let mut events = h.manager.subscribe();
    let row = add_named_by_the_link(&h, &server, &dir).await?;
    let question = asked(&mut events).await?;
    h.manager
        .answer_file_exists(question.id, answer(FileExistsChoice::Cancel, false))?;
    let cancelled = h.wait_status(row.id, DownloadStatus::Cancelled).await;
    assert_eq!(cancelled.file_name, NAME);

    h.restart().await?;
    assert_eq!(h.row(row.id).file_name, NAME);
    let mut events = h.manager.subscribe();
    h.manager.retry(row.id)?;
    let question = asked(&mut events).await?;
    assert_eq!(question.download, row.id);
    assert_eq!(question.file_name, NAME);
    h.manager
        .answer_file_exists(question.id, answer(FileExistsChoice::Replace, false))?;
    let done = h.wait_status(row.id, DownloadStatus::Completed).await;
    assert_eq!(Path::new(&done.path), dir.join(NAME));
    assert_eq!(std::fs::read(dir.join(NAME))?, server.data);
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_download_cancelled_before_it_ran_leaves_the_name_to_the_next() -> TestResult {
    let server = server().await;
    let h = Harness::new().await?;
    let dir = folder_with(&h, &same_size())?;
    let waiting = h
        .add_with(server.url(NAME).as_str(), |r| {
            r.folder = Some(dir.to_string_lossy().into_owned());
            r.start = false;
        })
        .await?;
    h.manager.cancel(waiting.id)?;
    h.wait_status(waiting.id, DownloadStatus::Cancelled).await;

    let mut events = h.manager.subscribe();
    let next = add_named_by_the_link(&h, &server, &dir).await?;
    assert_eq!(next.file_name, NAME);
    assert_eq!(asked(&mut events).await?.download, next.id);
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_download_cancelled_before_the_answer_leaves_the_name_to_the_next() -> TestResult {
    let server = server().await;
    server.hold();
    let h = Harness::new().await?;
    let dir = folder_with(&h, &same_size())?;
    let running = add_named_by_the_link(&h, &server, &dir).await?;
    h.manager.cancel(running.id)?;
    h.wait_status(running.id, DownloadStatus::Cancelled).await;
    server.release();

    let mut events = h.manager.subscribe();
    let next = add_named_by_the_link(&h, &server, &dir).await?;
    assert_eq!(next.file_name, NAME);
    assert_eq!(asked(&mut events).await?.download, next.id);
    Ok(())
}

/// Removing a download that never got an answer leaves the file its name only points at.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn removing_a_download_before_the_answer_keeps_the_file_of_that_name() -> TestResult {
    let server = server().await;
    server.hold();
    let h = Harness::new().await?;
    let dir = folder_with(&h, &same_size())?;
    let row = add_named_by_the_link(&h, &server, &dir).await?;
    h.manager.remove(&[row.id], RemoveFiles::Delete).await?;
    assert_eq!(std::fs::read(dir.join(NAME))?, same_size());
    Ok(())
}
