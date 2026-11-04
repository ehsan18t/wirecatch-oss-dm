//! The work inspecting a hostile HLS master playlist costs, counted in allocations rather than time: `CODECS` makes every comparison of a quality allocate, so quadratic work shows up whatever the machine's load.

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicU64, Ordering};

use tokio::sync::Mutex;

use wirecatch_engine::{CancellationToken, DownloadOptions};
use wirecatch_streams::{Error, PrivateNetwork, StreamInfo, inspect};
use wirecatch_test_support::TestResult;
use wirecatch_test_support::files::FileServer;

static ALLOCATIONS: AtomicU64 = AtomicU64::new(0);

struct Counting;

// SAFETY: every call goes to the system allocator unchanged; the counter only adds up how many were asked for.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
        // SAFETY: the caller keeps `alloc`'s contract, which is the system allocator's.
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, block: *mut u8, layout: Layout) {
        // SAFETY: `block` came from this allocator, which is the system allocator, with `layout`.
        unsafe { System.dealloc(block, layout) };
    }
}

#[global_allocator]
static COUNTING: Counting = Counting;

/// Far above what reading one quality takes, far below comparing it with every other.
const WORK_PER_QUALITY: u64 = 200;

const MEDIA: &str = "#EXTM3U\n#EXTINF:1,\ns.ts\n#EXT-X-ENDLIST\n";

/// One test at a time: the count is the whole process's.
static ALONE: Mutex<()> = Mutex::const_new(());

/// Inspects a master of `qualities` qualities, each of which allocates to tell whether it is audio only: what it gave, and the allocations it took.
async fn inspect_master(qualities: u64) -> TestResult<(Result<StreamInfo, Error>, u64)> {
    let server = FileServer::start().await;
    let line = "#EXT-X-STREAM-INF:CODECS=\"x\"\na\n";
    let text = format!("#EXTM3U\n{}", line.repeat(usize::try_from(qualities)?));
    server.put_typed("m.m3u8", text, "application/vnd.apple.mpegurl");
    server.put_typed("a", MEDIA, "application/vnd.apple.mpegurl");
    let before = ALLOCATIONS.load(Ordering::Relaxed);
    let inspected = inspect(
        &server.url("m.m3u8"),
        &DownloadOptions::default(),
        PrivateNetwork::Auto,
        CancellationToken::new(),
    )
    .await;
    Ok((inspected, ALLOCATIONS.load(Ordering::Relaxed) - before))
}

#[tokio::test]
async fn a_master_with_more_qualities_than_any_video_is_refused_without_comparing_them()
-> TestResult {
    const QUALITIES: u64 = 8_000;
    let _alone = ALONE.lock().await;
    let (inspected, spent) = inspect_master(QUALITIES).await?;
    assert!(
        spent <= WORK_PER_QUALITY * QUALITIES,
        "{spent} allocations to inspect {QUALITIES} qualities"
    );
    assert!(matches!(inspected, Err(Error::TooLarge)), "{inspected:?}");
    Ok(())
}

#[tokio::test]
async fn a_master_at_the_limit_costs_work_in_proportion_to_its_qualities() -> TestResult {
    const QUALITIES: u64 = 4_096;
    let _alone = ALONE.lock().await;
    let (inspected, spent) = inspect_master(QUALITIES).await?;
    assert!(
        spent <= WORK_PER_QUALITY * QUALITIES,
        "{spent} allocations to inspect {QUALITIES} qualities"
    );
    assert_eq!(inspected?.video.len(), 4_096);
    Ok(())
}
