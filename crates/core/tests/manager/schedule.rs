//! Acceptance 3: a scheduled queue starts and stops inside its window. Time is
//! moved by hand, so a window of an hour is crossed in milliseconds.

use chrono::TimeDelta;
use wirecatch_core::DEFAULT_QUEUE_ID;
use wirecatch_protocol::app::{DownloadStatus, Schedule, TimeOfDay, Weekday};

use crate::common::{
    Harness, QUIET, TestResult, fast_server, held_server, is_running, monday, wait_until,
};
use wirecatch_engine::units::KIB;

fn schedule_default_queue(h: &Harness, schedule: Schedule) -> TestResult {
    let mut queue = h
        .manager
        .list_queues()
        .into_iter()
        .find(|q| q.id == DEFAULT_QUEUE_ID)
        .ok_or("the default queue is missing")?;
    queue.settings.schedule = Some(schedule);
    h.manager.update_queue(queue.id, queue.settings)?;
    Ok(())
}

fn tod(hour: u8, minute: u8) -> TimeOfDay {
    TimeOfDay { hour, minute }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_window_starts_and_stops_its_queue() -> TestResult {
    let server = held_server(512 * KIB, 64 * KIB).await?;
    let h = Harness::at(monday(1, 0)?).await?;
    schedule_default_queue(
        &h,
        Schedule {
            start: tod(2, 0),
            stop: Some(tod(3, 0)),
            days: vec![Weekday::Mon],
        },
    )?;

    let row = h.add(&server.url("night.bin")).await?;
    assert_eq!(
        row.status,
        DownloadStatus::Queued,
        "started before its window"
    );

    h.clock.set(monday(1, 59)?);
    tokio::time::sleep(QUIET).await;
    assert_eq!(h.row(row.id).status, DownloadStatus::Queued);

    h.clock.set(monday(2, 0)?);
    h.wait_for(row.id, "start at 02:00", |r| is_running(r) && r.done > 0)
        .await;

    h.clock.set(monday(3, 0)?);
    let stopped = h.wait_status(row.id, DownloadStatus::Queued).await;
    assert!(stopped.done > 0, "progress was lost when the window closed");
    // Requests, not bytes: the stopped connection's last chunk can still leave the server just after the row turns queued.
    let asked = server.stats.seen().len();

    // Tuesday is not a scheduled day, so it stays queued.
    h.clock.set(monday(2, 30)? + TimeDelta::days(1));
    tokio::time::sleep(QUIET).await;
    assert_eq!(h.row(row.id).status, DownloadStatus::Queued);
    assert_eq!(
        server.stats.seen().len(),
        asked,
        "it asked the server for more outside its window"
    );

    // The next Monday's window picks it up again.
    h.clock.set(monday(2, 30)? + TimeDelta::days(7));
    h.wait_for(row.id, "restart next Monday", is_running).await;
    Ok(())
}

/// Nothing runs at the restart: the queue is paused for a moment. Its run goes on after it.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_run_without_a_stop_time_survives_a_restart_with_nothing_running() -> TestResult {
    let server = fast_server(16 * KIB).await?;
    let mut h = Harness::at(monday(1, 0)?).await?;
    schedule_default_queue(
        &h,
        Schedule {
            start: tod(2, 0),
            stop: None,
            days: vec![],
        },
    )?;
    h.manager.set_queue_paused(DEFAULT_QUEUE_ID, true)?;
    let row = h.add(&server.url("night.bin")).await?;
    let in_run = |h: &Harness| {
        h.manager
            .list_queues()
            .into_iter()
            .any(|q| q.id == DEFAULT_QUEUE_ID && q.status.running_until_empty)
    };

    h.clock.set(monday(2, 0)?);
    wait_until("the start at 02:00", || in_run(&h).then_some(())).await;
    h.restart().await?;
    assert!(in_run(&h), "the restart dropped the run");

    h.clock.set(monday(3, 0)?);
    h.manager.set_queue_paused(DEFAULT_QUEUE_ID, false)?;
    h.wait_status(row.id, DownloadStatus::Completed).await;
    wait_until("the run to end", || (!in_run(&h)).then_some(())).await;
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_start_time_without_stop_runs_until_the_queue_is_empty() -> TestResult {
    let server = fast_server(16 * KIB).await?;
    let h = Harness::at(monday(1, 0)?).await?;
    schedule_default_queue(
        &h,
        Schedule {
            start: tod(2, 0),
            stop: None,
            days: vec![],
        },
    )?;
    let first = h.add(&server.url("one.bin")).await?;
    let second = h.add(&server.url("two.bin")).await?;
    assert_eq!(first.status, DownloadStatus::Queued);

    h.clock.set(monday(2, 0)?);
    h.wait_status(first.id, DownloadStatus::Completed).await;
    h.wait_status(second.id, DownloadStatus::Completed).await;

    // The run ended with the queue empty; new work waits for the next start.
    h.clock.set(monday(9, 0)?);
    let late = h.add(&server.url("three.bin")).await?;
    assert_eq!(late.status, DownloadStatus::Queued);
    h.clock.set(monday(2, 0)? + TimeDelta::days(1));
    h.wait_status(late.id, DownloadStatus::Completed).await;
    Ok(())
}
