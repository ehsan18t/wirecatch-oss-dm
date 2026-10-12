//! Paused queues, starting a scheduled queue now, and why a download is waiting.

use std::time::Duration;

use chrono::TimeDelta;
use wirecatch_core::DEFAULT_QUEUE_ID;
use wirecatch_protocol::app::{
    AddRequest, DownloadStatus, Queue, Schedule, TimeOfDay, UiEvent, WaitReason,
};
use wirecatch_protocol::{DownloadId, QueueId};

use crate::common::{Harness, SilentServer, TestResult, fast_server, is_running, monday, tune};
use wirecatch_engine::units::KIB;

/// Downloads from a silent server never stop on their own; give up on them quickly.
const SHORT: Duration = Duration::from_millis(300);

fn queue(h: &Harness, id: QueueId) -> TestResult<Queue> {
    Ok(h.manager
        .list_queues()
        .into_iter()
        .find(|q| q.id == id)
        .ok_or("no such queue")?)
}

fn paused(r: &mut AddRequest) {
    r.start = false;
}

fn waiting(h: &Harness, id: DownloadId) -> Option<WaitReason> {
    h.row(id).waiting
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_paused_queue_starts_nothing_and_resumes_only_what_it_paused() -> TestResult {
    let silent = SilentServer::start().await;
    let mut h = Harness::with_stop_timeout(SHORT).await?;
    let running = h.add(&silent.url("running.bin")).await?.id;
    let by_hand = h.add_with(&silent.url("by-hand.bin"), paused).await?.id;
    assert!(is_running(&h.row(running)));

    h.manager.set_queue_paused(DEFAULT_QUEUE_ID, true).unwrap();
    let stopped = h.wait_status(running, DownloadStatus::Queued).await;
    assert_eq!(stopped.waiting, Some(WaitReason::QueuePaused));
    let later = h.add(&silent.url("later.bin")).await?;
    assert_eq!(later.status, DownloadStatus::Queued);
    assert_eq!(later.waiting, Some(WaitReason::QueuePaused));

    // The pause survives a restart.
    h.restart().await?;
    assert!(queue(&h, DEFAULT_QUEUE_ID)?.paused);
    assert_eq!(h.row(running).status, DownloadStatus::Queued);
    assert_eq!(waiting(&h, later.id), Some(WaitReason::QueuePaused));

    h.manager.set_queue_paused(DEFAULT_QUEUE_ID, false).unwrap();
    assert!(is_running(&h.row(running)));
    assert!(is_running(&h.row(later.id)));
    assert_eq!(h.row(by_hand).status, DownloadStatus::Paused);
    assert_eq!(waiting(&h, running), None);
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn start_now_runs_a_scheduled_queue_until_it_is_empty() -> TestResult {
    let server = fast_server(8 * KIB).await?;
    let h = Harness::at(monday(12, 0)?).await?;
    let two = TimeOfDay { hour: 2, minute: 0 };
    let mut main = queue(&h, DEFAULT_QUEUE_ID)?;
    main.schedule = Some(Schedule {
        start: two,
        stop: Some(TimeOfDay { hour: 3, minute: 0 }),
        days: vec![],
    });
    h.manager.update_queue(main).unwrap();
    let outside = Some(WaitReason::QueueSchedule {
        starts_at: Some(two),
    });

    let first = h.add(&server.url("first.bin")).await?;
    assert_eq!(first.waiting, outside);
    h.manager.start_queue_now(DEFAULT_QUEUE_ID).unwrap();
    h.wait_status(first.id, DownloadStatus::Completed).await;
    // Empty now, so the schedule applies again on its own.
    crate::common::wait_until("the start-now run to end", || {
        queue(&h, DEFAULT_QUEUE_ID)
            .map(|q| (!q.running_outside_schedule).then_some(()))
            .transpose()
    })
    .await?;
    let second = h.add(&server.url("second.bin")).await?;
    assert_eq!(second.status, DownloadStatus::Queued);
    assert_eq!(second.waiting, outside);

    // Moving the clock on does not end a run that still has work.
    h.manager.start_queue_now(DEFAULT_QUEUE_ID).unwrap();
    h.clock.advance(TimeDelta::hours(3));
    h.wait_status(second.id, DownloadStatus::Completed).await;
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn start_now_and_its_end_by_pausing_survive_a_restart() -> TestResult {
    let silent = SilentServer::start().await;
    let mut h = Harness::with_stop_timeout(SHORT).await?;
    let mut main = queue(&h, DEFAULT_QUEUE_ID)?;
    main.schedule = Some(Schedule {
        start: TimeOfDay { hour: 2, minute: 0 },
        stop: Some(TimeOfDay { hour: 3, minute: 0 }),
        days: vec![],
    });
    h.manager.update_queue(main).unwrap();
    let row = h.add(&silent.url("slow.bin")).await?;
    h.manager.start_queue_now(DEFAULT_QUEUE_ID).unwrap();
    assert!(is_running(&h.row(row.id)));

    h.restart().await?;
    assert!(queue(&h, DEFAULT_QUEUE_ID)?.running_outside_schedule);
    assert!(is_running(&h.row(row.id)));

    // Pausing ends the run; resuming then follows the schedule again.
    h.manager.set_queue_paused(DEFAULT_QUEUE_ID, true).unwrap();
    assert!(!queue(&h, DEFAULT_QUEUE_ID)?.running_outside_schedule);
    h.manager.set_queue_paused(DEFAULT_QUEUE_ID, false).unwrap();
    let back = h.wait_status(row.id, DownloadStatus::Queued).await;
    assert!(matches!(
        back.waiting,
        Some(WaitReason::QueueSchedule { .. })
    ));
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn waiting_downloads_say_why() -> TestResult {
    let silent = SilentServer::start().await;
    let h = Harness::new().await?;
    let mut events = h.manager.subscribe();
    let other = h.manager.create_queue("Other").unwrap().id;
    let mut main = queue(&h, DEFAULT_QUEUE_ID)?;
    main.max_concurrent = 1;
    h.manager.update_queue(main).unwrap();

    // The queue is full.
    let a1 = h.add(&silent.url("a1.bin")).await?.id;
    let a2 = h.add(&silent.url("a2.bin")).await?.id;
    assert!(is_running(&h.row(a1)));
    assert_eq!(
        waiting(&h, a2),
        Some(WaitReason::QueueFull {
            running: 1,
            limit: 1
        })
    );

    // The app is full: the first in line says so, the rest are in line.
    tune(&h.manager, |s| s.max_concurrent_downloads = 1)?;
    let into_other = |r: &mut AddRequest| r.queue = Some(other);
    let b1 = h.add_with(&silent.url("b1.bin"), into_other).await?.id;
    let b2 = h.add_with(&silent.url("b2.bin"), into_other).await?.id;
    assert_eq!(
        waiting(&h, b1),
        Some(WaitReason::GlobalLimit {
            running: 1,
            limit: 1
        })
    );
    assert_eq!(waiting(&h, b2), Some(WaitReason::InLine { position: 2 }));
    assert_eq!(waiting(&h, a1), None, "a running download has no reason");

    // A reason that changes reaches the UI as a changed row.
    h.manager.set_queue_paused(other, true).unwrap();
    let event = tokio::time::timeout(crate::common::PATIENCE, async {
        loop {
            if let Ok(UiEvent::DownloadsChanged(rows)) = events.recv().await
                && let Some(row) = rows.iter().find(|r| r.id == b1)
                && row.waiting == Some(WaitReason::QueuePaused)
            {
                return row.clone();
            }
        }
    })
    .await
    .expect("no event for the new reason");
    assert_eq!(event.status, DownloadStatus::Queued);
    assert_eq!(waiting(&h, b2), Some(WaitReason::QueuePaused));
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn pause_all_clears_the_reasons_of_downloads_it_paused() -> TestResult {
    let silent = SilentServer::start().await;
    let h = Harness::with_stop_timeout(SHORT).await?;
    let mut main = queue(&h, DEFAULT_QUEUE_ID)?;
    main.max_concurrent = 1;
    h.manager.update_queue(main).unwrap();
    let running = h.add(&silent.url("running.bin")).await?.id;
    let behind = h.add(&silent.url("behind.bin")).await?.id;
    assert!(is_running(&h.row(running)));
    assert!(matches!(
        waiting(&h, behind),
        Some(WaitReason::QueueFull { .. })
    ));

    h.manager.pause_all().unwrap();
    // A paused download waits for nothing, at once, not only after the next change.
    let row = h.row(behind);
    assert_eq!(row.status, DownloadStatus::Paused);
    assert_eq!(row.waiting, None);
    Ok(())
}

/// A capture waiting for the user's answer was meant for a queue that is then
/// deleted: like the queue's downloads, it moves to the default queue, so saying
/// yes still works.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn deleting_a_queue_moves_its_waiting_confirmations_to_the_default() -> TestResult {
    let server = fast_server(1024).await?;
    let h = Harness::new().await?;
    tune(&h.manager, |s| s.extension.ask_before_download = true)?;
    let later = h.manager.create_queue("Later").unwrap().id;
    let capture = wirecatch_protocol::extension::NewDownload {
        handover_id: "tab-1".into(),
        url: server.url("report.pdf").to_string(),
        referrer: None,
        page_url: None,
        suggested_name: Some("report.pdf".into()),
        mime: None,
        size_hint: Some(1024),
        cookies: None,
        user_agent: None,
        extra_headers: vec![],
        source: wirecatch_protocol::extension::CaptureSource::BrowserDownload,
        queue: Some(later),
        media: None,
    };
    h.manager.add_capture(capture).await.unwrap();
    assert_eq!(h.manager.pending_confirmations()[0].queue, later);

    h.manager.delete_queue(later).unwrap();
    let waiting = h.manager.pending_confirmations().remove(0);
    assert_eq!(waiting.queue, DEFAULT_QUEUE_ID);
    let answer = wirecatch_protocol::app::CaptureConfirmation {
        folder: None,
        file_name: None,
        queue: None,
        later: false,
    };
    h.manager.confirm_capture(waiting.id, answer).await.unwrap();
    let row = h.manager.list_downloads().remove(0);
    assert_eq!(row.queue, DEFAULT_QUEUE_ID);
    h.wait_status(row.id, DownloadStatus::Completed).await;
    Ok(())
}
