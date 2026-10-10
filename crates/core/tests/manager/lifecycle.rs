//! Acceptance 8 (expired link and refresh) and the rest of a download's life:
//! naming, cancel, retry, remove, queues and ordering.

use std::time::Duration;

use wirecatch_core::{CoreError, DEFAULT_QUEUE_ID};
use wirecatch_protocol::DownloadId;
use wirecatch_protocol::app::{DownloadStatus, FixAction, UiEvent};

use crate::common::{Harness, SilentServer, TestServer, exists, partial_files, wait_until};

const KIB: u64 = 1024;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_expired_link_is_refreshed_without_losing_bytes() {
    let len = 512 * KIB;
    let server = TestServer::throttled(len as usize, 128 * KIB).await;
    let h = Harness::new().await;
    let row = h.add(&server.url("old/file.bin")).await;
    h.wait_for(row.id, "a good part downloaded", |r| r.done >= 128 * KIB)
        .await;
    h.manager.pause(row.id).unwrap();
    let paused = h.wait_status(row.id, DownloadStatus::Paused).await;
    assert!(paused.done >= 128 * KIB);

    // The link expires while the download is paused.
    server.configure(|c| c.refuse.push(("/old/file.bin".into(), 410)));
    h.manager.resume(row.id).unwrap();
    let failed = h.wait_status(row.id, DownloadStatus::Failed).await;
    let error = failed.error.expect("a failed download explains why");
    assert_eq!(error.fix, Some(FixAction::RefreshLink));
    assert!(error.message.contains("expired"), "{}", error.message);
    assert_eq!(failed.done, paused.done, "progress was lost");

    let sent_before = server.bytes_sent();
    h.manager
        .refresh_link(row.id, &server.url("new/file.bin"))
        .unwrap();
    let done = h.wait_status(row.id, DownloadStatus::Completed).await;
    assert_eq!(done.path, row.path, "the file moved");
    assert_eq!(done.url, server.url("new/file.bin"));
    assert_eq!(std::fs::read(&done.path).unwrap(), server.data);
    let sent_after_refresh = server.bytes_sent() - sent_before;
    // Only the missing part came again (plus at most a little of the first answer).
    assert!(
        sent_after_refresh <= len - paused.done + 32 * KIB,
        "sent {sent_after_refresh} bytes after refreshing; {} were already here",
        paused.done
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cancel_discards_progress_and_retry_starts_over() {
    let server = TestServer::throttled(512 * KIB as usize, 128 * KIB).await;
    let h = Harness::new().await;
    let row = h.add(&server.url("file.bin")).await;
    h.wait_for(row.id, "progress", |r| r.done > 0).await;

    h.manager.cancel(row.id).unwrap();
    let cancelled = h.wait_status(row.id, DownloadStatus::Cancelled).await;
    assert_eq!(cancelled.done, 0);
    assert_eq!(cancelled.error, None);
    let partials = partial_files(&row.path);
    wait_until("partial files deleted", || {
        partials.iter().all(|p| !exists(p)).then_some(())
    })
    .await;

    server.configure(|c| c.per_connection_bps = None);
    h.manager.retry(row.id).unwrap();
    let done = h.wait_status(row.id, DownloadStatus::Completed).await;
    assert_eq!(std::fs::read(&done.path).unwrap(), server.data);
    assert!(matches!(
        h.manager.cancel(row.id),
        Err(CoreError::WrongState(_))
    ));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_ui_hears_when_saving_keeps_failing() {
    let h = Harness::new().await;
    let mut events = h.manager.subscribe();
    let db = h
        .root
        .path()
        .join("data")
        .join(wirecatch_core::DATABASE_FILE);
    let other = rusqlite::Connection::open(db).unwrap();
    other.execute("DROP TABLE downloads", []).unwrap();

    h.add_with("https://example.com/a.bin", |r| r.start = false)
        .await;
    let error = tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            if let Ok(UiEvent::StorageFailed(error)) = events.recv().await {
                return error;
            }
        }
    })
    .await
    .expect("no StorageFailed event");
    assert_eq!(error.fix, Some(FixAction::FreeDiskSpace));
    assert!(
        error.message.contains("could not save"),
        "{}",
        error.message
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn resume_and_retry_pressed_while_stopping_are_not_lost() {
    let server = TestServer::throttled(256 * KIB as usize, 64 * KIB).await;
    let h = Harness::new().await;
    let resumed = h.add(&server.url("resumed.bin")).await.id;
    let all = h.add(&server.url("all.bin")).await.id;
    let retried = h.add(&server.url("retried.bin")).await.id;
    for id in [resumed, all, retried] {
        h.wait_for(id, "progress", |r| r.done > 0).await;
    }
    server.configure(|c| c.per_connection_bps = None);

    // Each second call arrives while the first one's stop is still finishing.
    h.manager.pause(resumed).unwrap();
    h.manager.resume(resumed).unwrap();
    h.manager.pause(all).unwrap();
    h.manager.resume_all().unwrap();
    h.manager.cancel(retried).unwrap();
    h.manager.retry(retried).unwrap();

    for id in [resumed, all, retried] {
        let done = h.wait_status(id, DownloadStatus::Completed).await;
        assert_eq!(std::fs::read(&done.path).unwrap(), server.data);
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_download_being_removed_cannot_be_brought_back() {
    let silent = SilentServer::start().await;
    let h = Harness::with_stop_timeout(Duration::from_millis(500)).await;
    let row = h.add(&silent.url("gone.bin")).await;
    let manager = h.manager.clone();
    let removal = tokio::spawn(async move { manager.remove(&[row.id], false).await });
    wait_until("the removal to begin", || {
        h.manager.list_downloads().is_empty().then_some(())
    })
    .await;

    // The run is still stopping; nothing may revive the download meanwhile.
    let gone =
        |result: wirecatch_core::Result<()>| matches!(result, Err(CoreError::DownloadNotFound(_)));
    assert!(gone(h.manager.resume(row.id)));
    assert!(gone(h.manager.retry(row.id)));
    assert!(gone(
        h.manager.refresh_link(row.id, &silent.url("again.bin"))
    ));
    assert!(gone(h.manager.move_to_queue(&[row.id], DEFAULT_QUEUE_ID)));
    h.manager.resume_all().unwrap();

    removal.await.unwrap().unwrap();
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert!(h.manager.list_downloads().is_empty());
    assert_eq!(
        silent.accepted.load(std::sync::atomic::Ordering::SeqCst),
        1,
        "a removed download connected again"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_server_that_never_answers_cannot_hang_the_app() {
    let silent = SilentServer::start().await;
    let h = Harness::with_stop_timeout(Duration::from_millis(300)).await;
    let paused = h.add(&silent.url("pause.bin")).await;
    let removed = h.add(&silent.url("remove.bin")).await;
    let left = h.add(&silent.url("left.bin")).await;
    for row in [&paused, &removed, &left] {
        assert_eq!(row.status, DownloadStatus::Connecting);
    }
    let patience = Duration::from_secs(5);

    h.manager.pause(paused.id).unwrap();
    h.wait_status(paused.id, DownloadStatus::Paused).await;
    tokio::time::timeout(patience, h.manager.remove(&[removed.id], false))
        .await
        .expect("remove waited forever")
        .unwrap();
    tokio::time::timeout(patience, h.manager.shutdown())
        .await
        .expect("shutdown waited forever");
    // Interrupted by shutdown, so it continues on the next start.
    assert_eq!(h.row(left.id).status, DownloadStatus::Connecting);
}

/// Tauri runs synchronous commands on its main thread, outside any Tokio runtime.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn sync_methods_work_from_a_thread_outside_the_runtime() {
    let server = TestServer::fast(4 * KIB as usize).await;
    let h = Harness::new().await;
    let paused = |r: &mut wirecatch_protocol::app::AddRequest| r.start = false;
    let started = h.add_with(&server.url("a.bin"), paused).await;
    let cancelled = h.add_with(&server.url("b.bin"), paused).await;
    let manager = h.manager.clone();
    std::thread::spawn(move || {
        manager.pause_all().unwrap();
        manager.resume_all().unwrap();
        manager.resume(started.id).unwrap();
        manager.cancel(cancelled.id).unwrap();
        let queue = manager.create_queue("From a thread").unwrap();
        manager.update_queue(queue).unwrap();
        let settings = manager.get_settings();
        manager.update_settings(settings).unwrap();
    })
    .join()
    .expect("a manager method needed a Tokio runtime on the calling thread");
    h.wait_status(started.id, DownloadStatus::Completed).await;
    h.wait_status(cancelled.id, DownloadStatus::Cancelled).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_waiting_download_can_be_cancelled_and_removed() {
    let h = Harness::new().await;
    let row = h
        .add_with("https://example.com/later.bin", |r| r.start = false)
        .await;
    h.manager.cancel(row.id).unwrap();
    assert_eq!(h.row(row.id).status, DownloadStatus::Cancelled);
    h.manager.remove(&[row.id], false).await.unwrap();
    assert!(h.manager.list_downloads().is_empty());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn remove_stops_and_cleans_up() {
    let slow = TestServer::throttled(512 * KIB as usize, 64 * KIB).await;
    let fast = TestServer::fast(8 * KIB as usize).await;
    let h = Harness::new().await;
    let mut events = h.manager.subscribe();

    let finished = h.add(&fast.url("keep.bin")).await;
    let finished = h.wait_status(finished.id, DownloadStatus::Completed).await;
    let running = h.add(&slow.url("drop.bin")).await;
    h.wait_for(running.id, "progress", |r| r.done > 0).await;

    h.manager
        .remove(&[running.id, DownloadId(999)], false)
        .await
        .unwrap();
    assert!(partial_files(&running.path).iter().all(|p| !exists(p)));
    h.manager.remove(&[finished.id], true).await.unwrap();
    assert!(!exists(&finished.path));
    assert!(h.manager.list_downloads().is_empty());

    let mut removed = Vec::new();
    while let Ok(event) = events.try_recv() {
        if let UiEvent::DownloadsRemoved(ids) = event {
            removed.extend(ids);
        }
    }
    assert_eq!(removed, vec![running.id, finished.id]);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn names_are_unique_and_folders_follow_categories() {
    let h = Harness::new().await;
    let paused = |r: &mut wirecatch_protocol::app::AddRequest| r.start = false;
    let first = h
        .add_with("https://example.com/a/clip.MP4?x=1", paused)
        .await;
    let second = h.add_with("https://example.com/b/clip.MP4", paused).await;
    let videos = h.folders.downloads.join("Video");
    assert_eq!(std::path::Path::new(&first.path), videos.join("clip.MP4"));
    assert_eq!(
        std::path::Path::new(&second.path),
        videos.join("clip (1).MP4")
    );
    assert_eq!(second.file_name, "clip (1).MP4");

    let renamed = h
        .add_with("https://example.com/dl?id=3", |r| {
            r.start = false;
            r.file_name = Some("disk.iso".into());
        })
        .await;
    assert_eq!(
        std::path::Path::new(&renamed.path),
        h.folders.downloads.join("Compressed").join("disk.iso")
    );

    let err = h
        .manager
        .add_download(wirecatch_protocol::app::AddRequest {
            url: "ftp://example.com/x".into(),
            queue: None,
            folder: None,
            file_name: None,
            start: true,
        })
        .await
        .unwrap_err();
    assert!(matches!(err, CoreError::InvalidUrl(_)));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_folder_that_cannot_be_created_asks_for_another() {
    let server = TestServer::fast(1024).await;
    let h = Harness::new().await;
    let blocker = h.root.path().join("not-a-folder");
    std::fs::write(&blocker, b"x").unwrap();
    let folder = blocker.join("sub").to_string_lossy().into_owned();
    let row = h
        .add_with(&server.url("f.bin"), |r| r.folder = Some(folder))
        .await;
    let failed = h.wait_status(row.id, DownloadStatus::Failed).await;
    assert_eq!(failed.error.unwrap().fix, Some(FixAction::ChooseFolder));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn downloads_move_between_queues_and_keep_their_order() {
    let h = Harness::new().await;
    let paused = |r: &mut wirecatch_protocol::app::AddRequest| r.start = false;
    let a = h.add_with("https://example.com/a.bin", paused).await.id;
    let b = h.add_with("https://example.com/b.bin", paused).await.id;
    let c = h.add_with("https://example.com/c.bin", paused).await.id;
    let order = || {
        h.manager
            .list_downloads()
            .iter()
            .map(|r| (r.id, r.queue))
            .collect::<Vec<_>>()
    };

    h.manager.reorder(DEFAULT_QUEUE_ID, &[c, a, b]).unwrap();
    let main = DEFAULT_QUEUE_ID;
    assert_eq!(order(), vec![(c, main), (a, main), (b, main)]);
    assert!(h.manager.reorder(DEFAULT_QUEUE_ID, &[a, a]).is_err());

    let later = h.manager.create_queue("Later").unwrap().id;
    h.manager.move_to_queue(&[a], later).unwrap();
    assert_eq!(order(), vec![(c, main), (b, main), (a, later)]);
    assert!(h.manager.reorder(DEFAULT_QUEUE_ID, &[a, b]).is_err());

    assert!(matches!(
        h.manager.create_queue("later"),
        Err(CoreError::Invalid(_))
    ));
    assert!(matches!(
        h.manager.delete_queue(DEFAULT_QUEUE_ID),
        Err(CoreError::DefaultQueue)
    ));
    h.manager.delete_queue(later).unwrap();
    assert_eq!(order(), vec![(c, main), (b, main), (a, main)]);
    assert_eq!(h.manager.list_queues().len(), 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn pause_all_and_resume_all() {
    let server = TestServer::throttled(256 * KIB as usize, 64 * KIB).await;
    let h = Harness::new().await;
    let a = h.add(&server.url("a.bin")).await.id;
    let b = h.add(&server.url("b.bin")).await.id;
    h.wait_for(a, "running", |r| r.done > 0).await;

    h.manager.pause_all().unwrap();
    h.wait_status(a, DownloadStatus::Paused).await;
    h.wait_status(b, DownloadStatus::Paused).await;

    server.configure(|c| c.per_connection_bps = None);
    h.manager.resume_all().unwrap();
    h.wait_status(a, DownloadStatus::Completed).await;
    h.wait_status(b, DownloadStatus::Completed).await;
}
