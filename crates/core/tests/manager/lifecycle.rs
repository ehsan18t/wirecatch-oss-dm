//! Acceptance 8 (expired link and refresh) and the rest of a download's life:
//! naming, cancel, retry, remove, queues and ordering.

use std::time::Duration;

use wirecatch_core::{CoreError, DEFAULT_QUEUE_ID, LinkFrom, RemoveFiles};
use wirecatch_protocol::DownloadId;
use wirecatch_protocol::app::{DownloadStatus, FixAction, QueueSettings, UiEvent};
use wirecatch_protocol::wire::ErrorCode;

use crate::common::{
    Harness, PATIENCE, Seen, SilentServer, TestResult, exists, fast_server, partial_files,
    throttled_server, wait_until,
};
use wirecatch_engine::units::KIB;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_expired_link_is_refreshed_without_losing_bytes() -> TestResult {
    let len = 512 * KIB;
    let server = throttled_server(len, 128 * KIB).await?;
    let h = Harness::new().await?;
    let row = h.add(&server.url("old/file.bin")).await?;
    h.wait_for(row.id, "a good part downloaded", |r| r.done >= 128 * KIB)
        .await;
    h.manager.pause(row.id).unwrap();
    let paused = h.wait_status(row.id, DownloadStatus::Paused).await;
    assert!(paused.done >= 128 * KIB);

    // The link expires while the download is paused.
    server.configure(|c| c.refuse_paths.push(("/old/file.bin".into(), 410)));
    h.manager.resume(row.id).unwrap();
    let failed = h.wait_status(row.id, DownloadStatus::Failed).await;
    let error = failed.error.expect("a failed download explains why");
    assert_eq!(error.fix, Some(FixAction::RefreshLink));
    assert!(error.message.contains("expired"), "{}", error.message);
    assert_eq!(failed.done, paused.done, "progress was lost");

    // The first answer checks the new link: the server holds its body, so it
    // sends no byte whatever the machine's load, and a download that read it
    // on to fetch the file again would wait on it and fail.
    server.configure(|c| c.hold_bodies_from = Some(0));
    let asked_before = server.stats.seen().len();
    h.manager
        .refresh_link(
            row.id,
            server.url("new/file.bin").as_str(),
            LinkFrom::Person,
        )
        .unwrap();
    let done = h.wait_status(row.id, DownloadStatus::Completed).await;
    assert_eq!(done.path, row.path, "the file moved");
    assert_eq!(done.url, server.url("new/file.bin").as_str());
    assert_eq!(std::fs::read(&done.path).unwrap(), server.data);
    // Only the missing part came again: no answer that sent anything started before it. A total of bytes sent would also count what buffers took in from a range a split cut short, which grows with the machine's load.
    let asked = server.stats.seen().split_off(asked_before);
    let answers: Vec<_> = asked.iter().map(|s| (s.range(), s.sent())).collect();
    assert!(
        asked
            .iter()
            .filter(|s| s.sent() > 0)
            .all(|s| s.range().is_some_and(|(start, _)| start >= paused.done)),
        "asked again for bytes already here: {answers:?}, {} were here",
        paused.done
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cancel_discards_progress_and_retry_starts_over() -> TestResult {
    let server = throttled_server(512 * KIB, 128 * KIB).await?;
    let h = Harness::new().await?;
    let row = h.add(&server.url("file.bin")).await?;
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
    let asked_before = server.stats.seen().len();
    h.manager.retry(row.id).unwrap();
    let done = h.wait_status(row.id, DownloadStatus::Completed).await;
    assert_eq!(std::fs::read(&done.path).unwrap(), server.data);
    // Retry started over: its own answers brought the whole file again, from byte 0. Only the retry's, since the cancelled run's last chunk can still leave the server after it stopped.
    let retried = server.stats.seen().split_off(asked_before);
    let answers: Vec<_> = retried.iter().map(|s| (s.range(), s.sent())).collect();
    assert_eq!(
        retried
            .first()
            .and_then(Seen::range)
            .map(|(start, _)| start),
        Some(0),
        "{answers:?}"
    );
    assert!(
        retried.iter().map(Seen::sent).sum::<u64>() >= server.data.len() as u64,
        "the retry did not fetch the whole file: {answers:?}"
    );
    let finished = h.manager.cancel(row.id).unwrap_err();
    assert!(matches!(finished, CoreError::WrongState(_)), "{finished}");
    // A client tells a finished download from a request it got wrong by the code.
    assert_eq!(finished.to_protocol().code, ErrorCode::WrongState);
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_ui_hears_when_saving_keeps_failing() -> TestResult {
    let h = Harness::new().await?;
    let mut events = h.manager.subscribe();
    let db = h
        .root
        .path()
        .join("data")
        .join(wirecatch_core::DATABASE_FILE);
    let other = rusqlite::Connection::open(db).unwrap();
    other.execute("DROP TABLE downloads", []).unwrap();

    h.add_with("https://example.com/a.bin", |r| r.start = false)
        .await?;
    let error = tokio::time::timeout(PATIENCE, async {
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
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn resume_and_retry_pressed_while_stopping_are_not_lost() -> TestResult {
    let server = throttled_server(256 * KIB, 64 * KIB).await?;
    let h = Harness::new().await?;
    let resumed = h.add(&server.url("resumed.bin")).await?.id;
    let all = h.add(&server.url("all.bin")).await?.id;
    let retried = h.add(&server.url("retried.bin")).await?.id;
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
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_server_that_never_answers_cannot_hang_the_app() -> TestResult {
    let silent = SilentServer::start().await;
    let h = Harness::with_stop_timeout(Duration::from_millis(300)).await?;
    let paused = h.add(&silent.url("pause.bin")).await?;
    let removed = h.add(&silent.url("remove.bin")).await?;
    let left = h.add(&silent.url("left.bin")).await?;
    for row in [&paused, &removed, &left] {
        assert_eq!(row.status, DownloadStatus::Connecting);
    }
    let patience = Duration::from_secs(5);

    h.manager.pause(paused.id).unwrap();
    h.wait_status(paused.id, DownloadStatus::Paused).await;
    tokio::time::timeout(patience, h.manager.remove(&[removed.id], RemoveFiles::Keep))
        .await
        .expect("remove waited forever")
        .unwrap();
    tokio::time::timeout(patience, h.manager.shutdown())
        .await
        .expect("shutdown waited forever");
    // Interrupted by shutdown, so it continues on the next start.
    assert_eq!(h.row(left.id).status, DownloadStatus::Connecting);
    Ok(())
}

/// The desktop app relies on exactly these return
/// types: the new id from `add_download`, the new queue from `create_queue`, and
/// nothing from the rest. Errors reach it as their plain-language text.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn commands_return_what_the_ui_expects() -> TestResult {
    let h = Harness::new().await?;
    let m = &h.manager;
    let request = wirecatch_protocol::app::AddRequest {
        url: "https://example.com/a.bin".into(),
        queue: None,
        folder: None,
        file_name: None,
        start: false,
    };
    let id: DownloadId = m.add_download(request).await.unwrap();
    let queue: wirecatch_protocol::app::Queue =
        m.create_queue(QueueSettings::new("Later")).unwrap();
    let nothing: [(); 11] = [
        m.update_queue(queue.id, queue.settings.clone()).unwrap(),
        m.update_settings(m.get_settings()).unwrap(),
        m.pause(id).unwrap(),
        m.resume(id).unwrap(),
        m.pause_all().unwrap(),
        m.resume_all().unwrap(),
        m.move_to_queue(&[id], queue.id).unwrap(),
        m.reorder(queue.id, &[id]).unwrap(),
        m.refresh_link(id, "https://example.com/b.bin", LinkFrom::Person)
            .unwrap(),
        m.cancel(id).unwrap(),
        m.retry(id).unwrap(),
    ];
    assert_eq!(nothing.len(), 11);
    let (): () = m.remove(&[id], RemoveFiles::Keep).await.unwrap();
    let (): () = m.delete_queue(queue.id).unwrap();

    let error = m.resume(id).unwrap_err().to_string();
    assert_eq!(
        error,
        "That download no longer exists. It may have been removed."
    );
    Ok(())
}

/// The desktop app calls synchronous methods from its window thread, outside any Tokio runtime.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn sync_methods_work_from_a_thread_outside_the_runtime() -> TestResult {
    let server = fast_server(4 * KIB).await?;
    let h = Harness::new().await?;
    let paused = |r: &mut wirecatch_protocol::app::AddRequest| r.start = false;
    let started = h.add_with(&server.url("a.bin"), paused).await?;
    let cancelled = h.add_with(&server.url("b.bin"), paused).await?;
    let manager = h.manager.clone();
    std::thread::spawn(move || {
        manager.pause_all().unwrap();
        manager.resume_all().unwrap();
        manager.resume(started.id).unwrap();
        manager.cancel(cancelled.id).unwrap();
        let queue = manager
            .create_queue(QueueSettings::new("From a thread"))
            .unwrap();
        manager.update_queue(queue.id, queue.settings).unwrap();
        let settings = manager.get_settings();
        manager.update_settings(settings).unwrap();
    })
    .join()
    .expect("a manager method needed a Tokio runtime on the calling thread");
    h.wait_status(started.id, DownloadStatus::Completed).await;
    h.wait_status(cancelled.id, DownloadStatus::Cancelled).await;
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn removing_an_unfinished_download_never_deletes_someone_elses_file() -> TestResult {
    let h = Harness::new().await?;
    let row = h
        .add_with("https://example.com/report.pdf", |r| r.start = false)
        .await?;
    // The user saved a file of the same name there meanwhile.
    let path = std::path::Path::new(&row.path);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, b"not ours").unwrap();
    let [part, _] = partial_files(&row.path);
    std::fs::write(&part, b"ours").unwrap();

    h.manager
        .remove(&[row.id], RemoveFiles::Delete)
        .await
        .unwrap();
    assert!(exists(path), "an unrelated file was deleted");
    assert!(!exists(&part));
    Ok(())
}

/// A download window cancels by removing without the file, so a download that finished before
/// the window heard keeps its file.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn removing_a_finished_download_without_its_file_keeps_the_file() -> TestResult {
    let server = fast_server(8 * KIB).await?;
    let h = Harness::new().await?;
    let row = h.add(&server.url("keep.bin")).await?;
    let row = h.wait_status(row.id, DownloadStatus::Completed).await;

    h.manager
        .remove(&[row.id], RemoveFiles::Keep)
        .await
        .unwrap();
    assert!(exists(&row.path), "the finished file was deleted");
    assert_eq!(
        h.manager.list_downloads(),
        [] as [wirecatch_protocol::app::DownloadSummary; 0]
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_waiting_download_can_be_cancelled_and_removed() -> TestResult {
    let h = Harness::new().await?;
    let row = h
        .add_with("https://example.com/later.bin", |r| r.start = false)
        .await?;
    h.manager.cancel(row.id).unwrap();
    assert_eq!(h.row(row.id).status, DownloadStatus::Cancelled);
    h.manager
        .remove(&[row.id], RemoveFiles::Keep)
        .await
        .unwrap();
    assert_eq!(
        h.manager.list_downloads(),
        [] as [wirecatch_protocol::app::DownloadSummary; 0]
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn remove_stops_and_cleans_up() -> TestResult {
    let slow = throttled_server(512 * KIB, 64 * KIB).await?;
    let fast = fast_server(8 * KIB).await?;
    let h = Harness::new().await?;
    let mut events = h.manager.subscribe();

    let finished = h.add(&fast.url("keep.bin")).await?;
    let finished = h.wait_status(finished.id, DownloadStatus::Completed).await;
    let running = h.add(&slow.url("drop.bin")).await?;
    h.wait_for(running.id, "progress", |r| r.done > 0).await;

    h.manager
        .remove(&[running.id, DownloadId(999)], RemoveFiles::Keep)
        .await
        .unwrap();
    assert!(partial_files(&running.path).iter().all(|p| !exists(p)));
    h.manager
        .remove(&[finished.id], RemoveFiles::Delete)
        .await
        .unwrap();
    assert!(!exists(&finished.path));
    assert_eq!(
        h.manager.list_downloads(),
        [] as [wirecatch_protocol::app::DownloadSummary; 0]
    );

    let mut removed = Vec::new();
    while let Ok(event) = events.try_recv() {
        if let UiEvent::DownloadsRemoved(ids) = event {
            removed.extend(ids);
        }
    }
    assert_eq!(removed, vec![running.id, finished.id]);
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn names_are_unique_and_folders_follow_categories() -> TestResult {
    let h = Harness::new().await?;
    let paused = |r: &mut wirecatch_protocol::app::AddRequest| r.start = false;
    let first = h
        .add_with("https://example.com/a/clip.MP4?x=1", paused)
        .await?;
    let second = h.add_with("https://example.com/b/clip.MP4", paused).await?;
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
        .await?;
    assert_eq!(
        std::path::Path::new(&renamed.path),
        h.folders.downloads.join("Compressed").join("disk.iso")
    );

    let err = h
        .manager
        .add_download(wirecatch_protocol::app::AddRequest {
            url: "sftp://example.com/x".into(),
            queue: None,
            folder: None,
            file_name: None,
            start: true,
        })
        .await
        .unwrap_err();
    assert!(matches!(err, CoreError::InvalidUrl(_)));
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_folder_that_cannot_be_created_asks_for_another() -> TestResult {
    let server = fast_server(1024).await?;
    let h = Harness::new().await?;
    let blocker = h.root.path().join("not-a-folder");
    std::fs::write(&blocker, b"x").unwrap();
    let folder = blocker.join("sub").to_string_lossy().into_owned();
    let row = h
        .add_with(&server.url("f.bin"), |r| r.folder = Some(folder))
        .await?;
    let failed = h.wait_status(row.id, DownloadStatus::Failed).await;
    assert_eq!(failed.error.unwrap().fix, Some(FixAction::ChooseFolder));
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn downloads_move_between_queues_and_keep_their_order() -> TestResult {
    let h = Harness::new().await?;
    let paused = |r: &mut wirecatch_protocol::app::AddRequest| r.start = false;
    let a = h.add_with("https://example.com/a.bin", paused).await?.id;
    let b = h.add_with("https://example.com/b.bin", paused).await?.id;
    let c = h.add_with("https://example.com/c.bin", paused).await?.id;
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

    let later = h
        .manager
        .create_queue(QueueSettings::new("Later"))
        .unwrap()
        .id;
    h.manager.move_to_queue(&[a], later).unwrap();
    assert_eq!(order(), vec![(c, main), (b, main), (a, later)]);
    assert!(h.manager.reorder(DEFAULT_QUEUE_ID, &[a, b]).is_err());

    assert!(matches!(
        h.manager.create_queue(QueueSettings::new("later")),
        Err(CoreError::Invalid(_))
    ));
    assert!(matches!(
        h.manager.delete_queue(DEFAULT_QUEUE_ID),
        Err(CoreError::DefaultQueue)
    ));
    h.manager.delete_queue(later).unwrap();
    assert_eq!(order(), vec![(c, main), (b, main), (a, main)]);
    assert_eq!(h.manager.list_queues().len(), 1);
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_running_download_moves_folder_and_continues() -> TestResult {
    let len = 512 * KIB;
    let server = throttled_server(len, 128 * KIB).await?;
    let h = Harness::new().await?;
    let row = h.add(&server.url("big.bin")).await?;
    h.wait_for(row.id, "a good part downloaded", |r| r.done >= 64 * KIB)
        .await;

    let elsewhere = h.root.path().join("elsewhere");
    h.manager
        .set_download_folder(row.id, &elsewhere.to_string_lossy())
        .await
        .unwrap();
    let kept = h.row(row.id).done;
    assert!(kept >= 64 * KIB);
    // Running again in the new folder, counting from what was kept, not from zero.
    let resumed = h
        .wait_for(row.id, "running again", |r| {
            r.status == DownloadStatus::Downloading
        })
        .await;
    assert!(
        resumed.done >= kept,
        "{} < {kept}: it started over",
        resumed.done
    );
    assert_eq!(
        std::path::Path::new(&resumed.path),
        elsewhere.join("big.bin")
    );

    server.configure(|c| c.per_connection_bps = None);
    let done = h.wait_status(row.id, DownloadStatus::Completed).await;
    assert_eq!(std::fs::read(&done.path).unwrap(), server.data);
    assert_eq!(done.total, Some(len));
    assert!(!exists(&row.path) && partial_files(&row.path).iter().all(|p| !exists(p)));
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_finished_file_moves_and_keeps_clear_of_existing_names() -> TestResult {
    let server = fast_server(4 * KIB).await?;
    let h = Harness::new().await?;
    let row = h.add(&server.url("report.pdf")).await?;
    h.wait_status(row.id, DownloadStatus::Completed).await;
    let elsewhere = h.root.path().join("elsewhere");
    std::fs::create_dir_all(&elsewhere).unwrap();
    std::fs::write(elsewhere.join("report.pdf"), b"someone else's").unwrap();

    h.manager
        .set_download_folder(row.id, &elsewhere.to_string_lossy())
        .await
        .unwrap();
    let moved = h.row(row.id);
    assert_eq!(moved.file_name, "report (1).pdf");
    assert_eq!(std::fs::read(&moved.path).unwrap(), server.data);
    assert!(!exists(&row.path));
    assert_eq!(
        std::fs::read(elsewhere.join("report.pdf")).unwrap(),
        b"someone else's"
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn choosing_a_new_folder_fixes_a_folder_error() -> TestResult {
    let server = fast_server(1024).await?;
    let h = Harness::new().await?;
    let blocker = h.root.path().join("not-a-folder");
    std::fs::write(&blocker, b"x").unwrap();
    let folder = blocker.join("sub").to_string_lossy().into_owned();
    let row = h
        .add_with(&server.url("f.bin"), |r| r.folder = Some(folder))
        .await?;
    let failed = h.wait_status(row.id, DownloadStatus::Failed).await;
    assert_eq!(failed.error.unwrap().fix, Some(FixAction::ChooseFolder));

    // Folders that cannot be used are refused in plain words.
    let err = h
        .manager
        .set_download_folder(row.id, "relative/folder")
        .await
        .unwrap_err();
    assert!(err.to_string().contains("not a full folder path"), "{err}");
    let err = h
        .manager
        .set_download_folder(row.id, &blocker.to_string_lossy())
        .await
        .unwrap_err();
    assert!(err.to_string().contains("Could not use"), "{err}");

    h.manager
        .set_download_folder(row.id, "{Downloads}/Fixed")
        .await
        .unwrap();
    let done = h.wait_status(row.id, DownloadStatus::Completed).await;
    assert_eq!(
        std::path::Path::new(&done.path),
        h.folders.downloads.join("Fixed").join("f.bin")
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_list_is_grouped_by_queue_in_line_order() -> TestResult {
    let h = Harness::new().await?;
    let later = h
        .manager
        .create_queue(QueueSettings::new("Later"))
        .unwrap()
        .id;
    let main = DEFAULT_QUEUE_ID;
    let mut add = Vec::new();
    // Added alternately, so neither id nor time order matches the expected order.
    for (name, queue) in [("a1", main), ("b1", later), ("a2", main), ("b2", later)] {
        let row = h
            .add_with(&format!("https://example.com/{name}.bin"), |r| {
                r.start = false;
                r.queue = Some(queue);
            })
            .await?;
        add.push(row.id);
    }
    let [a1, b1, a2, b2] = add[..] else {
        unreachable!()
    };
    let ids = || -> Vec<DownloadId> { h.manager.list_downloads().iter().map(|r| r.id).collect() };
    assert_eq!(ids(), vec![a1, a2, b1, b2]);
    h.manager.reorder(later, &[b2, b1]).unwrap();
    h.manager.reorder(main, &[a2, a1]).unwrap();
    assert_eq!(ids(), vec![a2, a1, b2, b1]);
    let queues: Vec<_> = h.manager.list_queues().iter().map(|q| q.id).collect();
    assert_eq!(queues, vec![main, later]);
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn pause_all_and_resume_all() -> TestResult {
    let server = throttled_server(256 * KIB, 64 * KIB).await?;
    let h = Harness::new().await?;
    let a = h.add(&server.url("a.bin")).await?.id;
    let b = h.add(&server.url("b.bin")).await?.id;
    h.wait_for(a, "running", |r| r.done > 0).await;

    h.manager.pause_all().unwrap();
    h.wait_status(a, DownloadStatus::Paused).await;
    h.wait_status(b, DownloadStatus::Paused).await;

    server.configure(|c| c.per_connection_bps = None);
    h.manager.resume_all().unwrap();
    h.wait_status(a, DownloadStatus::Completed).await;
    h.wait_status(b, DownloadStatus::Completed).await;
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn one_download_can_be_read_by_its_id() -> TestResult {
    let server = fast_server(1024).await?;
    let h = Harness::new().await?;
    let row = h.add(&server.url("one.bin")).await?;
    let done = h.wait_status(row.id, DownloadStatus::Completed).await;
    assert_eq!(h.manager.get_download(row.id).unwrap(), done);
    assert!(matches!(
        h.manager.get_download(DownloadId(999)),
        Err(CoreError::DownloadNotFound(_))
    ));
    Ok(())
}

/// Files downloaded with Wirecatch carry the Mark of the Web, as a browser's do,
/// so SmartScreen and Office Protected View still look at them.
#[cfg(windows)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_finished_download_is_marked_as_from_the_internet() -> TestResult {
    let server = fast_server(1024).await?;
    let h = Harness::new().await?;
    let url = server
        .url("setup.exe")
        .as_str()
        .replace("http://", "http://user:pw@");
    let row = h.add(&url).await?;
    let done = h.wait_status(row.id, DownloadStatus::Completed).await;
    let stream = format!("{}:Zone.Identifier", done.path);
    let text = wait_until("the mark is written", || {
        std::fs::read_to_string(&stream).ok()
    })
    .await;
    let host = server.url("setup.exe");
    assert_eq!(
        text,
        format!("[ZoneTransfer]\r\nZoneId=3\r\nHostUrl={host}\r\n")
    );
    Ok(())
}

/// The part file is marked while the download runs, so the finished file has
/// the mark from the moment it is renamed into place.
#[cfg(windows)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_download_is_marked_before_it_is_finished() -> TestResult {
    let len = 256 * KIB;
    let server = throttled_server(len, 64 * KIB).await?;
    let h = Harness::new().await?;
    let row = h.add(&server.url("setup.exe?token=abc")).await?;
    let running = h
        .wait_for(row.id, "downloading", |r| r.done >= 16 * KIB)
        .await;
    let [part, _] = partial_files(&running.path);
    let stream = format!("{}:Zone.Identifier", part.display());
    let text = wait_until("the part file is marked", || {
        std::fs::read_to_string(&stream).ok()
    })
    .await;
    assert!(text.contains("HostUrl="), "{text}");
    assert!(!text.contains("token"), "{text}");
    Ok(())
}

/// A download added without a browser's user agent names the app and its version, never the
/// engine library's default.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_download_without_a_browser_agent_names_the_app() -> TestResult {
    let server = wirecatch_test_support::files::FileServer::start().await;
    server.put("plain.bin", b"plain".as_slice());
    let h = Harness::new().await?;
    let row = h.add(server.url("plain.bin")).await?;
    h.wait_status(row.id, DownloadStatus::Completed).await;
    let expected = format!(
        "{}/{}",
        wirecatch_product::PRODUCT_NAME,
        wirecatch_product::PRODUCT_VERSION
    );
    assert_ne!(
        expected,
        wirecatch_engine::DownloadOptions::default().user_agent,
        "the app's agent must not be the engine library's"
    );
    let seen = server.seen_at("plain.bin");
    assert!(!seen.is_empty());
    for request in seen {
        assert_eq!(
            request.headers.get("user-agent").unwrap(),
            expected.as_str()
        );
    }
    Ok(())
}
