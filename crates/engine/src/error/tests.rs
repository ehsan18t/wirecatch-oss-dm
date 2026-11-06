//! Where an HTTP/2 exchange broke off, against a server written frame by frame: h2's errors carry no cause, so they are read whole.

use std::sync::Arc;

use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
use tokio::net::TcpListener;
use tokio_util::sync::CancellationToken;
use url::Url;
use wirecatch_test_support::{PATIENCE, TestCertificate, private_dir};

use super::*;
use crate::download::{DownloadRequest, download_with};
use crate::http::Clients;
use crate::progress::Progress;

const PREFACE_LEN: usize = 24;
const FRAME_HEAD_LEN: usize = 9;
const HEADERS: u8 = 1;
const RST_STREAM: u8 = 3;
const SETTINGS: u8 = 4;
const GOAWAY: u8 = 7;
const ACK: u8 = 1;
const END_HEADERS: u8 = 4;
/// `:status: 200`, entry 8 of HPACK's static table.
const STATUS_200: u8 = 0x88;
/// `:status: 200`, then `content-length: abc` as a literal whose name is static entry 28.
const BAD_LENGTH_HEADERS: [u8; 7] = [STATUS_200, 0x0f, 0x0d, 3, b'a', b'b', b'c'];
const INTERNAL_ERROR: [u8; 4] = [0, 0, 0, 2];
const STREAM: u32 = 1;

/// How the server ends the answer it began.
#[derive(Clone, Copy)]
enum End {
    ResetStream,
    /// A `GOAWAY` with an error that still lets the stream finish, then the connection closes.
    GoAway,
    CloseSocket,
    /// No end: the headers' `content-length: abc` is what is wrong.
    BadLength,
}

fn frame(kind: u8, flags: u8, stream: u32, payload: &[u8]) -> Vec<u8> {
    let len = u32::try_from(payload.len()).unwrap().to_be_bytes();
    let mut out = vec![len[1], len[2], len[3], kind, flags];
    out.extend_from_slice(&stream.to_be_bytes());
    out.extend_from_slice(payload);
    out
}

/// Answers the first request with the headers of a `200`, then ends it before any body byte as `end` says: a run with no retries left gives up on the first failure.
async fn serve(
    listener: TcpListener,
    acceptor: tokio_rustls::TlsAcceptor,
    end: End,
) -> std::io::Result<()> {
    let (tcp, _) = listener.accept().await?;
    let mut tls = acceptor.accept(tcp).await?;
    let mut preface = [0u8; PREFACE_LEN];
    tls.read_exact(&mut preface).await?;
    tls.write_all(&frame(SETTINGS, 0, 0, &[])).await?;
    loop {
        let mut head = [0u8; FRAME_HEAD_LEN];
        tls.read_exact(&mut head).await?;
        let len = usize::from(head[0]) << 16 | usize::from(head[1]) << 8 | usize::from(head[2]);
        let mut payload = vec![0u8; len];
        tls.read_exact(&mut payload).await?;
        match head[3] {
            SETTINGS if head[4] & ACK == 0 => tls.write_all(&frame(SETTINGS, ACK, 0, &[])).await?,
            HEADERS => break,
            _ => {}
        }
    }
    let headers: &[u8] = match end {
        End::BadLength => &BAD_LENGTH_HEADERS,
        _ => &[STATUS_200],
    };
    tls.write_all(&frame(HEADERS, END_HEADERS, STREAM, headers))
        .await?;
    match end {
        End::ResetStream => {
            tls.write_all(&frame(RST_STREAM, 0, STREAM, &INTERNAL_ERROR))
                .await?;
        }
        End::GoAway => {
            let mut payload = STREAM.to_be_bytes().to_vec();
            payload.extend_from_slice(&INTERNAL_ERROR);
            tls.write_all(&frame(GOAWAY, 0, 0, &payload)).await?;
        }
        End::BadLength | End::CloseSocket => {}
    }
    tls.flush().await?;
    if matches!(end, End::GoAway | End::CloseSocket) {
        return tls.get_mut().0.shutdown().await;
    }
    // Keep the connection until the client lets it go.
    let mut rest = Vec::new();
    let (Ok(_) | Err(_)) = tls.read_to_end(&mut rest).await;
    Ok(())
}

async fn failure_over_http2(end: End, name: &str) -> NetworkFailure {
    let cert = TestCertificate::new();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let acceptor = tokio_rustls::TlsAcceptor::from(Arc::new(cert.server_config(&[b"h2"])));
    let server = tokio::spawn(serve(listener, acceptor, end));
    let dir = private_dir(name).unwrap();
    let url = Url::parse(&format!("https://127.0.0.1:{port}/f.bin")).unwrap();
    let mut request = DownloadRequest::new(url, dir.path());
    request.options.max_retries = 0;
    let clients = Clients::with_roots(&request.options, cert.roots());
    let run = download_with(
        request,
        Arc::new(Progress::default()),
        CancellationToken::new(),
        clients,
    );
    let result = tokio::time::timeout(PATIENCE, run).await.unwrap();
    server.abort();
    dir.remove().unwrap();
    match result {
        Err(Error::Network(error)) => error.failure(),
        other => panic!("not a network error: {other:?}"),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_stream_the_server_resets_dropped() {
    let failure = failure_over_http2(End::ResetStream, "h2-reset").await;
    assert_eq!(failure, NetworkFailure::Dropped);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_connection_the_server_sends_away_dropped() {
    let failure = failure_over_http2(End::GoAway, "h2-goaway").await;
    assert_eq!(failure, NetworkFailure::Dropped);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_http2_connection_closed_mid_answer_dropped() {
    let failure = failure_over_http2(End::CloseSocket, "h2-closed").await;
    assert_eq!(failure, NetworkFailure::Dropped);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_size_that_is_not_a_number_could_not_be_read() {
    let failure = failure_over_http2(End::BadLength, "h2-bad-length").await;
    assert_eq!(failure, NetworkFailure::Unreadable);
}
