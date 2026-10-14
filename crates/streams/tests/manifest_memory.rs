//! What reading a hostile manifest holds in memory, measured by counting the bytes allocated rather than by estimating what the parsed manifest keeps. A manifest is at most 8 MiB, but a DASH representation inherits from its adaptation set and period, so whatever it keeps of theirs as its own copy is multiplied by the number of representations.

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Mutex, PoisonError};

use wirecatch_engine::{CancellationToken, DownloadOptions};
use wirecatch_streams::{Error, PrivateNetwork, StreamInfo, inspect};
use wirecatch_test_support::TestResult;
use wirecatch_test_support::files::FileServer;

/// The bytes the process has allocated and not yet freed, on every thread, so a reading moved to another thread is still measured.
static NOW: AtomicUsize = AtomicUsize::new(0);
/// The most [`NOW`] was since [`measure`].
static PEAK: AtomicUsize = AtomicUsize::new(0);

/// One test at a time, from building its manifest to dropping what it read: the counters are the whole process's.
static ALONE: Mutex<()> = Mutex::new(());

/// The system allocator, counting the bytes it holds.
struct Counting;

impl Counting {
    fn add(bytes: usize) {
        let now = NOW
            .fetch_add(bytes, Ordering::Relaxed)
            .saturating_add(bytes);
        PEAK.fetch_max(now, Ordering::Relaxed);
    }

    fn remove(bytes: usize) {
        NOW.fetch_sub(bytes, Ordering::Relaxed);
    }
}

// SAFETY: every call goes to the system allocator unchanged; the counters only add up the sizes it was asked for.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        // SAFETY: the caller keeps `alloc`'s contract, which is the system allocator's.
        let block = unsafe { System.alloc(layout) };
        if !block.is_null() {
            Self::add(layout.size());
        }
        block
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        // SAFETY: as for `alloc`.
        let block = unsafe { System.alloc_zeroed(layout) };
        if !block.is_null() {
            Self::add(layout.size());
        }
        block
    }

    unsafe fn dealloc(&self, block: *mut u8, layout: Layout) {
        // SAFETY: `block` came from this allocator, which is the system allocator, with `layout`.
        unsafe { System.dealloc(block, layout) };
        Self::remove(layout.size());
    }

    unsafe fn realloc(&self, block: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        // SAFETY: as for `dealloc`, and the caller keeps `realloc`'s contract for `size`.
        let moved = unsafe { System.realloc(block, layout, size) };
        if !moved.is_null() {
            Self::remove(layout.size());
            Self::add(size);
        }
        moved
    }
}

#[global_allocator]
static COUNTING: Counting = Counting;

/// Starts a measurement: the bytes held now, from which [`peak_since`] counts.
fn measure() -> usize {
    let now = NOW.load(Ordering::Relaxed);
    PEAK.store(now, Ordering::Relaxed);
    now
}

/// The most bytes held at once, above `start`, since the measurement started.
fn peak_since(start: usize) -> usize {
    PEAK.load(Ordering::Relaxed).saturating_sub(start)
}

const MIB: usize = 1024 * 1024;

const VIDEO: &str = r#"mimeType="video/mp4""#;

/// The most reading a manifest of `len` bytes may hold at once: a multiple of its text, never of what it repeats. The text is held a few times over on its way in, and its parsed form takes about twenty bytes for each byte of a manifest of small elements (measured: a 1 MB timeline of 100,000 entries held 23 MB at most).
fn allowed(len: usize) -> usize {
    32 * len + 8 * MIB
}

/// Serves `manifest` and inspects it, returning what `inspect` gave and the most bytes held at once meanwhile.
fn inspect_measured(manifest: String) -> TestResult<(Result<StreamInfo, Error>, usize)> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    runtime.block_on(async {
        let server = FileServer::start().await;
        server.put_typed("manifest.mpd", manifest, "application/dash+xml");
        let url = server.url("manifest.mpd");
        let start = measure();
        let info = inspect(
            &url,
            &DownloadOptions::default(),
            PrivateNetwork::Auto,
            CancellationToken::new(),
        )
        .await;
        Ok((info, peak_since(start)))
    })
}

