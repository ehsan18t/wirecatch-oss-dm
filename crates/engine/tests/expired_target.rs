//! A link still works, but the redirect target it gave has expired.

use std::io;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use wirecatch_engine::units::KIB_USIZE;
use wirecatch_engine::{
    Backoff, CancellationToken, DownloadRequest, Error, Outcome, Progress, Url, download,
};
use wirecatch_test_support::{PATIENCE, TestResult, private_dir, test_data};

const SIZE: usize = 256 * KIB_USIZE;

struct Host {
    expired: AtomicBool,
    link_visits: AtomicUsize,
    data: Vec<u8>,
    /// What `/new` serves: another version of the file when it differs from `data`.
    new_data: Vec<u8>,
    /// The status `/link` answers once the target has expired, instead of a redirect to `/new`.
    link_refuses: bool,
}

impl Host {
    fn new() -> Self {
        let data = test_data(SIZE);
        Self {
            expired: AtomicBool::new(false),
            link_visits: AtomicUsize::new(0),
            new_data: data.clone(),
            data,
            link_refuses: false,
        }
    }
}

async fn request_head(socket: &mut TcpStream) -> io::Result<String> {
    let mut head = Vec::new();
    let mut byte = [0u8; 1];
    while !head.ends_with(b"\r\n\r\n") && socket.read(&mut byte).await? == 1 {
        head.push(byte[0]);
    }
    Ok(String::from_utf8_lossy(&head).into_owned())
}

/// `/link` sends the client to `/old`, then to `/new` once `/old` has expired. `/old` breaks off its first answer halfway and refuses everything after that, as a signed address past its time does.
async fn answer(mut socket: TcpStream, host: Arc<Host>) -> io::Result<()> {
    let head = request_head(&mut socket).await?;
    let path = head
        .split_whitespace()
        .nth(1)
        .unwrap_or_default()
        .to_owned();
    let from: usize = head
        .lines()
        .find_map(|line| {
            line.to_ascii_lowercase()
                .strip_prefix("range: bytes=")
                .map(str::to_owned)
        })
        .and_then(|range| range.split('-').next().and_then(|n| n.parse().ok()))
        .unwrap_or(0);
    let refusal = "HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
    if path == "/link" {
        let visit = host.link_visits.fetch_add(1, Ordering::SeqCst);
        if visit > 0 && host.link_refuses {
            return socket.write_all(refusal.as_bytes()).await;
        }
        let to = if visit == 0 { "/old" } else { "/new" };
        let reply = format!(
            "HTTP/1.1 302 Found\r\nLocation: {to}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
        );
        return socket.write_all(reply.as_bytes()).await;
    }
    if path == "/old" && host.expired.load(Ordering::SeqCst) {
        return socket.write_all(refusal.as_bytes()).await;
    }
    let data = if path == "/new" {
        &host.new_data
    } else {
        &host.data
    };
    let reply = format!(
        "HTTP/1.1 206 Partial Content\r\nContent-Range: bytes {from}-{}/{}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        data.len() - 1,
        data.len(),
        data.len() - from
    );
    socket.write_all(reply.as_bytes()).await?;
    if path == "/old" {
        let half = from + (data.len() - from) / 2;
        socket.write_all(&data[from..half]).await?;
        socket.flush().await?;
        host.expired.store(true, Ordering::SeqCst);
        return Ok(());
    }
    socket.write_all(&data[from..]).await
}

async fn run(host: Arc<Host>, name: &str) -> TestResult<wirecatch_engine::Result<Outcome>> {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let port = listener.local_addr()?.port();
    let serving = host.clone();
    tokio::spawn(async move {
        while let Ok((socket, _)) = listener.accept().await {
            tokio::spawn(answer(socket, serving.clone()));
        }
    });
    let dir = private_dir(name)?;
    let url = Url::parse(&format!("http://127.0.0.1:{port}/link"))?;
    let mut request = DownloadRequest::new(url, dir.to_path_buf());
    request.options.connections = 1;
    request.options.max_retries = 2;
    request.options.retry_backoff =
        Backoff::new(Duration::from_millis(1), Duration::from_millis(1));
    let run = download(
        request,
        Arc::new(Progress::default()),
        CancellationToken::new(),
    );
    let ended = tokio::time::timeout(PATIENCE, run).await?;
    if let Ok(Outcome::Completed { path, .. }) = &ended {
        assert!(std::fs::read(path)? == host.data, "the bytes differ");
    }
    dir.remove()?;
    Ok(ended)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_download_survives_its_redirect_target_expiring() -> TestResult {
    let host = Arc::new(Host::new());
    let ended = run(host.clone(), "expired-target").await?;
    assert!(
        matches!(ended, Ok(Outcome::Completed { .. })),
        "the link still works, but: {ended:?}"
    );
    assert_eq!(host.link_visits.load(Ordering::SeqCst), 2);
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_new_target_with_another_version_of_the_file_is_refused() -> TestResult {
    let host = Arc::new(Host {
        new_data: test_data(SIZE + 1),
        ..Host::new()
    });
    let ended = run(host, "expired-target-changed").await?;
    assert!(
        matches!(ended, Err(Error::RemoteChanged)),
        "the file changed, but: {ended:?}"
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_link_that_refuses_too_fails_the_download() -> TestResult {
    let host = Arc::new(Host {
        link_refuses: true,
        ..Host::new()
    });
    let ended = run(host, "expired-target-link-refuses").await?;
    assert!(
        matches!(ended, Err(Error::HttpStatus { status: 403, .. })),
        "the link no longer works, but: {ended:?}"
    );
    Ok(())
}
