//! Acceptance 6 and 7: the protocol server refuses bad clients clearly without
//! crashing, and a settings change from any side reaches every client. Also the
//! extension's capture flow, with and without confirmation.

use std::collections::{HashMap, VecDeque};
use std::io;
use std::pin::Pin;
use std::task::{Context, Poll, ready};
use std::time::Duration;

use tokio::io::{AsyncRead, AsyncWrite, AsyncWriteExt, DuplexStream, ReadBuf};
use tokio::sync::oneshot;
use tokio::task::JoinHandle;
use wirecatch_core::{LinkFrom, Manager, serve_connection};
use wirecatch_protocol::app::{
    CaptureConfirmation, DownloadStatus, MediaDefaultsPatch, UiEvent, Update,
};
use wirecatch_protocol::extension::{
    AddAction, BrowserControlsPatch, CaptureSource, ExtensionSettingsPatch, Header, NewDownload,
    WithdrawOutcome, WithdrawRequest,
};
use wirecatch_protocol::frame::{self, MAX_FRAME_LEN};
use wirecatch_protocol::wire::{
    Call, ClientKind, ErrorCode, Event, Hello, Message, Outcome, ProtocolError, Reply,
    UNKNOWN_REQUEST_ID,
};
use wirecatch_protocol::{MIN_PROTOCOL_VERSION, PROTOCOL_VERSION, QuestionId};

use crate::common::{
    Config, Harness, PATIENCE, SilentServer, TestResult, TestServer, fast_server, test_data, tune,
};
use wirecatch_engine::units::KIB_USIZE;

/// A socket client talking to `serve_connection` over an in-memory stream.
struct Client {
    stream: DuplexStream,
    server: JoinHandle<std::io::Result<()>>,
    next_id: u64,
    events: VecDeque<Event>,
    /// Responses that arrived while waiting for another one: requests run side by
    /// side, so their answers may come in any order.
    answered: HashMap<u64, Outcome>,
}

impl Client {
    fn connect(manager: &Manager) -> Self {
        let (stream, server_side) = tokio::io::duplex(64 * KIB_USIZE);
        Self::serving(manager, stream, server_side)
    }

    /// A client on `stream`, served on `server_side`, the other end of it.
    fn serving<S>(manager: &Manager, stream: DuplexStream, server_side: S) -> Self
    where
        S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
    {
        let server = tokio::spawn(serve_connection(manager.clone(), server_side));
        Self {
            stream,
            server,
            next_id: 1,
            events: VecDeque::new(),
            answered: HashMap::new(),
        }
    }

    async fn connected(manager: &Manager) -> TestResult<Self> {
        let mut client = Self::connect(manager);
        let reply = client
            .call(hello(MIN_PROTOCOL_VERSION, Some(PROTOCOL_VERSION)))
            .await?;
        assert!(
            matches!(reply, Ok(Reply::Hello(ref r)) if r.protocol_version == PROTOCOL_VERSION),
            "{reply:?}"
        );
        Ok(client)
    }

    async fn send(&mut self, message: &Message) -> TestResult {
        frame::write_async(&mut self.stream, message).await?;
        Ok(())
    }

    /// Sends a frame holding `body` byte for byte, for JSON no message type produces.
    async fn send_raw(&mut self, body: &str) -> TestResult {
        self.stream.write_all(&framed(body.as_bytes())?).await?;
        Ok(())
    }

    /// The next message, or `None` once the server closed the connection.
    async fn recv(&mut self) -> TestResult<Option<Message>> {
        let read = tokio::time::timeout(PATIENCE, frame::read_async(&mut self.stream))
            .await
            .map_err(|_| "no answer from the server")?;
        Ok(read?)
    }

    /// Sends a call and returns its outcome, keeping events that arrive meanwhile.
    async fn call(&mut self, call: Call) -> TestResult<Result<Reply, ProtocolError>> {
        let id = self.request(call).await?;
        self.response(id).await
    }

    /// Sends a call without waiting for its answer, and returns its id.
    async fn request(&mut self, call: Call) -> TestResult<u64> {
        let id = self.next_id;
        self.next_id += 1;
        self.send(&Message::Request { id, call }).await?;
        Ok(id)
    }

    async fn response(&mut self, id: u64) -> TestResult<Result<Reply, ProtocolError>> {
        loop {
            if let Some(outcome) = self.answered.remove(&id) {
                return Ok(match outcome {
                    Outcome::Ok(reply) => Ok(reply),
                    Outcome::Err(e) => Err(e),
                });
            }
            match self.recv().await?.ok_or("connection closed")? {
                Message::Response { id: got, outcome } => {
                    let earlier = self.answered.insert(got, outcome);
                    assert!(earlier.is_none(), "two responses to request {got}");
                }
                Message::Event { event } => self.events.push_back(event),
                Message::Request { .. } => return Err("the server sent a request".into()),
            }
        }
    }

    async fn next_event(&mut self) -> TestResult<Event> {
        if let Some(event) = self.events.pop_front() {
            return Ok(event);
        }
        match self.recv().await?.ok_or("connection closed")? {
            Message::Event { event } => Ok(event),
            other => Err(format!("expected an event, got {other:?}").into()),
        }
    }

    /// Checks the server closed the connection cleanly, without panicking.
    async fn assert_closed(mut self) -> TestResult {
        assert_eq!(self.recv().await?, None, "connection still open");
        let served = tokio::time::timeout(PATIENCE, self.server)
            .await
            .map_err(|_| "server task still running")?;
        served??;
        Ok(())
    }
}

fn hello(protocol_version: u32, max_protocol_version: Option<u32>) -> Call {
    Call::Hello(Hello {
        client: ClientKind::Extension,
        client_version: "0.1.0".into(),
        protocol_version,
        max_protocol_version,
        browser: None,
        start_app: true,
    })
}

