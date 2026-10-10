//! Acceptance 3: a scheduled queue starts and stops inside its window. Time is
//! moved by hand, so a window of an hour is crossed in milliseconds.

use chrono::TimeDelta;
use wirecatch_core::DEFAULT_QUEUE_ID;
use wirecatch_protocol::app::{DownloadStatus, Schedule, TimeOfDay, Weekday};

use crate::common::{Harness, TestServer, is_running, monday};

const KIB: u64 = 1024;

fn schedule_default_queue(h: &Harness, schedule: Schedule) {
    let mut queue = h
        .manager
        .list_queues()
        .into_iter()
        .find(|q| q.id == DEFAULT_QUEUE_ID)
        .unwrap();
    queue.schedule = Some(schedule);
    h.manager.update_queue(queue).unwrap();
}

fn tod(hour: u8, minute: u8) -> TimeOfDay {
    TimeOfDay { hour, minute }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_window_starts_and_stops_its_queue() {
    let server = TestServer::throttled(512 * KIB as usize, 64 * KIB).await;
    let h = Harness::at(monday(1, 0)).await;
    schedule_default_queue(
        &h,
        Schedule {
            start: tod(2, 0),
            stop: Some(tod(3, 0)),
            days: vec![Weekday::Mon],
        },
    );

    let row = h.add(&server.url("night.bin")).await;
    assert_eq!(
        row.status,
        DownloadStatus::Queued,
        "started before its window"
    );

    h.clock.set(monday(1, 59));
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    assert_eq!(h.row(row.id).status, DownloadStatus::Queued);

    h.clock.set(monday(2, 0));
    h.wait_for(row.id, "start at 02:00", |r| is_running(r) && r.done > 0)
        .await;

    h.clock.set(monday(3, 0));
    let stopped = h.wait_status(row.id, DownloadStatus::Queued).await;
    assert!(stopped.done > 0, "progress was lost when the window closed");
    let sent = server.bytes_sent();

    // Tuesday is not a scheduled day, so it stays queued.
    h.clock.set(monday(2, 30) + TimeDelta::days(1));
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    assert_eq!(h.row(row.id).status, DownloadStatus::Queued);
    assert_eq!(server.bytes_sent(), sent);

    // The next Monday's window picks it up again.
    h.clock.set(monday(2, 30) + TimeDelta::days(7));
    h.wait_for(row.id, "restart next Monday", is_running).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_start_time_without_stop_runs_until_the_queue_is_empty() {
    let server = TestServer::fast(16 * KIB as usize).await;
    let h = Harness::at(monday(1, 0)).await;
    schedule_default_queue(
        &h,
        Schedule {
            start: tod(2, 0),
            stop: None,
            days: vec![],
        },
    );
    let first = h.add(&server.url("one.bin")).await;
    let second = h.add(&server.url("two.bin")).await;
    assert_eq!(first.status, DownloadStatus::Queued);

    h.clock.set(monday(2, 0));
    h.wait_status(first.id, DownloadStatus::Completed).await;
    h.wait_status(second.id, DownloadStatus::Completed).await;

    // The run ended with the queue empty; new work waits for the next start.
    h.clock.set(monday(9, 0));
    let late = h.add(&server.url("three.bin")).await;
    assert_eq!(late.status, DownloadStatus::Queued);
    h.clock.set(monday(2, 0) + TimeDelta::days(1));
    h.wait_status(late.id, DownloadStatus::Completed).await;
}
