//! Acceptance 8 (expired link and refresh) and the rest of a download's life:
//! naming, cancel, retry, remove, queues and ordering.

use wirecatch_core::{CoreError, DEFAULT_QUEUE_ID};
use wirecatch_protocol::DownloadId;
use wirecatch_protocol::app::{DownloadStatus, FixAction, UiEvent};

use crate::common::{Harness, TestServer, exists, partial_files, wait_until};

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
    let cancelled = h.wait_status(row.id, DownloadStatus::Failed).await;
    assert_eq!(cancelled.done, 0);
    let error = cancelled.error.unwrap();
    assert_eq!(error.fix, Some(FixAction::Retry));
    assert!(error.message.contains("cancelled"));
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
