//! Options that cannot work are refused before anything is sent. A zero buffer size used to hang a thread, a zero split size panicked, and a zero checkpoint interval silently turned off resume files.

use std::future::Future;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::sync::mpsc;
use std::time::Duration;

use wirecatch_engine::{
    CancellationToken, DownloadOptions, DownloadRequest, Error, Fetcher, MAX_BUFFER_SIZE,
    MAX_CONNECTIONS, Progress, download,
};
use wirecatch_test_support::http::{Config, TestServer};
use wirecatch_test_support::{TestResult, private_dir, test_data};

/// Long enough for a refused download to return, short enough that a hang fails the test instead of stalling it.
const DEADLINE: Duration = Duration::from_secs(10);

/// An option's name and a way to set it out of range.
type Unworkable = (&'static str, fn(&mut DownloadOptions));

/// Each option set to a value that cannot work: zero, or above its limit.
fn unworkable() -> Vec<Unworkable> {
    vec![
        ("connections", |o| o.connections = 0),
        ("connections", |o| o.connections = MAX_CONNECTIONS + 1),
        ("min_split", |o| o.min_split = 0),
        ("buffer_size", |o| o.buffer_size = 0),
        ("buffer_size", |o| o.buffer_size = MAX_BUFFER_SIZE + 1),
        ("stall_timeout", |o| o.stall_timeout = Duration::ZERO),
        ("connect_timeout", |o| o.connect_timeout = Duration::ZERO),
        ("checkpoint_interval", |o| {
            o.checkpoint_interval = Duration::ZERO;
        }),
    ]
}

/// Runs `future` on a thread and runtime of its own and fails the test if it does not finish within [`DEADLINE`]. A `tokio::time::timeout` around it cannot catch a future that never yields (a zero buffer size used to loop without one): the timer never gets to run. A thread that hangs is left behind.
fn within_deadline<T: Send + 'static>(
    what: &str,
    future: impl Future<Output = T> + Send + 'static,
) -> TestResult<T> {
    let (done, finished) = mpsc::channel();
    std::thread::spawn(move || {
        let result = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map(|runtime| runtime.block_on(future));
        // The receiver is gone only once the test has already failed on the deadline.
        drop(done.send(result));
    });
    let finished = finished
        .recv_timeout(DEADLINE)
        .map_err(|_| format!("{what} hung the download"))?;
    Ok(finished?)
}

#[test]
fn the_defaults_are_valid() {
    DownloadOptions::default().validate().unwrap();
}

// Multi-threaded, so the test server keeps answering while this thread waits for the deadline.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_download_with_an_unworkable_option_is_refused_before_any_request() {
    let server = TestServer::start(test_data(1024), Config::default()).await;
    let dir = private_dir("options").unwrap();
    for (name, unworkable) in unworkable() {
        let mut request = DownloadRequest::new(server.url("file.bin"), dir.path());
        unworkable(&mut request.options);
        let refused = download(
            request,
            Arc::new(Progress::default()),
            CancellationToken::new(),
        );
        let result = tokio::task::block_in_place(|| {
            within_deadline(&format!("a bad {name}"), refused).unwrap()
        });
        match result {
            Err(Error::InvalidOption(option)) => assert_eq!(option, name),
            other => panic!("a bad {name} gave {other:?}"),
        }
    }
    assert_eq!(server.stats.requests.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn a_fetcher_with_an_unworkable_option_is_refused() {
    let server = TestServer::start(test_data(1024), Config::default()).await;
    for (name, unworkable) in unworkable() {
        let mut options = DownloadOptions::default();
        unworkable(&mut options);
        match Fetcher::new(&server.url("file.bin"), &options, CancellationToken::new()).await {
            Err(Error::InvalidOption(option)) => assert_eq!(option, name),
            Err(other) => panic!("a bad {name} gave {other}"),
            Ok(_) => panic!("a bad {name} was accepted"),
        }
    }
}
