//! A body of unknown size is cut, and the retry's answer is not the rest of that file.

use std::io;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use wirecatch_engine::units::{KIB_USIZE, MIB_USIZE};
use wirecatch_engine::{
    Backoff, CancellationToken, DownloadOptions, DownloadRequest, Error, Fetcher, Progress, Url,
    download,
};
use wirecatch_test_support::{PATIENCE, TestResult, private_dir};

const KEPT: usize = MIB_USIZE;
const SHORT_PAGE: &[u8] = b"<html>sign in</html>";

#[derive(Clone, Copy)]
struct Script {
    /// The first answer carries an `ETag`, which the retry's does not contradict.
    tagged: bool,
    /// The retry's answer says its size.
    sized: bool,
    /// The retry's page: `SHORT_PAGE` repeated this many times.
    repeats: usize,
}

/// The first answer is chunked, `KEPT` bytes, then the connection ends with no last chunk. Every later answer is a `200` with a page.
async fn answer(mut socket: TcpStream, number: usize, script: Script) -> io::Result<()> {
    let mut head = Vec::new();
    let mut buf = [0u8; KIB_USIZE];
    while !head.windows(4).any(|w| w == b"\r\n\r\n") {
        let n = socket.read(&mut buf).await?;
        if n == 0 {
            return Ok(());
        }
        head.extend_from_slice(&buf[..n]);
    }
    let tag = if script.tagged {
        "ETag: \"v1\"\r\n"
    } else {
        ""
    };
    if number == 0 {
        let start = format!(
            "HTTP/1.1 200 OK\r\n{tag}Transfer-Encoding: chunked\r\nConnection: close\r\n\r\n"
        );
        let mut cut = start.into_bytes();
        cut.extend_from_slice(format!("{KEPT:x}\r\n").as_bytes());
        cut.extend(std::iter::repeat_n(b'A', KEPT));
        cut.extend_from_slice(b"\r\n");
        socket.write_all(&cut).await?;
    } else {
        let page = SHORT_PAGE.repeat(script.repeats);
        let size = if script.sized {
            format!("Content-Length: {}\r\n", page.len())
        } else {
            String::new()
        };
        let head = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\n{tag}{size}Connection: close\r\n\r\n"
        );
        socket.write_all(head.as_bytes()).await?;
        socket.write_all(&page).await?;
    }
    socket.shutdown().await
}

async fn cut_then_page(script: Script) -> TestResult<(Url, Arc<AtomicUsize>)> {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let port = listener.local_addr()?.port();
    let answers = Arc::new(AtomicUsize::new(0));
    let counted = answers.clone();
    tokio::spawn(async move {
        while let Ok((socket, _)) = listener.accept().await {
            let number = counted.fetch_add(1, Ordering::SeqCst);
            tokio::spawn(answer(socket, number, script));
        }
    });
    let url = Url::parse(&format!("http://127.0.0.1:{port}/f.bin"))?;
    Ok((url, answers))
}

fn quick_retries() -> DownloadOptions {
    let mut options = DownloadOptions::default();
    options.max_retries = 2;
    options.retry_backoff = Backoff::new(Duration::from_millis(1), Duration::from_millis(1));
    options
}

/// How a download and a save of the script's link ended.
async fn both_fail_with(script: Script, name: &str, expected: fn(&Error) -> bool) -> TestResult {
    let (url, answers) = cut_then_page(script).await?;
    let dir = private_dir(name)?;
    let mut request = DownloadRequest::new(url, dir.to_path_buf());
    request.options = quick_retries();
    let run = download(
        request,
        Arc::new(Progress::default()),
        CancellationToken::new(),
    );
    let result = tokio::time::timeout(PATIENCE, run).await?;
    assert!(answers.load(Ordering::SeqCst) >= 1);
    assert!(
        result.as_ref().is_err_and(expected),
        "the download: {result:?}"
    );
    dir.remove()?;

    let (url, answers) = cut_then_page(script).await?;
    let dir = private_dir(name)?;
    let path = dir.join("segment.ts");
    let fetcher = Fetcher::new(&url, &quick_retries(), CancellationToken::new()).await?;
    let run = fetcher.save(&url, None, &path, |_| {});
    let result = tokio::time::timeout(PATIENCE, run).await?;
    assert!(answers.load(Ordering::SeqCst) >= 2, "no retry was made");
    assert!(result.as_ref().is_err_and(expected), "the save: {result:?}");
    dir.remove()?;
    Ok(())
}

fn cut_short(error: &Error) -> bool {
    matches!(error, Error::PrematureEof { .. })
}

fn changed(error: &Error) -> bool {
    matches!(error, Error::RemoteChanged)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_retry_that_says_it_is_shorter_than_the_bytes_kept_is_not_the_end() -> TestResult {
    let script = Script {
        tagged: true,
        sized: true,
        repeats: 1,
    };
    both_fail_with(script, "short-retry-sized", cut_short).await
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_retry_that_ends_before_the_bytes_kept_is_not_the_end() -> TestResult {
    let script = Script {
        tagged: true,
        sized: false,
        repeats: 1,
    };
    both_fail_with(script, "short-retry-unsized", cut_short).await
}

/// Nothing ties the retry's answer to the first one, so its tail must not be joined to what was kept, however long it is.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_retry_with_nothing_to_compare_is_not_joined_to_the_bytes_kept() -> TestResult {
    let script = Script {
        tagged: false,
        sized: true,
        repeats: 2 * KEPT / SHORT_PAGE.len(),
    };
    both_fail_with(script, "long-retry-untagged", changed).await
}
