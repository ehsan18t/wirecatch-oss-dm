//! Acceptance 2 and 4: concurrency limits per queue and overall, and speed limits
//! per queue and overall, including changes while downloading.

use std::collections::HashMap;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use tokio::sync::broadcast::error::RecvError;
use wirecatch_core::DEFAULT_QUEUE_ID;
use wirecatch_protocol::app::{DownloadStatus, Queue, UiEvent};

use crate::common::{Harness, TestResult, fast_server, is_running, throttled_server, tune};
use wirecatch_engine::units::KIB;

fn set_queue(
    h: &Harness,
    id: wirecatch_protocol::QueueId,
    change: impl FnOnce(&mut Queue),
) -> TestResult {
    let mut queue = h
        .manager
        .list_queues()
        .into_iter()
        .find(|q| q.id == id)
        .ok_or("no such queue")?;
    change(&mut queue);
    h.manager.update_queue(queue)?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn queue_and_global_concurrency_limits_hold() -> TestResult {
    let server = throttled_server(64 * KIB, 128 * KIB).await?;
    let h = Harness::new().await?;
    tune(&h.manager, |s| s.max_concurrent_downloads = 3)?;
    set_queue(&h, DEFAULT_QUEUE_ID, |q| q.max_concurrent = 1)?;
    let second = h.manager.create_queue("Second").unwrap();
    set_queue(&h, second.id, |q| q.max_concurrent = 3)?;
    let limits = HashMap::from([(DEFAULT_QUEUE_ID, 1), (second.id, 3)]);

    let mut events = h.manager.subscribe();
    let mut ids = Vec::new();
    for i in 0..4 {
        ids.push(h.add(&server.url(&format!("a{i}.bin"))).await?.id);
        let queue = Some(second.id);
        ids.push(
            h.add_with(&server.url(&format!("b{i}.bin")), |r| r.queue = queue)
                .await?
                .id,
        );
    }

    // Check the limits every time something changes, until everything is done.
    let mut peak = 0;
    loop {
        let rows = h.manager.list_downloads();
        let mut per_queue: HashMap<_, u32> = HashMap::new();
        for row in rows.iter().filter(|r| is_running(r)) {
            *per_queue.entry(row.queue).or_default() += 1;
        }
        for (queue, running) in &per_queue {
            assert!(running <= &limits[queue], "queue {queue:?} runs {running}");
        }
        let total: u32 = per_queue.values().sum();
        assert!(total <= 3, "{total} downloads run at once");
        peak = peak.max(total);
        if rows.iter().all(|r| r.status == DownloadStatus::Completed) {
            break;
        }
        let event = tokio::time::timeout(crate::common::PATIENCE, events.recv())
            .await
            .expect("downloads stopped making progress");
        // A lagged receiver is fine here: the next loop reads the full list anyway.
        assert!(
            !matches!(event, Err(RecvError::Closed)),
            "the manager stopped sending events"
        );
    }
    assert_eq!(peak, 3, "the global limit was never reached");
    assert!(server.stats.max_concurrent.load(Ordering::SeqCst) <= 3);
    assert_eq!(server.stats.requests.load(Ordering::SeqCst), ids.len());
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn lowering_a_limit_sends_extra_downloads_back_in_line() -> TestResult {
    let server = throttled_server(256 * KIB, 64 * KIB).await?;
    let h = Harness::new().await?;
    let a = h.add(&server.url("a.bin")).await?;
    let b = h.add(&server.url("b.bin")).await?;
    h.wait_for(b.id, "both running", |r| {
        is_running(r) && is_running(&h.row(a.id))
    })
    .await;

    tune(&h.manager, |s| s.max_concurrent_downloads = 1)?;
    h.wait_status(b.id, DownloadStatus::Queued).await;
    assert!(is_running(&h.row(a.id)));
    Ok(())
}

/// Times one 128 KiB download under the given queue and global limits.
async fn time_download(queue_limit: u64, global_limit: u64) -> TestResult<Duration> {
    let server = fast_server(128 * KIB).await?;
    let h = Harness::new().await?;
    tune(&h.manager, |s| s.global_speed_limit = Some(global_limit))?;
    set_queue(&h, DEFAULT_QUEUE_ID, |q| q.speed_limit = Some(queue_limit))?;
    let start = Instant::now();
    let row = h.add(&server.url("f.bin")).await?;
    h.wait_status(row.id, DownloadStatus::Completed).await;
    Ok(start.elapsed())
}

// 128 KiB at 64 KiB/s takes 2 s, less the limiter's 16 KiB burst allowance.
const SLOWEST_FAIR: Duration = Duration::from_millis(1600);
// Comfortably under what 32 KiB/s (half the stricter limit) would take.
const FASTEST_UNFAIR: Duration = Duration::from_millis(3500);

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_download_obeys_a_stricter_queue_limit() -> TestResult {
    let took = time_download(64 * KIB, 128 * KIB).await?;
    assert!(
        (SLOWEST_FAIR..FASTEST_UNFAIR).contains(&took),
        "took {took:?}"
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_download_obeys_a_stricter_global_limit() -> TestResult {
    let took = time_download(128 * KIB, 64 * KIB).await?;
    assert!(
        (SLOWEST_FAIR..FASTEST_UNFAIR).contains(&took),
        "took {took:?}"
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn limit_changes_apply_to_running_downloads() -> TestResult {
    let server = fast_server(512 * KIB).await?;
    let h = Harness::new().await?;
    set_queue(&h, DEFAULT_QUEUE_ID, |q| q.speed_limit = Some(16 * KIB))?;
    let start = Instant::now();
    let row = h.add(&server.url("f.bin")).await?;
    h.wait_for(row.id, "progress", |r| r.done > 0).await;

    // At 16 KiB/s this would take half a minute; lifting the limit finishes it now.
    set_queue(&h, DEFAULT_QUEUE_ID, |q| q.speed_limit = None)?;
    h.wait_status(row.id, DownloadStatus::Completed).await;
    assert!(
        start.elapsed() < Duration::from_secs(10),
        "{:?}",
        start.elapsed()
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn progress_events_are_batched() -> TestResult {
    let server = throttled_server(256 * KIB, 128 * KIB).await?;
    let h = Harness::new().await?;
    let mut events = h.manager.subscribe();
    let row = h.add(&server.url("f.bin")).await?;
    h.wait_for(row.id, "progress", |r| r.done > 0).await;

    let window = Duration::from_secs(1);
    let start = Instant::now();
    let mut batches = 0;
    let mut speeds = Vec::new();
    while start.elapsed() < window {
        if let Ok(Ok(UiEvent::DownloadsChanged(rows))) =
            tokio::time::timeout(window, events.recv()).await
        {
            batches += 1;
            speeds.extend(rows.iter().filter(|r| r.id == row.id).map(|r| r.speed));
        }
    }
    assert!(batches <= 12, "{batches} batches in one second");
    assert!(batches >= 5, "only {batches} batches while downloading");
    let speed = speeds
        .iter()
        .rev()
        .copied()
        .find(|s| *s > 0)
        .expect("no speed reading while downloading");
    // Smoothed speed sits near the server's 128 KiB/s.
    assert!((64 * KIB..256 * KIB).contains(&speed), "speed {speed}");
    Ok(())
}
