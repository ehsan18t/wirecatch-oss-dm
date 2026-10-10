//! Acceptance 1: after a restart every download is back with its status and
//! progress, and the ones that were running continue on their own.

use wirecatch_protocol::app::DownloadStatus;

use crate::common::{Harness, TestServer, is_running};

const KIB: usize = 1024;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_file_that_finished_before_the_restart_is_not_downloaded_again() {
    let slow = TestServer::throttled(512 * KIB, 64 * KIB as u64).await;
    let mut h = Harness::new().await;
    // Two downloads whose server has answered, so their paths are final.
    let mut settled = Vec::new();
    for name in ["done.bin", "half.bin"] {
        let row = h.add(&slow.url(name)).await;
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
        .await;

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

    h.restart().await;

    let row = h.row(finished.id);
    assert_eq!(row.status, DownloadStatus::Completed);
    assert_eq!((row.done, row.total), (5, Some(5)));
    // Still has its partial files: not finished.
    assert_eq!(h.row(unfinished.id).status, DownloadStatus::Paused);
    // A file at a guessed path is not this download's.
    assert_eq!(h.row(guessed.id).status, DownloadStatus::Paused);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ids_are_never_reused_after_deleting_the_newest() {
    let mut h = Harness::new().await;
    let paused = |r: &mut wirecatch_protocol::app::AddRequest| r.start = false;
    h.add_with("https://example.com/a.bin", paused).await;
    let newest = h.add_with("https://example.com/b.bin", paused).await;
    h.manager.remove(&[newest.id], false).await.unwrap();
    h.manager.create_queue("Kept").unwrap();
    let newest_queue = h.manager.create_queue("Deleted").unwrap();
    h.manager.delete_queue(newest_queue.id).unwrap();

    h.restart().await;

    let next = h.add_with("https://example.com/c.bin", paused).await;
    assert!(next.id.0 > newest.id.0, "download id {} reused", next.id.0);
    let queue = h.manager.create_queue("New").unwrap();
    assert!(
        queue.id.0 > newest_queue.id.0,
        "queue id {} reused",
        queue.id.0
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn downloads_survive_a_restart_and_running_ones_continue() {
    let slow = TestServer::throttled(512 * KIB, 256 * KIB as u64).await;
    let fast = TestServer::fast(32 * KIB).await;
    let mut h = Harness::new().await;

    let finished = h.add(&fast.url("done.bin")).await;
    h.wait_status(finished.id, DownloadStatus::Completed).await;
    let paused = h
        .add_with(&fast.url("later.bin"), |r| r.start = false)
        .await;
    let running = h.add(&slow.url("big.bin")).await;
    let before = h
        .wait_for(running.id, "some progress", |r| r.done >= 64 * KIB as u64)
        .await;
    assert!(is_running(&before));

    h.restart().await;

    // Everything is back, with progress no lower than before.
    let ids: Vec<_> = h.manager.list_downloads().iter().map(|r| r.id).collect();
    assert_eq!(ids, vec![finished.id, paused.id, running.id]);
    let finished_row = h.row(finished.id);
    assert_eq!(finished_row.status, DownloadStatus::Completed);
    assert_eq!(finished_row.done, 32 * KIB as u64);
    assert_eq!(h.row(paused.id).status, DownloadStatus::Paused);
    let resumed = h.row(running.id);
    assert!(is_running(&resumed), "{:?}", resumed.status);
    assert!(
        resumed.done >= before.done,
        "{} < {}",
        resumed.done,
        before.done
    );
    assert_eq!(resumed.total, Some(512 * KIB as u64));

    // The interrupted download finishes without being asked, continuing where it was.
    let done = h.wait_status(running.id, DownloadStatus::Completed).await;
    assert_eq!(std::fs::read(&done.path).unwrap(), slow.data);
    assert!(
        slow.bytes_sent() < (512 + 256) as u64 * KIB as u64,
        "sent {} bytes, so the download started over",
        slow.bytes_sent()
    );
    // Nothing that was paused started by itself.
    assert_eq!(h.row(paused.id).status, DownloadStatus::Paused);
}
