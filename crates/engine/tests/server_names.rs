//! The name a file gets from what a server sends: raw bytes in headers, other charsets, path parameters.

use std::io;
use std::sync::Arc;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use wirecatch_engine::{CancellationToken, DownloadRequest, Outcome, Progress, Url, download};
use wirecatch_test_support::ftp::{FtpConfig, FtpServer};
use wirecatch_test_support::http::{Config, TestServer};
use wirecatch_test_support::{PATIENCE, TestResult, private_dir, test_data};

const BODY: &[u8] = b"the file";

/// Answers `/start` with `first`, when given, and anything else with the file and the `extra` header bytes.
async fn answer(
    mut socket: TcpStream,
    extra: &'static [u8],
    first: &'static [u8],
) -> io::Result<()> {
    let mut head = Vec::new();
    let mut byte = [0u8; 1];
    while !head.ends_with(b"\r\n\r\n") && socket.read(&mut byte).await? == 1 {
        head.push(byte[0]);
    }
    if head.starts_with(b"GET /start ") && !first.is_empty() {
        return socket.write_all(first).await;
    }
    let start = format!(
        "HTTP/1.1 200 OK\r\nConnection: close\r\nContent-Length: {}\r\n",
        BODY.len()
    );
    socket.write_all(start.as_bytes()).await?;
    socket.write_all(extra).await?;
    socket.write_all(b"\r\n").await?;
    socket.write_all(BODY).await
}

async fn raw_server(extra: &'static [u8], first: &'static [u8]) -> TestResult<u16> {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let port = listener.local_addr()?.port();
    tokio::spawn(async move {
        while let Ok((socket, _)) = listener.accept().await {
            tokio::spawn(answer(socket, extra, first));
        }
    });
    Ok(port)
}

async fn saved_as(url: Url, tag: &str) -> TestResult<String> {
    let dir = private_dir(tag)?;
    let mut request = DownloadRequest::new(url, dir.to_path_buf());
    request.options.connections = 1;
    request.options.max_retries = 0;
    let run = download(
        request,
        Arc::new(Progress::default()),
        CancellationToken::new(),
    );
    let name = match tokio::time::timeout(PATIENCE, run).await?? {
        Outcome::Completed { path, .. } => path
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or("no name")?
            .to_owned(),
        other => return Err(format!("{other:?}").into()),
    };
    dir.remove()?;
    Ok(name)
}

async fn name_from_header(extra: &'static [u8], tag: &str) -> TestResult<String> {
    let port = raw_server(extra, b"").await?;
    saved_as(
        Url::parse(&format!("http://127.0.0.1:{port}/get.php"))?,
        tag,
    )
    .await
}

async fn name_from_disposition(value: &str, tag: &str) -> TestResult<String> {
    let server = TestServer::start(
        test_data(1024),
        Config {
            content_disposition: Some(value.into()),
            ..Config::default()
        },
    )
    .await;
    saved_as(server.url("get"), tag).await
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_raw_byte_in_the_plain_name_does_not_lose_the_encoded_name() -> TestResult {
    let name = name_from_header(
        b"Content-Disposition: attachment; filename=\"r\xC3\xA9sum\xC3\xA9.pdf\"; filename*=UTF-8''r%C3%A9sum%C3%A9.pdf\r\n",
        "raw-plain-and-encoded",
    )
    .await?;
    assert_eq!(name, "résumé.pdf");
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_plain_name_in_raw_utf8_is_read() -> TestResult {
    let name = name_from_header(
        b"Content-Disposition: attachment; filename=\"caf\xC3\xA9.pdf\"\r\n",
        "raw-utf8",
    )
    .await?;
    assert_eq!(name, "café.pdf");
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_plain_name_in_raw_latin1_is_read() -> TestResult {
    let name = name_from_header(
        b"Content-Disposition: attachment; filename=\"caf\xE9.pdf\"\r\n",
        "raw-latin1",
    )
    .await?;
    assert_eq!(name, "café.pdf");
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_redirect_to_an_address_in_raw_utf8_is_followed() -> TestResult {
    let port = raw_server(
        b"",
        b"HTTP/1.1 302 Found\r\nLocation: /caf\xC3\xA9.zip\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
    )
    .await?;
    let url = Url::parse(&format!("http://127.0.0.1:{port}/start"))?;
    assert_eq!(saved_as(url, "raw-redirect").await?, "café.zip");
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_encoded_name_in_latin1_is_read_as_latin1() -> TestResult {
    let name = name_from_disposition(
        "attachment; filename=\"cafe.pdf\"; filename*=ISO-8859-1''caf%E9.pdf",
        "latin1-encoded",
    )
    .await?;
    assert_eq!(name, "café.pdf");
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_encoded_name_in_an_unknown_charset_gives_way_to_the_plain_name() -> TestResult {
    let name = name_from_disposition(
        "attachment; filename=\"cafe.pdf\"; filename*=KOI8-R''caf%C5.pdf",
        "unknown-charset",
    )
    .await?;
    assert_eq!(name, "cafe.pdf");
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_ftp_type_suffix_stays_out_of_the_name() -> TestResult {
    let server = FtpServer::start(FtpConfig {
        files: vec![("pub/file.bin".to_string(), Arc::new(test_data(1024)))],
        ..FtpConfig::default()
    })
    .await;
    let url = server.url("ftp", "", "pub/file.bin;type=i");
    assert_eq!(saved_as(url, "ftp-type").await?, "file.bin");
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn path_parameters_stay_out_of_a_name_from_a_web_link() -> TestResult {
    let server = TestServer::start(test_data(1024), Config::default()).await;
    let name = saved_as(server.url("a.zip;jsessionid=ABC123"), "web-parameters").await?;
    assert_eq!(name, "a.zip");
    Ok(())
}