/// A manifest of `count` representations with the attributes `rep`, in one adaptation set with the attributes `set` and the elements `inside`, after the manifest's own elements `outer`.
fn manifest(outer: &str, set: &str, inside: &str, count: usize, rep: &str) -> String {
    format!(
        r#"<MPD mediaPresentationDuration="PT10S">{outer}<Period><AdaptationSet {set}>{inside}{}</AdaptationSet></Period></MPD>"#,
        format!("<Representation {rep}/>").repeat(count)
    )
}

/// Inspects the manifest `text` builds, alone in the process, and checks that it is read (or refused as too large, when `accepted` is false) holding at most [`allowed`].
fn check(name: &str, text: impl FnOnce() -> String, accepted: bool) -> TestResult {
    let _alone = ALONE.lock().unwrap_or_else(PoisonError::into_inner);
    let text = text();
    let len = text.len();
    let (info, peak) = inspect_measured(text)?;
    match (&info, accepted) {
        (Ok(_), true) | (Err(Error::TooLarge), false) => {}
        _ => {
            return Err(format!(
                "{name}: expected {}, got {:?}",
                if accepted {
                    "it read"
                } else {
                    "Error::TooLarge"
                },
                info.map(|i| i.video.len() + i.audio.len())
            )
            .into());
        }
    }
    // Fetching the text holds it at least once; less means the counting allocator measured nothing.
    assert!(
        peak >= len,
        "{name}: reading a manifest of {len} bytes held only {peak} bytes"
    );
    assert!(
        peak <= allowed(len),
        "{name}: reading a manifest of {len} bytes held {peak} bytes at once, more than {}",
        allowed(len)
    );
    Ok(())
}

/// A long base address on the manifest, inherited by thousands of representations.
#[test]
fn representations_share_the_base_address_they_inherit() -> TestResult {
    let text = || {
        let base = format!(
            "<BaseURL>https://cdn.example/{}/</BaseURL>",
            "a".repeat(8000)
        );
        manifest(&base, VIDEO, "", 4000, r#"bandwidth="1""#)
    };
    check("a long base address", text, true)
}

/// A long segment template on the adaptation set, inherited by every representation in it.
#[test]
fn representations_share_the_template_they_inherit() -> TestResult {
    let text = || {
        let template = format!(
            r#"<SegmentTemplate duration="1" media="{}$Number$"/>"#,
            "a".repeat(MIB)
        );
        manifest("", VIDEO, &template, 200, r#"bandwidth="1""#)
    };
    check("a long template", text, true)
}

/// A timeline on the adaptation set, inherited by every representation in it.
#[test]
fn representations_share_the_timeline_they_inherit() -> TestResult {
    let text = || {
        let template = format!(
            r#"<SegmentTemplate media="$Time$"><SegmentTimeline>{}</SegmentTimeline></SegmentTemplate>"#,
            r#"<S d="1"/>"#.repeat(100_000)
        );
        manifest("", VIDEO, &template, 40, r#"bandwidth="1""#)
    };
    check("a long timeline", text, true)
}

/// A long label on the adaptation set, which every representation in it is reported with.
#[test]
fn a_long_name_repeated_for_every_representation_is_refused() -> TestResult {
    let text = || {
        let label = format!("<Label>{}</Label>", "a".repeat(MIB));
        manifest(
            "",
            r#"mimeType="audio/mp4""#,
            &label,
            200,
            r#"bandwidth="1""#,
        )
    };
    check("a long label", text, false)
}

/// A 24-hour programme in 2-second segments listed once on the adaptation set and shared by its representations, each with its own base address: a rare but valid long VOD.
#[test]
fn representations_share_the_segment_list_they_inherit() -> TestResult {
    let text = || {
        let list = format!(
            "<SegmentList>{}</SegmentList>",
            r#"<SegmentURL media="seg.m4s"/>"#.repeat(43_200)
        );
        let reps = (0..20)
            .map(|n| {
                format!(
                    r#"<Representation bandwidth="{n}"><BaseURL>q{n}/</BaseURL></Representation>"#
                )
            })
            .collect::<Vec<_>>();
        manifest("", VIDEO, &(list + &reps.concat()), 0, "")
    };
    check("a long segment list", text, true)
}
