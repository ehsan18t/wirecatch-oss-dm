//! A body of unknown size is cut, and the retry's answer ends before the bytes already kept.

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

/// The first answer is chunked, `KEPT` bytes, then the connection ends with no last chunk. Every later answer is a `200` with a short body.
async fn answer(mut socket: TcpStream, number: usize, sized: bool) -> io::Result<()> {
    let mut head = Vec::new();
    let mut buf = [0u8; KIB_USIZE];
    while !head.windows(4).any(|w| w == b"\r\n\r\n") {
        let n = socket.read(&mut buf).await?;
        if n == 0 {
            return Ok(());
        }
        head.extend_from_slice(&buf[..n]);
    }
    if number == 0 {
        let mut cut =
            b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n".to_vec();
        cut.extend_from_slice(format!("{KEPT:x}\r\n").as_bytes());
        cut.extend(std::iter::repeat_n(b'A', KEPT));
        cut.extend_from_slice(b"\r\n");
        socket.write_all(&cut).await?;
    } else {
        let size = if sized {
            format!("Content-Length: {}\r\n", SHORT_PAGE.len())
        } else {
            String::new()
        };
        let head = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\n{size}Connection: close\r\n\r\n"
        );
        socket.write_all(head.as_bytes()).await?;
        socket.write_all(SHORT_PAGE).await?;
    }
    socket.shutdown().await
}

async fn cut_then_short(sized: bool) -> TestResult<(Url, Arc<AtomicUsize>)> {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let port = listener.local_addr()?.port();
    let answers = Arc::new(AtomicUsize::new(0));
    let counted = answers.clone();
    tokio::spawn(async move {
        while let Ok((socket, _)) = listener.accept().await {
            let number = counted.fetch_add(1, Ordering::SeqCst);
            tokio::spawn(answer(socket, number, sized));
        }
    });
    let url = Url::parse(&format!("http://127.0.0.1:{port}/f.bin"))?;
    Ok((url, answers))
}

fn assert_cut_short<T: std::fmt::Debug>(result: &Result<T, Error>, answers: &AtomicUsize) {
    assert!(answers.load(Ordering::SeqCst) >= 2, "no retry was made");
    assert!(
        matches!(result, Err(Error::PrematureEof { .. })),
        "the short answer did not fail the run: {result:?}"
    );
}

fn quick_retries() -> DownloadOptions {
    let mut options = DownloadOptions::default();
    options.max_retries = 2;
    options.retry_backoff = Backoff::new(Duration::from_millis(1), Duration::from_millis(1));
    options
}

async fn download_fails(sized: bool, name: &str) -> TestResult {
    let (url, answers) = cut_then_short(sized).await?;
    let dir = private_dir(name)?;
    let mut request = DownloadRequest::new(url, dir.to_path_buf());
    request.options = quick_retries();
    let run = download(
        request,
        Arc::new(Progress::default()),
        CancellationToken::new(),
    );
    let result = tokio::time::timeout(PATIENCE, run).await?;
    assert_cut_short(&result, &answers);
    dir.remove()?;
    Ok(())
}

async fn save_fails(sized: bool, name: &str) -> TestResult {
    let (url, answers) = cut_then_short(sized).await?;
    let dir = private_dir(name)?;
    let path = dir.join("segment.ts");
    let fetcher = Fetcher::new(&url, &quick_retries(), CancellationToken::new()).await?;
    let run = fetcher.save(&url, None, &path, |_| {});
    let result = tokio::time::timeout(PATIENCE, run).await?;
    assert_cut_short(&result, &answers);
    dir.remove()?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_download_is_not_complete_when_a_retry_says_it_is_shorter_than_the_bytes_kept()
-> TestResult {
    download_fails(true, "short-retry-sized").await
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_download_is_not_complete_when_a_retry_ends_before_the_bytes_kept() -> TestResult {
    download_fails(false, "short-retry-unsized").await
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_save_is_not_whole_when_a_retry_says_it_is_shorter_than_the_bytes_kept() -> TestResult {
    save_fails(true, "short-retry-save-sized").await
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_save_is_not_whole_when_a_retry_ends_before_the_bytes_kept() -> TestResult {
    save_fails(false, "short-retry-save-unsized").await
}
