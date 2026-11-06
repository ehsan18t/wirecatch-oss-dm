//! A failed exchange says where it broke: a server that answered and then closed the connection, or one whose answer is not HTTP, is not a server that could not be reached.

use std::io;
use std::sync::Arc;
use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use wirecatch_engine::units::KIB_USIZE;
use wirecatch_engine::{
    Backoff, CancellationToken, DownloadOptions, DownloadRequest, Error, NetworkFailure, Progress,
    Url, download,
};
use wirecatch_test_support::{PATIENCE, TestResult, private_dir};

const HEADERS_THEN_CLOSE: &[u8] =
    b"HTTP/1.1 200 OK\r\nContent-Length: 1048576\r\nConnection: close\r\n\r\n";
const BROKEN_HEADER: &[u8] =
    b"HTTP/1.1 200 OK\r\nContent Length: 10\r\nConnection: close\r\n\r\n0123456789";

/// Reads one request's head, then sends `reply` and closes.
async fn answer(mut socket: TcpStream, reply: &'static [u8]) -> io::Result<()> {
    let mut head = Vec::new();
    let mut buf = [0u8; KIB_USIZE];
    while !head.windows(4).any(|w| w == b"\r\n\r\n") {
        let n = socket.read(&mut buf).await?;
        if n == 0 {
            return Ok(());
        }
        head.extend_from_slice(&buf[..n]);
    }
    socket.write_all(reply).await?;
    socket.shutdown().await
}

/// How a download from a server that answers every request with `reply` fails.
async fn failure_with(reply: &'static [u8], name: &str) -> TestResult<NetworkFailure> {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let port = listener.local_addr()?.port();
    let server = tokio::spawn(async move {
        while let Ok((socket, _)) = listener.accept().await {
            tokio::spawn(answer(socket, reply));
        }
    });
    let dir = private_dir(name)?;
    let url = Url::parse(&format!("http://127.0.0.1:{port}/f.bin"))?;
    let mut request = DownloadRequest::new(url, dir.to_path_buf());
    let mut options = DownloadOptions::default();
    options.max_retries = 1;
    options.retry_backoff = Backoff::new(Duration::from_millis(1), Duration::from_millis(1));
    request.options = options;
    let run = download(
        request,
        Arc::new(Progress::default()),
        CancellationToken::new(),
    );
    let result = tokio::time::timeout(PATIENCE, run).await?;
    server.abort();
    dir.remove()?;
    match result {
        Err(Error::Network(error)) => Ok(error.failure()),
        other => Err(format!("not a network error: {other:?}").into()),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_server_that_answers_then_closes_dropped_the_connection() -> TestResult {
    let failure = failure_with(HEADERS_THEN_CLOSE, "network-dropped").await?;
    assert_eq!(failure, NetworkFailure::Dropped);
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_reply_that_is_not_http_could_not_be_read() -> TestResult {
    let failure = failure_with(BROKEN_HEADER, "network-unreadable").await?;
    assert_eq!(failure, NetworkFailure::Unreadable);
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_closed_port_could_not_be_reached() -> TestResult {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let port = listener.local_addr()?.port();
    drop(listener);
    let dir = private_dir("network-unreachable")?;
    let url = Url::parse(&format!("http://127.0.0.1:{port}/f.bin"))?;
    let mut request = DownloadRequest::new(url, dir.to_path_buf());
    request.options.max_retries = 0;
    let run = download(
        request,
        Arc::new(Progress::default()),
        CancellationToken::new(),
    );
    let result = tokio::time::timeout(PATIENCE, run).await?;
    dir.remove()?;
    match result {
        Err(Error::Network(error)) => assert_eq!(error.failure(), NetworkFailure::Unreachable),
        other => return Err(format!("not a network error: {other:?}").into()),
    }
    Ok(())
}
