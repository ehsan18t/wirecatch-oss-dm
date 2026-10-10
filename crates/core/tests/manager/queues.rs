//! Paused queues, starting a scheduled queue now, and why a download is waiting.

use std::time::Duration;

use chrono::TimeDelta;
use wirecatch_core::DEFAULT_QUEUE_ID;
use wirecatch_protocol::app::{
    AddRequest, DownloadStatus, Queue, Schedule, TimeOfDay, UiEvent, WaitReason,
};
use wirecatch_protocol::{DownloadId, QueueId};

use crate::common::{Harness, SilentServer, TestServer, is_running, monday, tune};

/// Downloads from a silent server never stop on their own; give up on them quickly.
const SHORT: Duration = Duration::from_millis(300);

fn queue(h: &Harness, id: QueueId) -> Queue {
    h.manager
        .list_queues()
        .into_iter()
        .find(|q| q.id == id)
        .unwrap()
}

fn paused(r: &mut AddRequest) {
    r.start = false;
}

fn waiting(h: &Harness, id: DownloadId) -> Option<WaitReason> {
    h.row(id).waiting
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_paused_queue_starts_nothing_and_resumes_only_what_it_paused() {
    let silent = SilentServer::start().await;
    let mut h = Harness::with_stop_timeout(SHORT).await;
    let running = h.add(&silent.url("running.bin")).await.id;
    let by_hand = h.add_with(&silent.url("by-hand.bin"), paused).await.id;
    assert!(is_running(&h.row(running)));

    h.manager.set_queue_paused(DEFAULT_QUEUE_ID, true).unwrap();
    let stopped = h.wait_status(running, DownloadStatus::Queued).await;
    assert_eq!(stopped.waiting, Some(WaitReason::QueuePaused));
    let later = h.add(&silent.url("later.bin")).await;
    assert_eq!(later.status, DownloadStatus::Queued);
    assert_eq!(later.waiting, Some(WaitReason::QueuePaused));

    // The pause survives a restart.
    h.restart().await;
    assert!(queue(&h, DEFAULT_QUEUE_ID).paused);
    assert_eq!(h.row(running).status, DownloadStatus::Queued);
    assert_eq!(waiting(&h, later.id), Some(WaitReason::QueuePaused));

    h.manager.set_queue_paused(DEFAULT_QUEUE_ID, false).unwrap();
    assert!(is_running(&h.row(running)));
    assert!(is_running(&h.row(later.id)));
    assert_eq!(h.row(by_hand).status, DownloadStatus::Paused);
    assert_eq!(waiting(&h, running), None);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn start_now_runs_a_scheduled_queue_until_it_is_empty() {
    let server = TestServer::fast(8 * 1024).await;
    let h = Harness::at(monday(12, 0)).await;
    let two = TimeOfDay { hour: 2, minute: 0 };
    let mut main = queue(&h, DEFAULT_QUEUE_ID);
    main.schedule = Some(Schedule {
        start: two,
        stop: Some(TimeOfDay { hour: 3, minute: 0 }),
        days: vec![],
    });
    h.manager.update_queue(main).unwrap();
    let outside = Some(WaitReason::QueueSchedule {
        starts_at: Some(two),
    });

    let first = h.add(&server.url("first.bin")).await;
    assert_eq!(first.waiting, outside);
    h.manager.start_queue_now(DEFAULT_QUEUE_ID).unwrap();
    h.wait_status(first.id, DownloadStatus::Completed).await;
    // Empty now, so the schedule applies again on its own.
    crate::common::wait_until("the start-now run to end", || {
        (!queue(&h, DEFAULT_QUEUE_ID).running_outside_schedule).then_some(())
    })
    .await;
    let second = h.add(&server.url("second.bin")).await;
    assert_eq!(second.status, DownloadStatus::Queued);
    assert_eq!(second.waiting, outside);

    // Moving the clock on does not end a run that still has work.
    h.manager.start_queue_now(DEFAULT_QUEUE_ID).unwrap();
    h.clock.advance(TimeDelta::hours(3));
    h.wait_status(second.id, DownloadStatus::Completed).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn start_now_and_its_end_by_pausing_survive_a_restart() {
    let silent = SilentServer::start().await;
    let mut h = Harness::with_stop_timeout(SHORT).await;
    let mut main = queue(&h, DEFAULT_QUEUE_ID);
    main.schedule = Some(Schedule {
        start: TimeOfDay { hour: 2, minute: 0 },
        stop: Some(TimeOfDay { hour: 3, minute: 0 }),
        days: vec![],
    });
    h.manager.update_queue(main).unwrap();
    let row = h.add(&silent.url("slow.bin")).await;
    h.manager.start_queue_now(DEFAULT_QUEUE_ID).unwrap();
    assert!(is_running(&h.row(row.id)));

    h.restart().await;
    assert!(queue(&h, DEFAULT_QUEUE_ID).running_outside_schedule);
    assert!(is_running(&h.row(row.id)));

    // Pausing ends the run; resuming then follows the schedule again.
    h.manager.set_queue_paused(DEFAULT_QUEUE_ID, true).unwrap();
    assert!(!queue(&h, DEFAULT_QUEUE_ID).running_outside_schedule);
    h.manager.set_queue_paused(DEFAULT_QUEUE_ID, false).unwrap();
    let back = h.wait_status(row.id, DownloadStatus::Queued).await;
    assert!(matches!(
        back.waiting,
        Some(WaitReason::QueueSchedule { .. })
    ));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn waiting_downloads_say_why() {
    let silent = SilentServer::start().await;
    let h = Harness::new().await;
    let mut events = h.manager.subscribe();
    let other = h.manager.create_queue("Other").unwrap().id;
    let mut main = queue(&h, DEFAULT_QUEUE_ID);
    main.max_concurrent = 1;
    h.manager.update_queue(main).unwrap();

    // The queue is full.
    let a1 = h.add(&silent.url("a1.bin")).await.id;
    let a2 = h.add(&silent.url("a2.bin")).await.id;
    assert!(is_running(&h.row(a1)));
    assert_eq!(
        waiting(&h, a2),
        Some(WaitReason::QueueFull {
            running: 1,
            limit: 1
        })
    );

    // The app is full: the first in line says so, the rest are in line.
    tune(&h.manager, |s| s.max_concurrent_downloads = 1);
    let into_other = |r: &mut AddRequest| r.queue = Some(other);
    let b1 = h.add_with(&silent.url("b1.bin"), into_other).await.id;
    let b2 = h.add_with(&silent.url("b2.bin"), into_other).await.id;
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
}
