//! The work inspecting a hostile HLS master playlist costs, counted in allocations rather than time: `CODECS` makes every comparison of a quality allocate, so quadratic work shows up whatever the machine's load.

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicU64, Ordering};

use wirecatch_engine::{CancellationToken, DownloadOptions};
use wirecatch_streams::{Error, PrivateNetwork, inspect};
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

const MEDIA: &str = "#EXTM3U\n#EXTINF:1,\ns.ts\n#EXT-X-ENDLIST\n";

#[tokio::test]
async fn a_master_with_more_qualities_than_any_video_is_refused_without_comparing_them()
-> TestResult {
    const QUALITIES: u64 = 8_000;
    let server = FileServer::start().await;
    let line = "#EXT-X-STREAM-INF:CODECS=\"x\"\na\n";
    let text = format!("#EXTM3U\n{}", line.repeat(usize::try_from(QUALITIES)?));
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
    let spent = ALLOCATIONS.load(Ordering::Relaxed) - before;

    assert!(
        spent <= 200 * QUALITIES,
        "{spent} allocations to inspect {QUALITIES} qualities"
    );
    assert!(matches!(inspected, Err(Error::TooLarge)), "{inspected:?}");
    Ok(())
}
