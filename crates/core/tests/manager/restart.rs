//! Acceptance 1: after a restart every download is back with its status and
//! progress, and the ones that were running continue on their own.

use wirecatch_protocol::app::DownloadStatus;

use crate::common::{Harness, TestResult, fast_server, is_running, throttled_server};
use wirecatch_engine::units::KIB;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_file_that_finished_before_the_restart_is_not_downloaded_again() -> TestResult {
    let slow = throttled_server(512 * KIB, 64 * KIB).await?;
    let mut h = Harness::new().await?;
    // Two downloads whose server has answered, so their paths are final.
    let mut settled = Vec::new();
    for name in ["done.bin", "half.bin"] {
        let row = h.add(&slow.url(name)).await?;
        h.wait_for(row.id, "the server's answer", |r| r.done > 0)
            .await;
        h.manager.pause(row.id).unwrap();
        settled.push(h.wait_status(row.id, DownloadStatus::Paused).await);
    }
    let [finished, unfinished] = &settled[..] else {
        unreachable!()
    };
    // One that never ran: its path is still a guess.
    let guessed = h
        .add_with(&slow.url("guess.bin"), |r| r.start = false)
        .await?;

    // The engine renamed the first one's part file just before the app stopped.
    let partials = |path: &str| {
        [wirecatch_engine::PART_EXT, wirecatch_engine::STATE_EXT].map(|ext| format!("{path}{ext}"))
    };
    for partial in partials(&finished.path) {
        std::fs::remove_file(partial).unwrap();
    }
    for row in [finished, unfinished, &guessed] {
        let path = std::path::Path::new(&row.path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, b"12345").unwrap();
    }

    h.restart().await?;

    let row = h.row(finished.id);
    assert_eq!(row.status, DownloadStatus::Completed);
    assert_eq!((row.done, row.total), (5, Some(5)));
    // Still has its partial files: not finished.
    assert_eq!(h.row(unfinished.id).status, DownloadStatus::Paused);
    // A file at a guessed path is not this download's.
    assert_eq!(h.row(guessed.id).status, DownloadStatus::Paused);
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ids_are_never_reused_after_deleting_the_newest() -> TestResult {
    let mut h = Harness::new().await?;
    let paused = |r: &mut wirecatch_protocol::app::AddRequest| r.start = false;
    h.add_with("https://example.com/a.bin", paused).await?;
    let newest = h.add_with("https://example.com/b.bin", paused).await?;
    h.manager.remove(&[newest.id], false).await.unwrap();
    h.manager.create_queue("Kept").unwrap();
    let newest_queue = h.manager.create_queue("Deleted").unwrap();
    h.manager.delete_queue(newest_queue.id).unwrap();

    h.restart().await?;

    let next = h.add_with("https://example.com/c.bin", paused).await?;
    assert!(next.id.0 > newest.id.0, "download id {} reused", next.id.0);
    let queue = h.manager.create_queue("New").unwrap();
    assert!(
        queue.id.0 > newest_queue.id.0,
        "queue id {} reused",
        queue.id.0
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn downloads_survive_a_restart_and_running_ones_continue() -> TestResult {
    let slow = throttled_server(512 * KIB, 256 * KIB).await?;
    let fast = fast_server(32 * KIB).await?;
    let mut h = Harness::new().await?;

    let finished = h.add(&fast.url("done.bin")).await?;
    h.wait_status(finished.id, DownloadStatus::Completed).await;
    let paused = h
        .add_with(&fast.url("later.bin"), |r| r.start = false)
        .await?;
    let running = h.add(&slow.url("big.bin")).await?;
    let before = h
        .wait_for(running.id, "some progress", |r| r.done >= 64 * KIB)
        .await;
    assert!(is_running(&before));

    h.restart().await?;

    // Everything is back, with progress no lower than before.
    let ids: Vec<_> = h.manager.list_downloads().iter().map(|r| r.id).collect();
    assert_eq!(ids, vec![finished.id, paused.id, running.id]);
    let finished_row = h.row(finished.id);
    assert_eq!(finished_row.status, DownloadStatus::Completed);
    assert_eq!(finished_row.done, 32 * KIB);
    assert_eq!(h.row(paused.id).status, DownloadStatus::Paused);
    let resumed = h.row(running.id);
    assert!(is_running(&resumed), "{:?}", resumed.status);
    assert!(
        resumed.done >= before.done,
        "{} < {}",
        resumed.done,
        before.done
    );
    assert_eq!(resumed.total, Some(512 * KIB));

    // The interrupted download finishes without being asked, continuing where it was.
    let done = h.wait_status(running.id, DownloadStatus::Completed).await;
    assert_eq!(std::fs::read(&done.path).unwrap(), slow.data);
    assert!(
        slow.bytes_sent() < (512 + 256) * KIB,
        "sent {} bytes, so the download started over",
        slow.bytes_sent()
    );
    // Nothing that was paused started by itself.
    assert_eq!(h.row(paused.id).status, DownloadStatus::Paused);
    Ok(())
}

/// The desktop app and the end-to-end tests delete or move the data folder right
/// after shutting down; on Windows that fails while the database file is open.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_data_folder_can_be_deleted_right_after_shutdown() -> TestResult {
    let server = fast_server(64 * KIB).await?;
    let h = Harness::new().await?;
    let row = h.add(&server.url("f.bin")).await?;
    h.wait_status(row.id, DownloadStatus::Completed).await;
    h.manager.shutdown().await;
    let data = h.root.path().join("data");
    assert!(data.join(wirecatch_core::DATABASE_FILE).is_file());
    std::fs::remove_dir_all(&data).expect("the data folder is still in use after shutdown");
    Ok(())
}

/// A damaged download list used to stop the app from starting at all. Now it is set aside, a new list opens, and the person is told where the old one is.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_damaged_download_list_is_set_aside_and_the_person_is_told() -> TestResult {
    let mut h = Harness::new().await?;
    assert!(
        h.manager.startup_notices().is_empty(),
        "a normal start has nothing to say"
    );
    h.manager.shutdown().await;
    let database = h
        .root
        .path()
        .join("data")
        .join(wirecatch_core::DATABASE_FILE);
    std::fs::write(&database, b"not a database".repeat(400)).unwrap();

    h.restart().await?;

    let [notice] = h.manager.startup_notices() else {
        panic!("the person is told once: {:?}", h.manager.startup_notices());
    };
    assert!(notice.message.contains("damaged"), "{}", notice.message);
    assert!(
        notice.message.contains("still in their folders"),
        "{}",
        notice.message
    );
    assert_eq!(
        h.manager.list_downloads(),
        [] as [wirecatch_protocol::app::DownloadSummary; 0]
    );
    Ok(())
}