/// A capture whose handover id is derived from its link.
fn capture(url: String) -> NewDownload {
    NewDownload {
        handover_id: format!("handover-{url}"),
        url,
        referrer: Some("https://intranet.test/page".into()),
        page_url: Some("https://intranet.test/page".into()),
        frame_url: None,
        suggested_name: Some("report.pdf".into()),
        mime: None,
        size_hint: Some(1024),
        cookies: Some("session=abc".into()),
        user_agent: Some("TestBrowser/1".into()),
        extra_headers: vec![],
        source: CaptureSource::BrowserDownload,
        queue: None,
        media: None,
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn skipping_hello_is_refused_and_closed() -> TestResult {
    let h = Harness::new().await?;
    let mut client = Client::connect(&h.manager);
    let err = client.call(Call::PauseAll).await?.unwrap_err();
    assert_eq!(err.code, ErrorCode::InvalidRequest);
    assert!(
        err.message.contains("first message must be hello"),
        "{}",
        err.message
    );
    client.assert_closed().await?;
    Ok(())
}

/// Whatever the first request is, a call the app has or not, the client is told hello comes
/// first; the connection then closes, which an `unsupported_call` answer would deny.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_first_call_the_app_lacks_is_told_hello_comes_first() -> TestResult {
    let h = Harness::new().await?;
    for first in [
        r#"{"kind":"request","id":9,"call":{"method":"format_disk","params":{}}}"#,
        r#"{"kind":"request","id":9,"call":{"method":"add_download","params":{"url":3}}}"#,
    ] {
        let mut client = Client::connect(&h.manager);
        client.send_raw(first).await?;
        let err = client.response(9).await?.unwrap_err();
        assert_eq!(err.code, ErrorCode::InvalidRequest, "{first}");
        assert!(
            err.message.contains("first message must be hello"),
            "{first}: {}",
            err.message
        );
        client.assert_closed().await?;
    }
    Ok(())
}

/// A peer that floods the connection with calls the app lacks, as large as a frame may be or
/// nested deeper than any reader follows, gets an answer to each and leaves the connection
/// working.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_flood_of_calls_the_app_lacks_is_answered_and_survived() -> TestResult {
    let h = Harness::new().await?;
    let mut client = Client::connected(&h.manager).await?;
    let nested = format!("{}{}", "[".repeat(5000), "]".repeat(5000));
    for id in 1..=64_u64 {
        let prefix =
            format!(r#"{{"kind":"request","id":{id},"call":{{"method":"format_disk","params":"#);
        let suffix = "}}";
        let params = if id % 8 == 0 {
            nested.clone()
        } else {
            let quotes = 2;
            let filler = "x".repeat(MAX_FRAME_LEN - prefix.len() - suffix.len() - quotes);
            format!(r#""{filler}""#)
        };
        let body = format!("{prefix}{params}{suffix}");
        assert!(body.len() <= MAX_FRAME_LEN, "{}", body.len());
        client.send_raw(&body).await?;
        let err = client.response(id).await?.unwrap_err();
        assert_eq!(err.code, ErrorCode::UnsupportedCall, "{id}");
    }
    assert!(client.call(Call::ListQueues).await?.is_ok());
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_client_sharing_no_protocol_version_is_refused_and_closed() -> TestResult {
    let h = Harness::new().await?;
    let too_new = hello(PROTOCOL_VERSION + 1, Some(PROTOCOL_VERSION + 3));
    let too_old = hello(0, Some(MIN_PROTOCOL_VERSION - 1));
    for (call, update) in [
        (too_new, "the Wirecatch app"),
        (too_old, "the browser extension"),
    ] {
        let mut client = Client::connect(&h.manager);
        let err = client.call(call).await?.unwrap_err();
        assert_eq!(err.code, ErrorCode::UnsupportedVersion);
        assert!(
            err.message.contains(&format!("Update {update}")),
            "{}",
            err.message
        );
        client.assert_closed().await?;
    }
    Ok(())
}

/// An extension from before version ranges sends only `protocol_version: 1` and accepts only
/// 1 back. The app answers it in version 1, with version 1's error codes.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_client_from_before_version_ranges_is_answered_in_version_1() -> TestResult {
    let h = Harness::new().await?;
    let mut client = Client::connect(&h.manager);
    client
        .send_raw(r#"{"kind":"request","id":1,"call":{"method":"hello","params":{"client":"extension","client_version":"0.1.0","protocol_version":1,"browser":null}}}"#)
        .await?;
    let reply = client.response(1).await?;
    assert!(
        matches!(reply, Ok(Reply::Hello(ref r)) if r.protocol_version == 1),
        "{reply:?}"
    );

    client
        .send_raw(r#"{"kind":"request","id":2,"call":{"method":"format_disk"}}"#)
        .await?;
    assert_eq!(
        client.response(2).await?.unwrap_err().code,
        ErrorCode::InvalidRequest
    );
    assert!(client.call(Call::ListQueues).await?.is_ok());
    Ok(())
}

/// A client newer than the app, still speaking a version the app has, is answered in the
/// newest version both speak, and fields only it knows are passed over.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_newer_client_is_answered_in_the_newest_version_both_speak() -> TestResult {
    let h = Harness::new().await?;
    let mut client = Client::connect(&h.manager);
    let newest = PROTOCOL_VERSION + 7;
    client
        .send_raw(&format!(r#"{{"kind":"request","id":1,"call":{{"method":"hello","params":{{"client":"extension","client_version":"9.0.0","protocol_version":{MIN_PROTOCOL_VERSION},"max_protocol_version":{newest},"browser":null,"start_app":false,"capabilities":["teleport"]}}}},"trace":"abc"}}"#))
        .await?;
    let reply = client.response(1).await?;
    assert!(
        matches!(reply, Ok(Reply::Hello(ref r)) if r.protocol_version == PROTOCOL_VERSION),
        "{reply:?}"
    );
    client
        .send_raw(r#"{"kind":"request","id":2,"call":{"method":"list_queues","priority":"high"},"trace":"def"}"#)
        .await?;
    assert!(matches!(client.response(2).await?, Ok(Reply::Queues(_))));
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_hello_with_impossible_versions_is_refused_and_closed() -> TestResult {
    let h = Harness::new().await?;
    let versions = [
        r#""protocol_version":2,"max_protocol_version":1"#,
        r#""protocol_version":-1"#,
        r#""protocol_version":4294967296"#,
        r#""protocol_version":"2""#,
        r#""protocol_version":1,"max_protocol_version":1e300"#,
        r#""max_protocol_version":2"#,
    ];
    for version in versions {
        let mut client = Client::connect(&h.manager);
        client
            .send_raw(&format!(r#"{{"kind":"request","id":1,"call":{{"method":"hello","params":{{"client":"extension","client_version":"1",{version},"browser":null}}}}}}"#))
            .await?;
        let err = client.response(1).await?.unwrap_err();
        assert_eq!(err.code, ErrorCode::InvalidRequest, "{version}");
        client.assert_closed().await?;
    }

    // The widest range there is still agrees on this app's newest version.
    let mut client = Client::connect(&h.manager);
    let reply = client.call(hello(0, Some(u32::MAX))).await?;
    assert!(
        matches!(reply, Ok(Reply::Hello(ref r)) if r.protocol_version == PROTOCOL_VERSION),
        "{reply:?}"
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn bad_messages_get_errors_and_the_connection_survives() -> TestResult {
    let h = Harness::new().await?;
    let mut client = Client::connected(&h.manager).await?;

    // A method the app does not have is unsupported, not unreadable.
    client
        .send_raw(
            r#"{"kind":"request","id":41,"call":{"method":"format_disk","params":{"drive":"C"}}}"#,
        )
        .await?;
    let err = client.response(41).await?.unwrap_err();
    assert_eq!(err.code, ErrorCode::UnsupportedCall);

    // A call the app has, with parameters it cannot read, is invalid.
    client
        .send_raw(
            r#"{"kind":"request","id":42,"call":{"method":"add_download","params":{"url":3}}}"#,
        )
        .await?;
    let err = client.response(42).await?.unwrap_err();
    assert_eq!(err.code, ErrorCode::InvalidRequest);

    // Garbage has no id at all.
    client
        .stream
        .write_all(&framed(b"\x00\xffnot json")?)
        .await
        .unwrap();
    let err = client.response(UNKNOWN_REQUEST_ID).await?.unwrap_err();
    assert_eq!(err.code, ErrorCode::InvalidRequest);

    // An oversized frame is skipped without being stored.
    let len = MAX_FRAME_LEN + 1;
    client
        .stream
        // Over the limit, so `length_prefix` rightly refuses to write it.
        .write_all(&u32::try_from(len).unwrap().to_le_bytes())
        .await
        .unwrap();
    let filler = vec![b' '; 64 * KIB_USIZE];
    let mut left = len;
    while left > 0 {
        let n = left.min(filler.len());
        client.stream.write_all(&filler[..n]).await.unwrap();
        left -= n;
    }
    let err = client.response(UNKNOWN_REQUEST_ID).await?.unwrap_err();
    assert_eq!(err.code, ErrorCode::InvalidRequest);
    assert!(err.message.contains("limit"), "{}", err.message);

    // A client may only send requests.
    client
        .send(&Message::Event {
            event: Event::QueuesChanged(vec![]),
        })
        .await?;
    assert!(client.response(UNKNOWN_REQUEST_ID).await?.is_err());

    // After all that, the connection still works.
    let reply = client.call(Call::ListQueues).await?.unwrap();
    assert!(matches!(reply, Reply::Queues(q) if q.len() == 1 && q[0].is_default));
    drop(client.stream);
    client.server.await.unwrap().unwrap();
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_client_vanishing_mid_frame_does_not_hurt_the_server() -> TestResult {
    let h = Harness::new().await?;
    let mut client = Client::connected(&h.manager).await?;
    client
        .stream
        .write_all(&[200, 0, 0, 0, b'{'])
        .await
        .unwrap();
    drop(client.stream);
    client.server.await.unwrap().unwrap();
    // Other clients are unaffected.
    let mut other = Client::connected(&h.manager).await?;
    assert!(other.call(Call::GetExtensionSettings).await?.is_ok());
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn settings_changes_reach_every_client() -> TestResult {
    let h = Harness::new().await?;
    let mut ui = h.manager.subscribe();
    let mut chrome = Client::connected(&h.manager).await?;
    let mut firefox = Client::connected(&h.manager).await?;

    // From one client to the other client and to the UI.
    let patch = ExtensionSettingsPatch {
        ask_before_download: Some(true),
        ignored_sites: Some(vec!["Example.COM".into()]),
        ..Default::default()
    };
    let reply = chrome
        .call(Call::UpdateExtensionSettings(patch))
        .await?
        .unwrap();
    let Reply::ExtensionSettings(saved) = reply else {
        panic!("unexpected reply {reply:?}");
    };
    assert_eq!(saved.ignored_sites, vec!["example.com"]);
    assert_eq!(
        firefox.next_event().await?,
        Event::ExtensionSettingsChanged(saved.clone())
    );
    assert!(matches!(
        ui.recv().await.unwrap(),
        UiEvent::SettingsChanged(s) if s.extension == saved
    ));

    // From the UI to every client.
    tune(&h.manager, |s| s.extension.capture_enabled = false)?;
    for client in [&mut chrome, &mut firefox] {
        loop {
            match client.next_event().await? {
                Event::ExtensionSettingsChanged(s) if !s.capture_enabled => break,
                Event::ExtensionSettingsChanged(_) => {}
                other @ (Event::QueuesChanged(_)
                | Event::BrowserControlsChanged(_)
                | Event::NoteRequested(_)) => {
                    panic!("unexpected event {other:?}")
                }
            }
        }
    }

    // Queue changes are pushed too.
    h.manager.create_queue("Later").unwrap();
    assert!(matches!(
        firefox.next_event().await?,
        Event::QueuesChanged(q) if q.iter().any(|q| q.name == "Later")
    ));
    Ok(())
}

/// The server's end of a stream whose first flush waits for `release`: what the server
/// wrote is already readable, but the server is held right after writing it.
struct FirstFlushHeld {
    stream: DuplexStream,
    release: Option<oneshot::Receiver<()>>,
}

impl AsyncRead for FirstFlushHeld {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        Pin::new(&mut self.stream).poll_read(cx, buf)
    }
}

impl AsyncWrite for FirstFlushHeld {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        Pin::new(&mut self.stream).poll_write(cx, buf)
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        if let Some(release) = &mut self.release {
            ready!(Pin::new(release).poll(cx)).map_err(io::Error::other)?;
            self.release = None;
        }
        Pin::new(&mut self.stream).poll_flush(cx)
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.stream).poll_shutdown(cx)
    }
}

// A client may act as soon as its hello is answered, before the server's task runs
// again; a change made in that moment still reaches it.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_change_made_as_the_hello_is_answered_reaches_the_client() -> TestResult {
    let h = Harness::new().await?;
    let (stream, server_side) = tokio::io::duplex(64 * KIB_USIZE);
    let (release, held) = oneshot::channel();
    let server_side = FirstFlushHeld {
        stream: server_side,
        release: Some(held),
    };
    let mut client = Client::serving(&h.manager, stream, server_side);
    let reply = client
        .call(hello(MIN_PROTOCOL_VERSION, Some(PROTOCOL_VERSION)))
        .await?;
    assert!(matches!(reply, Ok(Reply::Hello(_))), "{reply:?}");

    tune(&h.manager, |s| s.extension.capture_enabled = false)?;
    release.send(()).map_err(|()| "the server stopped")?;
    assert!(matches!(
        client.next_event().await?,
        Event::ExtensionSettingsChanged(s) if !s.capture_enabled
    ));
    Ok(())
}

// The extension's "Open Wirecatch" starts the app in the tray; this call brings its window up.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_client_can_ask_for_the_window() -> TestResult {
    let h = Harness::new().await?;
    let mut ui = h.manager.subscribe();
    let mut client = Client::connected(&h.manager).await?;
    assert_eq!(client.call(Call::ShowWindow).await?.unwrap(), Reply::Done);
    while !matches!(ui.recv().await.unwrap(), UiEvent::WindowRequested) {}
    assert_eq!(
        client.call(Call::ShowBrowserSettings).await?.unwrap(),
        Reply::Done
    );
    while !matches!(ui.recv().await.unwrap(), UiEvent::BrowserSettingsRequested) {}
    Ok(())
}

/// D65: the app asks every browser to show a note, and hears which showed it and what was pressed.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_note_reaches_the_browsers_and_their_answers_reach_the_app() -> TestResult {
    use wirecatch_protocol::extension::{BrowserNote, NoteId, NoteKind, NotePressed, NoteShown};

    let h = Harness::new().await?;
    let mut ui = h.manager.subscribe();
    let mut client = Client::connected(&h.manager).await?;
    let note = BrowserNote {
        id: NoteId(4),
        kind: NoteKind::Finished,
        title: "Download finished".into(),
        body: "a.zip".into(),
        buttons: vec!["Show in folder".into()],
        stays_ms: Some(8000),
    };
    assert!(
        h.manager.request_note(note.clone()) > 0,
        "the client listens"
    );
    loop {
        if let Event::NoteRequested(asked) = client.next_event().await? {
            assert_eq!(asked, note);
            break;
        }
    }
    let shown = client
        .call(Call::NoteShown(NoteShown { id: NoteId(4) }))
        .await?;
    assert_eq!(shown.unwrap(), Reply::Done);
    while !matches!(ui.recv().await.unwrap(), UiEvent::NoteShown(NoteId(4))) {}
    let pressed = NotePressed {
        id: NoteId(4),
        button: 0,
    };
    let reply = client.call(Call::NotePressed(pressed.clone())).await?;
    assert_eq!(reply.unwrap(), Reply::Done);
    while !matches!(ui.recv().await.unwrap(), UiEvent::NotePressed(p) if p == pressed) {}
    Ok(())
}

/// The app settings the popup shows (D61) are the app's: a change from one browser is stored
/// and reaches the other browser and the UI, and a change in the app reaches every browser.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn browser_controls_changes_reach_every_client() -> TestResult {
    let h = Harness::new().await?;
    let mut chrome = Client::connected(&h.manager).await?;
    let mut firefox = Client::connected(&h.manager).await?;
    assert_eq!(
        chrome.call(Call::GetBrowserControls).await?.unwrap(),
        Reply::BrowserControls(h.manager.get_settings().browser_controls())
    );

    let patch = BrowserControlsPatch {
        global_speed_limit: Update::Set(Some(512 * 1024)),
        max_concurrent_downloads: Some(2),
        media: Some(MediaDefaultsPatch {
            ask_each_time: Some(false),
            ..Default::default()
        }),
    };
    let reply = chrome
        .call(Call::UpdateBrowserControls(patch))
        .await?
        .unwrap();
    let Reply::BrowserControls(saved) = reply else {
        panic!("unexpected reply {reply:?}");
    };
    assert_eq!(saved.global_speed_limit, Some(512 * 1024));
    assert_eq!(saved.max_concurrent_downloads, 2);
    assert!(!saved.media.ask_each_time);
    assert_eq!(h.manager.get_settings().browser_controls(), saved);
    assert_eq!(
        firefox.next_event().await?,
        Event::BrowserControlsChanged(saved.clone())
    );

    tune(&h.manager, |s| s.global_speed_limit = None)?;
    // The browser that made the first change is told of it too, before this one.
    for client in [&mut chrome, &mut firefox] {
        loop {
            match client.next_event().await? {
                Event::BrowserControlsChanged(c) if c.global_speed_limit.is_none() => {
                    assert_eq!(c.max_concurrent_downloads, 2);
                    break;
                }
                Event::BrowserControlsChanged(c) => assert_eq!(c, saved),
                other => panic!("unexpected event {other:?}"),
            }
        }
    }
    Ok(())
}

/// A change to a setting the popup does not show sends no `browser_controls_changed`.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn other_settings_send_no_browser_controls_event() -> TestResult {
    let h = Harness::new().await?;
    let mut client = Client::connected(&h.manager).await?;
    tune(&h.manager, |s| s.close_to_tray = !s.close_to_tray)?;
    tune(&h.manager, |s| s.max_concurrent_downloads = 2)?;
    assert!(matches!(
        client.next_event().await?,
        Event::BrowserControlsChanged(c) if c.max_concurrent_downloads == 2
    ));
    Ok(())
}

/// A value the app's own settings refuse is refused from the popup too, and nothing changes.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn browser_controls_out_of_range_are_refused() -> TestResult {
    let h = Harness::new().await?;
    let before = h.manager.get_settings();
    let mut client = Client::connected(&h.manager).await?;
    let patch = BrowserControlsPatch {
        max_concurrent_downloads: Some(0),
        ..Default::default()
    };
    let error = client
        .call(Call::UpdateBrowserControls(patch))
        .await?
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::InvalidRequest);
    assert_eq!(h.manager.get_settings(), before);
    Ok(())
}

/// A client on version 1 has neither the new calls nor the new event (D32.3).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_version_1_client_never_sees_the_browser_controls() -> TestResult {
    let h = Harness::new().await?;
    let mut client = Client::connect(&h.manager);
    let reply = client.call(hello(1, Some(1))).await?;
    assert!(
        matches!(reply, Ok(Reply::Hello(ref r)) if r.protocol_version == 1),
        "{reply:?}"
    );
    for call in [Call::GetBrowserControls, Call::ShowBrowserSettings] {
        assert_eq!(
            client.call(call).await?.unwrap_err().code,
            ErrorCode::InvalidRequest
        );
    }
    tune(&h.manager, |s| s.global_speed_limit = Some(1024))?;
    tune(&h.manager, |s| s.extension.capture_enabled = false)?;
    assert!(matches!(
        client.next_event().await?,
        Event::ExtensionSettingsChanged(s) if !s.capture_enabled
    ));
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_capture_starts_at_once_and_shows_a_toast() -> TestResult {
    let server = fast_server(1024).await?;
    let h = Harness::new().await?;
    let mut ui = h.manager.subscribe();
    let mut client = Client::connected(&h.manager).await?;

    let reply = client
        .call(Call::AddDownload(capture(
            server.url("get?id=7").to_string(),
        )))
        .await?
        .unwrap();
    let Reply::DownloadAdded(added) = reply else {
        panic!("unexpected reply {reply:?}");
    };
    assert_eq!(added.action, AddAction::Started);
    let id = added.id.unwrap();
    let toast = loop {
        if let UiEvent::Captured(row) = ui.recv().await.unwrap() {
            break row;
        }
    };
    assert_eq!((toast.id, toast.file_name.as_str()), (id, "report.pdf"));
    assert!(toast.path.ends_with("report.pdf"));

    let done = h.wait_status(id, DownloadStatus::Completed).await;
    assert!(std::path::Path::new(&done.path).starts_with(h.folders.downloads.join("Documents")));
    // The browser's cookies went along with the request.
    assert!(
        server
            .stats
            .header_values("cookie")
            .contains(&"session=abc".to_string())
    );

    // Once finished, the cookies are no longer kept.
    h.manager.shutdown().await;
    let db = h
        .root
        .path()
        .join("data")
        .join(wirecatch_core::DATABASE_FILE);
    let db = rusqlite::Connection::open(db).unwrap();
    let origin: String = db
        .query_row("SELECT origin FROM downloads WHERE id = ?1", [id.0], |r| {
            r.get(0)
        })
        .unwrap();
    assert!(!origin.contains("session=abc"), "{origin}");
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ask_before_download_waits_for_the_user() -> TestResult {
    let server = fast_server(1024).await?;
    let h = Harness::new().await?;
    tune(&h.manager, |s| s.extension.ask_before_download = true)?;
    let mut ui = h.manager.subscribe();
    let mut client = Client::connected(&h.manager).await?;

    let ask = |name: &str| capture(server.url(name).to_string());
    let reply = client.call(Call::AddDownload(ask("a"))).await?.unwrap();
    assert!(matches!(
        reply,
        Reply::DownloadAdded(a) if a.id.is_none() && a.action == AddAction::AwaitingConfirmation
    ));
    let proposal = loop {
        if let UiEvent::ConfirmationRequested(p) = ui.recv().await.unwrap() {
            break p;
        }
    };
    assert_eq!(proposal.file_name, "report.pdf");
    assert_eq!(proposal.size, Some(1024));
    assert_eq!(
        proposal.page_url.as_deref(),
        Some("https://intranet.test/page")
    );
    assert_eq!(h.manager.pending_confirmations(), vec![proposal.clone()]);
    assert_eq!(
        h.manager.list_downloads(),
        [] as [wirecatch_protocol::app::DownloadSummary; 0]
    );

    // The user picks another name and folder.
    let folder = h.folders.home.join("Chosen");
    h.manager
        .confirm_capture(
            proposal.id,
            CaptureConfirmation {
                folder: Some(folder.to_string_lossy().into_owned()),
                file_name: Some("mine.pdf".into()),
                queue: None,
                later: false,
            },
        )
        .await
        .unwrap();
    let row = h.manager.list_downloads().remove(0);
    assert_eq!(std::path::Path::new(&row.path), folder.join("mine.pdf"));
    h.wait_status(row.id, DownloadStatus::Completed).await;
    assert_eq!(
        h.manager.pending_confirmations(),
        [] as [wirecatch_protocol::app::PendingCapture; 0]
    );

    // A rejected capture downloads nothing.
    client.call(Call::AddDownload(ask("b"))).await?.unwrap();
    let second = h.manager.pending_confirmations().remove(0);
    h.manager.reject_capture(second.id).unwrap();
    assert_eq!(
        h.manager.pending_confirmations(),
        [] as [wirecatch_protocol::app::PendingCapture; 0]
    );
    assert_eq!(h.manager.list_downloads().len(), 1);
    assert!(h.manager.reject_capture(second.id).is_err());
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_repeated_handover_adds_nothing() -> TestResult {
    let server = fast_server(1024).await?;
    let h = Harness::new().await?;
    let mut client = Client::connected(&h.manager).await?;

    let first = client
        .call(Call::AddDownload(capture(server.url("x.pdf").to_string())))
        .await?
        .unwrap();
    let again = client
        .call(Call::AddDownload(capture(server.url("x.pdf").to_string())))
        .await?
        .unwrap();
    assert_eq!(first, again);

    // Two copies racing each other still make one download.
    let racing = capture(server.url("y.pdf").to_string());
    let (a, b) = tokio::join!(
        h.manager.add_capture(racing.clone()),
        h.manager.add_capture(racing)
    );
    assert_eq!(a.unwrap(), b.unwrap());
    assert_eq!(h.manager.list_downloads().len(), 2);

    // The same holds while confirmations are asked for.
    tune(&h.manager, |s| s.extension.ask_before_download = true)?;
    let asked = capture(server.url("z.pdf").to_string());
    let first = h.manager.add_capture(asked.clone()).await.unwrap();
    let again = h.manager.add_capture(asked).await.unwrap();
    assert_eq!(
        (first.action, &first),
        (AddAction::AwaitingConfirmation, &again)
    );
    assert_eq!(h.manager.pending_confirmations().len(), 1);
    Ok(())
}

/// Two tabs capturing the same link share one confirmation, but each handover is
/// still its own: one tab withdrawing must not take the other's away, and once
/// the user has answered, every tab hears the same answer.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_shared_confirmation_tracks_every_handover() -> TestResult {
    let server = fast_server(1024).await?;
    let h = Harness::new().await?;
    tune(&h.manager, |s| s.extension.ask_before_download = true)?;
    let from_tab = |name: &str, tab: &str| {
        let mut capture = capture(server.url(name).to_string());
        capture.handover_id = tab.into();
        capture
    };

    // Withdrawn by one tab, then the other: gone only after the last one.
    for tab in ["a-1", "a-2"] {
        h.manager.add_capture(from_tab("a.pdf", tab)).await.unwrap();
    }
    assert_eq!(h.manager.pending_confirmations().len(), 1);
    assert_eq!(
        h.manager.withdraw_capture("a-1").unwrap(),
        WithdrawOutcome::Withdrawn
    );
    assert_eq!(
        h.manager.pending_confirmations().len(),
        1,
        "one tab withdrew the other tab's download"
    );
    assert_eq!(
        h.manager.withdraw_capture("a-2").unwrap(),
        WithdrawOutcome::Withdrawn
    );
    assert_eq!(
        h.manager.pending_confirmations(),
        [] as [wirecatch_protocol::app::PendingCapture; 0]
    );

    // Confirmed: it is the app's, for every tab, whether the server answered yet or not.
    for tab in ["b-1", "b-2"] {
        h.manager.add_capture(from_tab("b.pdf", tab)).await.unwrap();
    }
    let proposal = h.manager.pending_confirmations().remove(0);
    let answer = CaptureConfirmation {
        folder: None,
        file_name: None,
        queue: None,
        later: false,
    };
    h.manager
        .confirm_capture(proposal.id, answer.clone())
        .await
        .unwrap();
    for tab in ["b-1", "b-2"] {
        assert_eq!(
            h.manager.withdraw_capture(tab).unwrap(),
            WithdrawOutcome::AlreadyStarted,
            "{tab}"
        );
    }
    assert_eq!(h.manager.list_downloads().len(), 1);

    // Rejected: nothing was taken over, for every tab.
    for tab in ["c-1", "c-2"] {
        h.manager.add_capture(from_tab("c.pdf", tab)).await.unwrap();
    }
    let proposal = h.manager.pending_confirmations().remove(0);
    h.manager.reject_capture(proposal.id).unwrap();
    for tab in ["c-1", "c-2"] {
        assert_eq!(
            h.manager.withdraw_capture(tab).unwrap(),
            WithdrawOutcome::NotFound,
            "{tab}"
        );
    }
    assert_eq!(h.manager.list_downloads().len(), 1);
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn confirmations_are_deduplicated_and_capped() -> TestResult {
    let h = Harness::new().await?;
    tune(&h.manager, |s| s.extension.ask_before_download = true)?;
    let mut first = capture("https://example.com/same.zip".into());
    let mut second = first.clone();
    first.handover_id = "tab-1".into();
    second.handover_id = "tab-2".into();
    h.manager.add_capture(first).await.unwrap();
    let again = h.manager.add_capture(second).await.unwrap();
    assert_eq!(again.action, AddAction::AwaitingConfirmation);
    assert_eq!(h.manager.pending_confirmations().len(), 1);

    for i in 1..100 {
        h.manager
            .add_capture(capture(format!("https://example.com/{i}.zip")))
            .await
            .unwrap();
    }
    let err = h
        .manager
        .add_capture(capture("https://example.com/one-too-many.zip".into()))
        .await
        .unwrap_err();
    assert!(
        err.to_string().contains("Answer some of them first"),
        "{err}"
    );
    assert_eq!(h.manager.pending_confirmations().len(), 100);
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn withdraw_drops_only_what_the_server_has_not_answered() -> TestResult {
    let server = fast_server(1024).await?;
    let h = Harness::new().await?;
    let mut client = Client::connected(&h.manager).await?;
    let withdraw = |capture: &NewDownload| {
        Call::WithdrawDownload(WithdrawRequest {
            handover_id: capture.handover_id.clone(),
        })
    };
    let outcome = |reply| match reply {
        Reply::Withdrawn(outcome) => outcome,
        other => panic!("unexpected reply {other:?}"),
    };

    let unknown = capture(server.url("never-sent").to_string());
    let reply = client.call(withdraw(&unknown)).await?.unwrap();
    assert_eq!(outcome(reply), WithdrawOutcome::NotFound);

    // A capture waiting for its queue's schedule has not reached the server.
    let mut queue = h.manager.list_queues().remove(0);
    queue.schedule = Some(wirecatch_protocol::app::Schedule {
        start: wirecatch_protocol::app::TimeOfDay { hour: 2, minute: 0 },
        stop: Some(wirecatch_protocol::app::TimeOfDay { hour: 3, minute: 0 }),
        days: vec![],
    });
    h.manager.update_queue(queue.clone()).unwrap();
    let waiting = capture(server.url("waiting.bin").to_string());
    let added = h.manager.add_capture(waiting.clone()).await.unwrap();
    assert_eq!(added.action, AddAction::Queued);
    let reply = client.call(withdraw(&waiting)).await?.unwrap();
    assert_eq!(outcome(reply), WithdrawOutcome::Withdrawn);
    assert_eq!(
        h.manager.list_downloads(),
        [] as [wirecatch_protocol::app::DownloadSummary; 0]
    );
    assert_eq!(
        server
            .stats
            .requests
            .load(std::sync::atomic::Ordering::SeqCst),
        0
    );
    let reply = client.call(withdraw(&waiting)).await?.unwrap();
    assert_eq!(outcome(reply), WithdrawOutcome::NotFound);

    // A finished one is the app's now.
    queue.schedule = None;
    h.manager.update_queue(queue).unwrap();
    let done = capture(server.url("done.bin").to_string());
    let id = h
        .manager
        .add_capture(done.clone())
        .await
        .unwrap()
        .id
        .unwrap();
    h.wait_status(id, DownloadStatus::Completed).await;
    let reply = client.call(withdraw(&done)).await?.unwrap();
    assert_eq!(outcome(reply), WithdrawOutcome::AlreadyStarted);
    assert_eq!(h.manager.list_downloads().len(), 1);

    // A capture waiting for the user's confirmation is dropped too.
    tune(&h.manager, |s| s.extension.ask_before_download = true)?;
    let asked = capture(server.url("asked.bin").to_string());
    h.manager.add_capture(asked.clone()).await.unwrap();
    let reply = client.call(withdraw(&asked)).await?.unwrap();
    assert_eq!(outcome(reply), WithdrawOutcome::Withdrawn);
    assert_eq!(
        h.manager.pending_confirmations(),
        [] as [wirecatch_protocol::app::PendingCapture; 0]
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ignored_site_edits_from_two_browsers_merge() -> TestResult {
    let h = Harness::new().await?;
    let mut chrome = Client::connected(&h.manager).await?;
    let mut firefox = Client::connected(&h.manager).await?;
    let edit = |add: &[&str], remove: &[&str]| {
        let list = |l: &[&str]| Some(l.iter().map(std::string::ToString::to_string).collect());
        Call::UpdateExtensionSettings(ExtensionSettingsPatch {
            add_ignored_sites: list(add),
            remove_ignored_sites: list(remove),
            ..Default::default()
        })
    };
    chrome.call(edit(&["A.example"], &[])).await?.unwrap();
    firefox.call(edit(&["b.example"], &[])).await?.unwrap();
    let reply = chrome.call(edit(&[], &["a.EXAMPLE/"])).await?.unwrap();
    let Reply::ExtensionSettings(saved) = reply else {
        panic!("unexpected reply {reply:?}");
    };
    assert_eq!(saved.ignored_sites, vec!["b.example"]);
    Ok(())
}

async fn capture_server(len: usize, change: impl FnOnce(&mut Config)) -> TestServer {
    let mut config = Config {
        ranges: true,
        ..Config::default()
    };
    change(&mut config);
    TestServer::start(test_data(len), config).await
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn started_means_the_server_answered() -> TestResult {
    let delay = Duration::from_millis(400);
    let server = capture_server(1024, |c| c.answer_delay = Some(delay)).await;
    let h = Harness::new().await?;
    let start = std::time::Instant::now();
    let added = h
        .manager
        .add_capture(capture(server.url("report.pdf").to_string()))
        .await
        .unwrap();
    assert_eq!(added.action, AddAction::Started);
    assert!(
        start.elapsed() >= delay,
        "replied before the server answered"
    );
    let row = h.row(added.id.unwrap());
    assert!(
        matches!(
            row.status,
            DownloadStatus::Downloading | DownloadStatus::Connecting | DownloadStatus::Completed
        ) && row.path.ends_with("report.pdf")
    );
    Ok(())
}

/// `url` with its host written as a name that resolves to this computer, as a
/// website's name can when its owner makes it answer with a local address.
fn by_name(mut url: wirecatch_engine::Url) -> TestResult<String> {
    url.set_host(Some("localhost"))?;
    Ok(url.to_string())
}

/// A capture of `url` from a page at `page`.
fn capture_from(page: &str, url: String) -> NewDownload {
    NewDownload {
        referrer: Some(page.into()),
        page_url: Some(page.into()),
        frame_url: None,
        ..capture(url)
    }
}

/// A capture of `url` from a page on the internet.
fn from_website(url: String) -> NewDownload {
    capture_from("https://example.com/page", url)
}

/// Captures `capture` and expects it left to the browser for leading into
/// the local network, with nothing added or asked.
async fn left_to_the_browser(h: &Harness, capture: NewDownload) -> TestResult {
    let before = h.manager.list_downloads().len();
    let result = h.manager.add_capture(capture).await;
    let Err(wirecatch_core::CoreError::NotStarted(reason)) = result else {
        panic!("{result:?}");
    };
    assert!(reason.contains("your own network"), "{reason}");
    assert_eq!(h.manager.list_downloads().len(), before);
    assert_eq!(h.manager.pending_confirmations().len(), 0);
    Ok(())
}

/// A website's link into the local network, by a name that resolves there or
/// by an address, is declined before the app answers, so the browser keeps
/// its download: also when the window would ask first or the queue is
/// stopped, where the browser lets go of its copy as soon as the app answers
/// (CORE-3, D39).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_websites_link_into_the_local_network_is_left_to_the_browser() -> TestResult {
    let server = fast_server(1024).await?;
    let h = Harness::new().await?;
    let links = || -> TestResult<[String; 2]> {
        Ok([
            by_name(server.url("report.pdf"))?,
            server.url("report.pdf").to_string(),
        ])
    };
    for link in links()? {
        left_to_the_browser(&h, from_website(link)).await?;
    }
    tune(&h.manager, |s| s.extension.ask_before_download = true)?;
    for link in links()? {
        left_to_the_browser(&h, from_website(link)).await?;
    }
    tune(&h.manager, |s| s.extension.ask_before_download = false)?;
    let queue = h.manager.list_queues()[0].id;
    h.manager.set_queue_paused(queue, true)?;
    for link in links()? {
        left_to_the_browser(&h, from_website(link)).await?;
    }
    assert!(
        server.stats.seen().is_empty(),
        "the local server was contacted"
    );
    Ok(())
}

/// A page on the person's network may send Wirecatch to another device there:
/// one whose own address says so, or a site the person listed (D39).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_page_on_the_network_may_lead_to_it() -> TestResult {
    let server = fast_server(1024).await?;
    let h = Harness::new().await?;
    let added = h
        .manager
        .add_capture(capture_from(
            "http://nas/files",
            by_name(server.url("report.pdf"))?,
        ))
        .await?;
    h.wait_status(added.id.unwrap(), DownloadStatus::Completed)
        .await;

    let intranet = || -> TestResult<NewDownload> {
        Ok(capture_from(
            "https://wiki.corp.example.com/files",
            by_name(server.url("other.pdf"))?,
        ))
    };
    left_to_the_browser(&h, intranet()?).await?;
    tune(&h.manager, |s| {
        s.local_sites = vec!["corp.example.com".into()];
    })?;
    let added = h.manager.add_capture(intranet()?).await?;
    h.wait_status(added.id.unwrap(), DownloadStatus::Completed)
        .await;
    Ok(())
}

/// A link the person adds themselves may name a server on their own network,
/// also after the app restarts.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_link_the_person_adds_may_name_a_server_on_their_network() -> TestResult {
    let server = fast_server(1024).await?;
    let mut h = Harness::new().await?;
    let row = h.add(by_name(server.url("report.pdf"))?).await?;
    h.wait_status(row.id, DownloadStatus::Completed).await;
    let later = h
        .add_with(by_name(server.url("later.pdf"))?, |r| r.start = false)
        .await?;
    h.restart().await?;
    h.manager.resume(later.id)?;
    h.wait_status(later.id, DownloadStatus::Completed).await;
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_server_that_does_not_answer_in_time_leaves_the_download_to_the_browser() -> TestResult {
    let silent = SilentServer::start().await;
    let h = Harness::with_capture_timeout(Duration::from_millis(300)).await?;
    let mut client = Client::connected(&h.manager).await?;
    let lost = capture(silent.url("never.pdf").to_string());
    let err = client
        .call(Call::AddDownload(lost.clone()))
        .await?
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::NotStarted);
    assert!(err.message.contains("did not answer"), "{}", err.message);
    assert!(
        err.message.contains("The browser keeps it"),
        "{}",
        err.message
    );
    assert!(
        h.manager.list_downloads().is_empty(),
        "the attempt was left behind"
    );
    let reply = client
        .call(Call::WithdrawDownload(WithdrawRequest {
            handover_id: lost.handover_id,
        }))
        .await?
        .unwrap();
    assert_eq!(reply, Reply::Withdrawn(WithdrawOutcome::NotFound));
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_different_file_than_the_browser_saw_is_not_taken_over() -> TestResult {
    // The browser saw 1024 bytes; the app's request gets 2048.
    let resized = capture_server(2048, |_| {}).await;
    // The browser saw a PDF; the app's request gets a sign-in page.
    let page = capture_server(1024, |c| {
        c.content_type = Some("text/html; charset=utf-8".into());
    })
    .await;
    let h = Harness::new().await?;

    let err = h
        .manager
        .add_capture(capture(resized.url("report.pdf").to_string()))
        .await
        .unwrap_err();
    assert!(err.to_string().contains("different file"), "{err}");
    let err = h
        .manager
        .add_capture(capture(page.url("report.pdf").to_string()))
        .await
        .unwrap_err();
    assert!(err.to_string().contains("web page"), "{err}");
    assert_eq!(
        h.manager.list_downloads(),
        [] as [wirecatch_protocol::app::DownloadSummary; 0]
    );
    // Nothing was written.
    let documents = h.folders.downloads.join("Documents");
    let left = std::fs::read_dir(&documents).map_or(0, std::iter::Iterator::count);
    assert_eq!(left, 0, "files were left in {}", documents.display());
    Ok(())
}

/// The server holds its answers until the test lets them go, so the duplicate and the withdrawal both arrive while the capture waits for it, however slow the machine.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn while_waiting_a_duplicate_shares_the_outcome_and_a_withdrawal_wins() -> TestResult {
    let server = capture_server(1024, |_| {}).await;
    let h = Harness::new().await?;

    server.hold();
    let first = capture(server.url("a.pdf").to_string());
    let (a, b) = tokio::join!(h.manager.add_capture(first.clone()), async {
        server.wait_for_request("a.pdf", PATIENCE).await;
        // The duplicate asks first, while the server still holds its answer.
        tokio::join!(biased; h.manager.add_capture(first.clone()), async {
            server.release();
        })
        .0
    });
    let (a, b) = (a.unwrap(), b.unwrap());
    assert_eq!(a, b);
    assert_eq!(a.action, AddAction::Started);

    server.hold();
    let second = capture(server.url("b.pdf").to_string());
    let (added, withdrawn) = tokio::join!(h.manager.add_capture(second.clone()), async {
        server.wait_for_request("b.pdf", PATIENCE).await;
        h.manager.withdraw_capture(&second.handover_id)
    });
    assert_eq!(withdrawn.unwrap(), WithdrawOutcome::Withdrawn);
    assert!(matches!(
        added,
        Err(wirecatch_core::CoreError::NotStarted(_))
    ));
    let ids: Vec<_> = h.manager.list_downloads().iter().map(|r| r.id).collect();
    assert_eq!(ids, vec![a.id.unwrap()]);
    // Once started, a withdrawal no longer takes it back.
    let outcome = h.manager.withdraw_capture(&first.handover_id).unwrap();
    assert_eq!(outcome, WithdrawOutcome::AlreadyStarted);
    Ok(())
}

/// One slow request must not hold up the others on the same connection: the
/// extension's withdrawal, in particular, has to arrive while the capture it
/// withdraws is still waiting for a silent server. The capture waits longer than
/// the client waits for any answer, so an answer held up behind it never comes.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_slow_capture_does_not_hold_up_other_requests() -> TestResult {
    let silent = SilentServer::start().await;
    let h = Harness::with_capture_timeout(2 * PATIENCE).await?;
    let mut client = Client::connected(&h.manager).await?;
    let waiting = capture(silent.url("slow.pdf").to_string());
    let added = client.request(Call::AddDownload(waiting.clone())).await?;
    // The capture is now waiting for the server's answer.
    crate::common::wait_until("the capture to connect", || {
        (silent.accepted() > 0).then_some(())
    })
    .await;

    let reply = client.call(Call::ListQueues).await?.unwrap();
    assert!(matches!(reply, Reply::Queues(_)));

    let reply = client
        .call(Call::WithdrawDownload(WithdrawRequest {
            handover_id: waiting.handover_id.clone(),
        }))
        .await?
        .unwrap();
    assert_eq!(reply, Reply::Withdrawn(WithdrawOutcome::Withdrawn));
    let err = client.response(added).await?.unwrap_err();
    assert_eq!(err.code, ErrorCode::NotStarted);
    assert_eq!(
        h.manager.list_downloads(),
        [] as [wirecatch_protocol::app::DownloadSummary; 0]
    );
    Ok(())
}

/// A client may not pile up requests without limit: the ones beyond the limit get
/// an error at once, and the connection keeps working.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn requests_beyond_the_limit_are_refused() -> TestResult {
    // The server holds its answer, so no request finishes and frees its place before the
    // last one arrives; the capture outlasts the test, however slow the machine.
    let server = capture_server(1024, |_| {}).await;
    server.hold();
    let h = Harness::with_capture_timeout(2 * PATIENCE).await?;
    let mut client = Client::connected(&h.manager).await?;
    // The first add to run waits for the server; every repeat of it gets that one's
    // outcome, whether it runs before the server answers or after.
    let held = capture(server.url("held.pdf").to_string());
    let mut ids = Vec::new();
    for _ in 0..=wirecatch_core::MAX_REQUESTS_IN_FLIGHT {
        ids.push(client.request(Call::AddDownload(held.clone())).await?);
    }
    let last = ids.pop().unwrap();
    let err = client.response(last).await?.unwrap_err();
    assert_eq!(err.code, ErrorCode::LimitReached);
    assert!(err.message.contains("at once"), "{}", err.message);
    // The server's answer ends the waiting requests; the connection works afterwards.
    server.release();
    let mut replies = Vec::new();
    for id in ids {
        replies.push(client.response(id).await?.map_err(|e| e.message)?);
    }
    let Reply::DownloadAdded(added) = &replies[0] else {
        return Err(format!("{:?}", replies[0]).into());
    };
    assert_eq!(added.action, AddAction::Started);
    assert!(replies.iter().all(|r| r == &replies[0]), "{replies:?}");
    assert!(client.call(Call::ListQueues).await?.is_ok());
    Ok(())
}

/// Cookies and credentials belong to the site the browser got them from. A link
/// refreshed to another site must not carry them there; one refreshed on the
/// same site keeps them.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_link_refreshed_to_another_site_leaves_the_credentials_behind() -> TestResult {
    let first = fast_server(1024).await?;
    let other = fast_server(1024).await?;
    let h = Harness::new().await?;
    // Held in line, so nothing is requested before the links are refreshed.
    h.manager
        .set_queue_paused(wirecatch_core::DEFAULT_QUEUE_ID, true)
        .unwrap();
    let with_credentials = |name: &str| {
        let mut capture = capture(first.url(name).to_string());
        capture.extra_headers = vec![
            Header {
                name: "Authorization".into(),
                value: "Bearer secret".into(),
            },
            Header {
                name: "Accept-Language".into(),
                value: "en".into(),
            },
        ];
        capture
    };
    let stays = h.manager.add_capture(with_credentials("stays.pdf")).await;
    let moves = h.manager.add_capture(with_credentials("moves.pdf")).await;
    let (stays, moves) = (stays.unwrap().id.unwrap(), moves.unwrap().id.unwrap());
    h.manager
        .refresh_link(
            stays,
            first.url("fresh/stays.pdf").as_str(),
            LinkFrom::Person,
        )
        .unwrap();
    h.manager
        .refresh_link(moves, other.url("moves.pdf").as_str(), LinkFrom::Person)
        .unwrap();
    h.manager
        .set_queue_paused(wirecatch_core::DEFAULT_QUEUE_ID, false)
        .unwrap();
    h.wait_status(stays, DownloadStatus::Completed).await;
    h.wait_status(moves, DownloadStatus::Completed).await;

    assert_eq!(first.stats.header_values("cookie"), vec!["session=abc"]);
    assert_eq!(
        first.stats.header_values("authorization"),
        vec!["Bearer secret"]
    );
    assert!(
        other.stats.header_values("cookie").is_empty(),
        "the other site got the first site's cookies"
    );
    assert!(
        other.stats.header_values("authorization").is_empty(),
        "the other site got the first site's credentials"
    );
    assert!(
        other
            .stats
            .requests
            .load(std::sync::atomic::Ordering::SeqCst)
            > 0
    );
    Ok(())
}

/// A queued capture that later gets another file than the browser saw fails with
/// a way out: refreshing the link from the page.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_queued_capture_that_gets_another_file_offers_a_fresh_link() -> TestResult {
    // The browser saw 1024 bytes; this link now serves 2048.
    let changed = fast_server(2048).await?;
    let right = fast_server(1024).await?;
    let h = Harness::new().await?;
    h.manager
        .set_queue_paused(wirecatch_core::DEFAULT_QUEUE_ID, true)
        .unwrap();
    let added = h
        .manager
        .add_capture(capture(changed.url("report.pdf").to_string()))
        .await
        .unwrap();
    assert_eq!(added.action, AddAction::Queued);
    let id = added.id.unwrap();
    h.manager
        .set_queue_paused(wirecatch_core::DEFAULT_QUEUE_ID, false)
        .unwrap();
    let failed = h.wait_status(id, DownloadStatus::Failed).await;
    let error = failed.error.unwrap();
    assert!(
        error.message.contains("different file"),
        "{}",
        error.message
    );
    assert_eq!(
        error.fix,
        Some(wirecatch_protocol::app::FixAction::RefreshLink)
    );

    h.manager
        .refresh_link(id, right.url("report.pdf").as_str(), LinkFrom::Person)
        .unwrap();
    let done = h.wait_status(id, DownloadStatus::Completed).await;
    assert_eq!(std::fs::read(&done.path).unwrap(), right.data);
    Ok(())
}

fn framed(body: &[u8]) -> TestResult<Vec<u8>> {
    let mut out = frame::length_prefix(body.len())?.to_vec();
    out.extend_from_slice(body);
    Ok(out)
}

/// Waits for the next confirmation dialog to close and returns its id.
async fn closed(ui: &mut tokio::sync::broadcast::Receiver<UiEvent>) -> TestResult<QuestionId> {
    loop {
        if let UiEvent::ConfirmationClosed { id } = ui.recv().await? {
            return Ok(id);
        }
    }
}

/// A confirmation dialog must close when its question goes away, whichever way.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_closed_question_is_announced_so_the_dialog_closes() -> TestResult {
    let server = fast_server(1024).await?;
    let h = Harness::new().await?;
    tune(&h.manager, |s| s.extension.ask_before_download = true)?;
    let mut ui = h.manager.subscribe();

    let withdrawn = capture(server.url("withdrawn.bin").to_string());
    h.manager.add_capture(withdrawn.clone()).await.unwrap();
    let asked = h.manager.pending_confirmations().remove(0);
    h.manager.withdraw_capture(&withdrawn.handover_id).unwrap();
    assert_eq!(closed(&mut ui).await?, asked.id);

    h.manager
        .add_capture(capture(server.url("rejected.bin").to_string()))
        .await
        .unwrap();
    let asked = h.manager.pending_confirmations().remove(0);
    h.manager.reject_capture(asked.id).unwrap();
    assert_eq!(closed(&mut ui).await?, asked.id);

    h.manager
        .add_capture(capture(server.url("confirmed.bin").to_string()))
        .await
        .unwrap();
    let asked = h.manager.pending_confirmations().remove(0);
    h.manager
        .confirm_capture(asked.id, CaptureConfirmation::default())
        .await
        .unwrap();
    assert_eq!(closed(&mut ui).await?, asked.id);
    Ok(())
}

/// The stored `origin` (cookies and headers) of download `id`, read from the
/// database the way anyone with the file could; `None` until the database
/// thread has written the row, so callers can wait for it.
fn stored_origin(h: &Harness, id: wirecatch_protocol::DownloadId) -> Option<String> {
    let db = h
        .root
        .path()
        .join("data")
        .join(wirecatch_core::DATABASE_FILE);
    let db = rusqlite::Connection::open(db).ok()?;
    db.query_row("SELECT origin FROM downloads WHERE id = ?1", [id.0], |r| {
        r.get(0)
    })
    .ok()
}

async fn wait_until_stored(
    h: &Harness,
    id: wirecatch_protocol::DownloadId,
    check: impl Fn(&str) -> bool,
) -> String {
    crate::common::wait_until("the stored origin", || {
        stored_origin(h, id).filter(|o| check(o))
    })
    .await
}

/// D17: a download that has not finished a week after it was added forgets
/// the browser's sign-in (cookies, credentials, tokens), on disk too. If it
/// then needs a fresh link, its row says why.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_download_unfinished_after_a_week_forgets_its_sign_in() -> TestResult {
    let server = fast_server(1024).await?;
    // The server wants the sign-in for this one.
    server.configure(|c| c.refuse_paths.push(("/old.pdf".into(), 403)));
    let h = Harness::new().await?;
    h.manager
        .set_queue_paused(wirecatch_core::DEFAULT_QUEUE_ID, true)
        .unwrap();
    let mut old = capture(server.url("old.pdf").to_string());
    old.extra_headers = vec![
        Header {
            name: "Authorization".into(),
            value: "Bearer secret".into(),
        },
        Header {
            name: "X-Session-Token".into(),
            value: "t0k3n".into(),
        },
        Header {
            name: "Accept-Language".into(),
            value: "en".into(),
        },
    ];
    let old = h.manager.add_capture(old).await.unwrap().id.unwrap();
    let stored = wait_until_stored(&h, old, |o| o.contains("session=abc")).await;
    assert!(stored.contains("Bearer secret"), "{stored}");

    // Six days on, it is all still there.
    h.clock.advance(chrono::TimeDelta::days(6));
    tokio::time::sleep(Duration::from_millis(200)).await;
    wait_until_stored(&h, old, |o| o.contains("session=abc")).await;

    // A week on, the sign-in is gone; harmless headers stay.
    h.clock
        .advance(chrono::TimeDelta::days(1) + chrono::TimeDelta::minutes(1));
    let stored = wait_until_stored(&h, old, |o| !o.contains("session=abc")).await;
    for secret in ["Bearer secret", "t0k3n", "Cookie"] {
        assert!(!stored.contains(secret), "{secret} kept: {stored}");
    }
    assert!(stored.contains("Accept-Language"), "{stored}");
    assert!(
        stored.contains("intranet.test/page"),
        "the referrer went: {stored}"
    );

    // A download added now keeps its sign-in.
    let young = h
        .manager
        .add_capture(capture(server.url("young.pdf").to_string()))
        .await
        .unwrap()
        .id
        .unwrap();
    h.manager
        .set_queue_paused(wirecatch_core::DEFAULT_QUEUE_ID, false)
        .unwrap();
    let failed = h.wait_status(old, DownloadStatus::Failed).await;
    let error = failed.error.unwrap();
    assert_eq!(
        error.message,
        "Wirecatch forgot the sign-in for this download after 7 days; get a fresh link."
    );
    assert_eq!(
        error.fix,
        Some(wirecatch_protocol::app::FixAction::RefreshLink)
    );
    h.wait_status(young, DownloadStatus::Completed).await;
    // Only the young download sent cookies, and none sent credentials.
    assert_eq!(server.stats.header_values("cookie"), vec!["session=abc"]);
    assert_eq!(
        server.stats.header_values("authorization"),
        [] as [std::string::String; 0]
    );
    Ok(())
}

