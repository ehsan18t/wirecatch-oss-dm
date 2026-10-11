//! Options that cannot work are refused before anything is sent. A zero buffer size used to hang a thread, a zero split size panicked, and a zero checkpoint interval silently turned off resume files.

mod common;

use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;

use common::{Config, TestServer, test_data};
use wirecatch_engine::{
    CancellationToken, DownloadOptions, DownloadRequest, Error, Fetcher, Progress, download,
};

/// Long enough for a refused download to return, short enough that a hang fails the test instead of stalling it.
const DEADLINE: Duration = Duration::from_secs(10);

/// An option's name and a way to set it to zero.
type Zeroed = (&'static str, fn(&mut DownloadOptions));

/// Each option that must be greater than zero.
fn zeroed() -> Vec<Zeroed> {
    vec![
        ("connections", |o| o.connections = 0),
        ("min_split", |o| o.min_split = 0),
        ("buffer_size", |o| o.buffer_size = 0),
        ("stall_timeout", |o| o.stall_timeout = Duration::ZERO),
        ("connect_timeout", |o| o.connect_timeout = Duration::ZERO),
        ("checkpoint_interval", |o| {
            o.checkpoint_interval = Duration::ZERO
        }),
    ]
}

#[test]
fn the_defaults_are_valid() {
    DownloadOptions::default().validate().unwrap();
}

#[tokio::test]
async fn a_download_with_an_unworkable_option_is_refused_before_any_request() {
    let server = TestServer::start(test_data(1024), Config::default()).await;
    let dir = std::env::temp_dir().join(format!("wirecatch-options-{}", std::process::id()));
    for (name, zero) in zeroed() {
        let mut request = DownloadRequest::new(server.url("file.bin"), &dir);
        zero(&mut request.options);
        let result = tokio::time::timeout(
            DEADLINE,
            download(
                request,
                Arc::new(Progress::default()),
                CancellationToken::new(),
            ),
        )
        .await
        .unwrap_or_else(|_| panic!("a zero {name} hung the download"));
        match result {
            Err(Error::InvalidOption(option)) => assert_eq!(option, name),
            other => panic!("a zero {name} gave {other:?}"),
        }
    }
    assert_eq!(server.stats.requests.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn a_fetcher_with_an_unworkable_option_is_refused() {
    let server = TestServer::start(test_data(1024), Config::default()).await;
    for (name, zero) in zeroed() {
        let mut options = DownloadOptions::default();
        zero(&mut options);
        match Fetcher::new(&server.url("file.bin"), &options, CancellationToken::new()).await {
            Err(Error::InvalidOption(option)) => assert_eq!(option, name),
            Err(other) => panic!("a zero {name} gave {other}"),
            Ok(_) => panic!("a zero {name} was accepted"),
        }
    }
}
