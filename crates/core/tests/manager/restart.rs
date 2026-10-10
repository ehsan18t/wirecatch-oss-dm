//! Acceptance 1: after a restart every download is back with its status and
//! progress, and the ones that were running continue on their own.

use wirecatch_protocol::app::DownloadStatus;

use crate::common::{Harness, TestServer, is_running};

const KIB: usize = 1024;

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