/// A password in a link is never shown: rows, events, captures waiting for an
/// answer and error messages hide it, while the manager keeps (and stores)
/// the real link to download with.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_password_in_a_link_is_never_shown() -> TestResult {
    let server = fast_server(1024).await?;
    server.configure(|c| c.refuse_paths.push(("/locked.bin".into(), 403)));
    let h = Harness::new().await?;
    let mut ui = h.manager.subscribe();
    let secret_url = |name: &str| {
        server
            .url(name)
            .as_str()
            .replacen("http://", "http://ada:s3cret@", 1)
    };
    let row = h.add(&secret_url("locked.bin")).await?;
    assert!(row.url.starts_with("http://ada:***@"), "{}", row.url);
    let failed = h.wait_status(row.id, DownloadStatus::Failed).await;
    assert!(!failed.url.contains("s3cret"), "{}", failed.url);
    assert!(
        !failed.error.unwrap().message.contains("s3cret"),
        "the error showed the password"
    );
    for listed in h.manager.list_downloads() {
        assert!(!listed.url.contains("s3cret"), "{}", listed.url);
    }
    assert!(
        !h.manager
            .get_download(row.id)
            .unwrap()
            .url
            .contains("s3cret")
    );

    // A capture waiting for an answer shows the hidden form, and downloads the real link.
    tune(&h.manager, |s| s.extension.ask_before_download = true)?;
    let mut asked = capture(secret_url("asked.bin"));
    asked.page_url = Some("https://ada:pagepass@intranet.test/page".into());
    h.manager.add_capture(asked).await.unwrap();
    let pending = h.manager.pending_confirmations().remove(0);
    assert!(
        pending.url.starts_with("http://ada:***@"),
        "{}",
        pending.url
    );
    assert_eq!(
        pending.page_url.as_deref(),
        Some("https://ada:***@intranet.test/page")
    );
    h.manager
        .confirm_capture(pending.id, CaptureConfirmation::default())
        .await
        .unwrap();
    let confirmed = crate::common::wait_until("the confirmed download", || {
        h.manager
            .list_downloads()
            .into_iter()
            .find(|r| r.url.ends_with("/asked.bin"))
    })
    .await;
    let done = h.wait_status(confirmed.id, DownloadStatus::Completed).await;
    assert!(!done.url.contains("s3cret"));

    // Nothing the UI heard had the password in it.
    while let Ok(event) = ui.try_recv() {
        let text = serde_json::to_string(&event).unwrap();
        assert!(
            !text.contains("s3cret") && !text.contains("pagepass"),
            "{text}"
        );
    }
    // The manager kept the real link to download with.
    h.manager.shutdown().await;
    let db = h
        .root
        .path()
        .join("data")
        .join(wirecatch_core::DATABASE_FILE);
    let db = rusqlite::Connection::open(db).unwrap();
    let stored: String = db
        .query_row("SELECT url FROM downloads WHERE id = ?1", [row.id.0], |r| {
            r.get(0)
        })
        .unwrap();
    assert!(stored.contains("ada:s3cret@"), "{stored}");
    Ok(())
}
