//! Acceptance 6 and 7: the protocol server refuses bad clients clearly without
//! crashing, and a settings change from any side reaches every client. Also the
//! extension's capture flow, with and without confirmation.

use std::collections::{HashMap, VecDeque};
use std::time::{Duration, Instant};

use tokio::io::{AsyncWriteExt, DuplexStream};
use tokio::task::JoinHandle;
use wirecatch_core::{Manager, serve_connection};
use wirecatch_protocol::PROTOCOL_VERSION;
use wirecatch_protocol::app::{CaptureConfirmation, DownloadStatus, UiEvent};
use wirecatch_protocol::extension::{
    AddAction, CaptureSource, ExtensionSettingsPatch, NewDownload, WithdrawOutcome, WithdrawRequest,
};
use wirecatch_protocol::frame::{self, MAX_FRAME_LEN};
use wirecatch_protocol::wire::{
    Call, ClientKind, ErrorCode, Event, Hello, Message, Outcome, ProtocolError, Reply,
    UNKNOWN_REQUEST_ID,
};

use crate::common::{Config, Harness, SilentServer, TestServer, test_data, tune};

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
        let (stream, server_side) = tokio::io::duplex(64 * 1024);
        let server = tokio::spawn(serve_connection(manager.clone(), server_side));
        Self {
            stream,
            server,
            next_id: 1,
            events: VecDeque::new(),
            answered: HashMap::new(),
        }
    }

    async fn connected(manager: &Manager) -> Self {
        let mut client = Self::connect(manager);
        let reply = client.call(hello(PROTOCOL_VERSION)).await.unwrap();
        assert!(matches!(reply, Reply::Hello(r) if r.protocol_version == PROTOCOL_VERSION));
        client
    }

    async fn send(&mut self, message: &Message) {
        frame::write_async(&mut self.stream, message).await.unwrap();
    }

    async fn recv(&mut self) -> Option<Message> {
        tokio::time::timeout(Duration::from_secs(10), frame::read_async(&mut self.stream))
            .await
            .expect("no answer from the server")
            .unwrap()
    }

    /// Sends a call and returns its outcome, keeping events that arrive meanwhile.
    async fn call(&mut self, call: Call) -> Result<Reply, ProtocolError> {
        let id = self.request(call).await;
        self.response(id).await
    }

    /// Sends a call without waiting for its answer, and returns its id.
    async fn request(&mut self, call: Call) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        self.send(&Message::Request { id, call }).await;
        id
    }

    async fn response(&mut self, id: u64) -> Result<Reply, ProtocolError> {
        loop {
            if let Some(outcome) = self.answered.remove(&id) {
                return match outcome {
                    Outcome::Ok(reply) => Ok(reply),
                    Outcome::Err(e) => Err(e),
                };
            }
            match self.recv().await.expect("connection closed") {
                Message::Response { id: got, outcome } => {
                    let earlier = self.answered.insert(got, outcome);
                    assert!(earlier.is_none(), "two responses to request {got}");
                }
                Message::Event { event } => self.events.push_back(event),
                Message::Request { .. } => panic!("the server sent a request"),
            }
        }
    }

    async fn next_event(&mut self) -> Event {
        if let Some(event) = self.events.pop_front() {
            return event;
        }
        match self.recv().await.expect("connection closed") {
            Message::Event { event } => event,
            other => panic!("expected an event, got {other:?}"),
        }
    }

    /// Asserts the server closed the connection cleanly, without panicking.
    async fn assert_closed(mut self) {
        assert_eq!(self.recv().await, None, "connection still open");
        let served = tokio::time::timeout(Duration::from_secs(5), self.server)
            .await
            .expect("server task still running");
        served.expect("server task panicked").unwrap();
    }
}

fn hello(protocol_version: u32) -> Call {
    Call::Hello(Hello {
        client: ClientKind::Extension,
        client_version: "0.1.0".into(),
        protocol_version,
        browser: None,
    })
}

/// A capture whose handover id is derived from its link.
fn capture(url: String) -> NewDownload {
    NewDownload {
        handover_id: format!("handover-{url}"),
        url,
        referrer: Some("https://example.com/page".into()),
        page_url: Some("https://example.com/page".into()),
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
async fn skipping_hello_is_refused_and_closed() {
    let h = Harness::new().await;
    let mut client = Client::connect(&h.manager);
    let err = client.call(Call::PauseAll).await.unwrap_err();
    assert_eq!(err.code, ErrorCode::InvalidRequest);
    assert!(
        err.message.contains("first message must be hello"),
        "{}",
        err.message
    );
    client.assert_closed().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_wrong_protocol_version_is_refused_and_closed() {
    let h = Harness::new().await;
    let mut client = Client::connect(&h.manager);
    let err = client.call(hello(PROTOCOL_VERSION + 1)).await.unwrap_err();
    assert_eq!(err.code, ErrorCode::UnsupportedVersion);
    assert!(
        err.message.contains("Update the Wirecatch app"),
        "{}",
        err.message
    );
    client.assert_closed().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn bad_messages_get_errors_and_the_connection_survives() {
    let h = Harness::new().await;
    let mut client = Client::connected(&h.manager).await;

    // An unknown method still has a readable id.
    let unknown = br#"{"kind":"request","id":41,"call":{"method":"format_disk"}}"#;
    client.stream.write_all(&framed(unknown)).await.unwrap();
    let err = client.response(41).await.unwrap_err();
    assert_eq!(err.code, ErrorCode::InvalidRequest);
    assert!(err.message.contains("format_disk"), "{}", err.message);

    // Garbage has no id at all.
    client
        .stream
        .write_all(&framed(b"\x00\xffnot json"))
        .await
        .unwrap();
    let err = client.response(UNKNOWN_REQUEST_ID).await.unwrap_err();
    assert_eq!(err.code, ErrorCode::InvalidRequest);

    // An oversized frame is skipped without being stored.
    let len = MAX_FRAME_LEN + 1;
    client
        .stream
        .write_all(&(len as u32).to_le_bytes())
        .await
        .unwrap();
    let filler = vec![b' '; 64 * 1024];
    let mut left = len;
    while left > 0 {
        let n = left.min(filler.len());
        client.stream.write_all(&filler[..n]).await.unwrap();
        left -= n;
    }
    let err = client.response(UNKNOWN_REQUEST_ID).await.unwrap_err();
    assert_eq!(err.code, ErrorCode::InvalidRequest);
    assert!(err.message.contains("limit"), "{}", err.message);

    // A client may only send requests.
    client
        .send(&Message::Event {
            event: Event::QueuesChanged(vec![]),
        })
        .await;
    assert!(client.response(UNKNOWN_REQUEST_ID).await.is_err());

    // After all that, the connection still works.
    let reply = client.call(Call::ListQueues).await.unwrap();
    assert!(matches!(reply, Reply::Queues(q) if q.len() == 1 && q[0].is_default));
    drop(client.stream);
    client.server.await.unwrap().unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_client_vanishing_mid_frame_does_not_hurt_the_server() {
    let h = Harness::new().await;
    let mut client = Client::connected(&h.manager).await;
    client
        .stream
        .write_all(&[200, 0, 0, 0, b'{'])
        .await
        .unwrap();
    drop(client.stream);
    client.server.await.unwrap().unwrap();
    // Other clients are unaffected.
    let mut other = Client::connected(&h.manager).await;
    assert!(other.call(Call::GetExtensionSettings).await.is_ok());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn settings_changes_reach_every_client() {
    let h = Harness::new().await;
    let mut ui = h.manager.subscribe();
    let mut chrome = Client::connected(&h.manager).await;
    let mut firefox = Client::connected(&h.manager).await;

    // From one client to the other client and to the UI.
    let patch = ExtensionSettingsPatch {
        ask_before_download: Some(true),
        ignored_sites: Some(vec!["Example.COM".into()]),
        ..Default::default()
    };
    let reply = chrome
        .call(Call::UpdateExtensionSettings(patch))
        .await
        .unwrap();
    let Reply::ExtensionSettings(saved) = reply else {
        panic!("unexpected reply {reply:?}");
    };
    assert_eq!(saved.ignored_sites, vec!["example.com"]);
    assert_eq!(
        firefox.next_event().await,
        Event::ExtensionSettingsChanged(saved.clone())
    );
    assert!(matches!(
        ui.recv().await.unwrap(),
        UiEvent::SettingsChanged(s) if s.extension == saved
    ));

    // From the UI to every client.
    tune(&h.manager, |s| s.extension.capture_enabled = false);
    for client in [&mut chrome, &mut firefox] {
        loop {
            match client.next_event().await {
                Event::ExtensionSettingsChanged(s) if !s.capture_enabled => break,
                Event::ExtensionSettingsChanged(_) => continue,
                other => panic!("unexpected event {other:?}"),
            }
        }
    }

    // Queue changes are pushed too.
    h.manager.create_queue("Later").unwrap();
    assert!(matches!(
        firefox.next_event().await,
        Event::QueuesChanged(q) if q.iter().any(|q| q.name == "Later")
    ));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_capture_starts_at_once_and_shows_a_toast() {
    let server = TestServer::fast(1024).await;
    let h = Harness::new().await;
    let mut ui = h.manager.subscribe();
    let mut client = Client::connected(&h.manager).await;

    let reply = client
        .call(Call::AddDownload(capture(server.url("get?id=7"))))
        .await
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
            .cookies
            .lock()
            .unwrap()
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
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ask_before_download_waits_for_the_user() {
    let server = TestServer::fast(1024).await;
    let h = Harness::new().await;
    tune(&h.manager, |s| s.extension.ask_before_download = true);
    let mut ui = h.manager.subscribe();
    let mut client = Client::connected(&h.manager).await;

    let ask = |name: &str| capture(server.url(name));
    let reply = client.call(Call::AddDownload(ask("a"))).await.unwrap();
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
        Some("https://example.com/page")
    );
    assert_eq!(h.manager.pending_confirmations(), vec![proposal.clone()]);
    assert!(h.manager.list_downloads().is_empty());

    // The user picks another name and folder.
    let folder = h.folders.home.join("Chosen");
    h.manager
        .confirm_capture(
            proposal.id,
            CaptureConfirmation {
                folder: Some(folder.to_string_lossy().into_owned()),
                file_name: Some("mine.pdf".into()),
                queue: None,
            },
        )
        .await
        .unwrap();
    let row = h.manager.list_downloads().remove(0);
    assert_eq!(std::path::Path::new(&row.path), folder.join("mine.pdf"));
    h.wait_status(row.id, DownloadStatus::Completed).await;
    assert!(h.manager.pending_confirmations().is_empty());

    // A rejected capture downloads nothing.
    client.call(Call::AddDownload(ask("b"))).await.unwrap();
    let second = h.manager.pending_confirmations().remove(0);
    h.manager.reject_capture(second.id).unwrap();
    assert!(h.manager.pending_confirmations().is_empty());
    assert_eq!(h.manager.list_downloads().len(), 1);
    assert!(h.manager.reject_capture(second.id).is_err());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_repeated_handover_adds_nothing() {
    let server = TestServer::fast(1024).await;
    let h = Harness::new().await;
    let mut client = Client::connected(&h.manager).await;

    let first = client
        .call(Call::AddDownload(capture(server.url("x.pdf"))))
        .await
        .unwrap();
    let again = client
        .call(Call::AddDownload(capture(server.url("x.pdf"))))
        .await
        .unwrap();
    assert_eq!(first, again);

    // Two copies racing each other still make one download.
    let racing = capture(server.url("y.pdf"));
    let (a, b) = tokio::join!(
        h.manager.add_capture(racing.clone()),
        h.manager.add_capture(racing)
    );
    assert_eq!(a.unwrap(), b.unwrap());
    assert_eq!(h.manager.list_downloads().len(), 2);

    // The same holds while confirmations are asked for.
    tune(&h.manager, |s| s.extension.ask_before_download = true);
    let asked = capture(server.url("z.pdf"));
    let first = h.manager.add_capture(asked.clone()).await.unwrap();
    let again = h.manager.add_capture(asked).await.unwrap();
    assert_eq!(
        (first.action, &first),
        (AddAction::AwaitingConfirmation, &again)
    );
    assert_eq!(h.manager.pending_confirmations().len(), 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn confirmations_are_deduplicated_and_capped() {
    let h = Harness::new().await;
    tune(&h.manager, |s| s.extension.ask_before_download = true);
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
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn withdraw_drops_only_what_the_server_has_not_answered() {
    let server = TestServer::fast(1024).await;
    let h = Harness::new().await;
    let mut client = Client::connected(&h.manager).await;
    let withdraw = |capture: &NewDownload| {
        Call::WithdrawDownload(WithdrawRequest {
            handover_id: capture.handover_id.clone(),
        })
    };
    let outcome = |reply| match reply {
        Reply::Withdrawn(outcome) => outcome,
        other => panic!("unexpected reply {other:?}"),
    };

    let unknown = capture(server.url("never-sent"));
    let reply = client.call(withdraw(&unknown)).await.unwrap();
    assert_eq!(outcome(reply), WithdrawOutcome::NotFound);

    // A capture waiting for its queue's schedule has not reached the server.
    let mut queue = h.manager.list_queues().remove(0);
    queue.schedule = Some(wirecatch_protocol::app::Schedule {
        start: wirecatch_protocol::app::TimeOfDay { hour: 2, minute: 0 },
        stop: Some(wirecatch_protocol::app::TimeOfDay { hour: 3, minute: 0 }),
        days: vec![],
    });
    h.manager.update_queue(queue.clone()).unwrap();
    let waiting = capture(server.url("waiting.bin"));
    let added = h.manager.add_capture(waiting.clone()).await.unwrap();
    assert_eq!(added.action, AddAction::Queued);
    let reply = client.call(withdraw(&waiting)).await.unwrap();
    assert_eq!(outcome(reply), WithdrawOutcome::Withdrawn);
    assert!(h.manager.list_downloads().is_empty());
    assert_eq!(
        server
            .stats
            .requests
            .load(std::sync::atomic::Ordering::SeqCst),
        0
    );
    let reply = client.call(withdraw(&waiting)).await.unwrap();
    assert_eq!(outcome(reply), WithdrawOutcome::NotFound);

    // A finished one is the app's now.
    queue.schedule = None;
    h.manager.update_queue(queue).unwrap();
    let done = capture(server.url("done.bin"));
    let id = h
        .manager
        .add_capture(done.clone())
        .await
        .unwrap()
        .id
        .unwrap();
    h.wait_status(id, DownloadStatus::Completed).await;
    let reply = client.call(withdraw(&done)).await.unwrap();
    assert_eq!(outcome(reply), WithdrawOutcome::AlreadyStarted);
    assert_eq!(h.manager.list_downloads().len(), 1);

    // A capture waiting for the user's confirmation is dropped too.
    tune(&h.manager, |s| s.extension.ask_before_download = true);
    let asked = capture(server.url("asked.bin"));
    h.manager.add_capture(asked.clone()).await.unwrap();
    let reply = client.call(withdraw(&asked)).await.unwrap();
    assert_eq!(outcome(reply), WithdrawOutcome::Withdrawn);
    assert!(h.manager.pending_confirmations().is_empty());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ignored_site_edits_from_two_browsers_merge() {
    let h = Harness::new().await;
    let mut chrome = Client::connected(&h.manager).await;
    let mut firefox = Client::connected(&h.manager).await;
    let edit = |add: &[&str], remove: &[&str]| {
        let list = |l: &[&str]| Some(l.iter().map(|s| s.to_string()).collect());
        Call::UpdateExtensionSettings(ExtensionSettingsPatch {
            add_ignored_sites: list(add),
            remove_ignored_sites: list(remove),
            ..Default::default()
        })
    };
    chrome.call(edit(&["A.example"], &[])).await.unwrap();
    firefox.call(edit(&["b.example"], &[])).await.unwrap();
    let reply = chrome.call(edit(&[], &["a.EXAMPLE/"])).await.unwrap();
    let Reply::ExtensionSettings(saved) = reply else {
        panic!("unexpected reply {reply:?}");
    };
    assert_eq!(saved.ignored_sites, vec!["b.example"]);
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
async fn started_means_the_server_answered() {
    let delay = Duration::from_millis(400);
    let server = capture_server(1024, |c| c.answer_delay = Some(delay)).await;
    let h = Harness::new().await;
    let start = std::time::Instant::now();
    let added = h
        .manager
        .add_capture(capture(server.url("report.pdf")))
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
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_server_that_does_not_answer_in_time_leaves_the_download_to_the_browser() {
    let silent = SilentServer::start().await;
    let h = Harness::with_capture_timeout(Duration::from_millis(300)).await;
    let mut client = Client::connected(&h.manager).await;
    let lost = capture(silent.url("never.pdf"));
    let err = client
        .call(Call::AddDownload(lost.clone()))
        .await
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
        .await
        .unwrap();
    assert_eq!(reply, Reply::Withdrawn(WithdrawOutcome::NotFound));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_different_file_than_the_browser_saw_is_not_taken_over() {
    // The browser saw 1024 bytes; the app's request gets 2048.
    let resized = capture_server(2048, |_| {}).await;
    // The browser saw a PDF; the app's request gets a sign-in page.
    let page = capture_server(1024, |c| {
        c.content_type = Some("text/html; charset=utf-8".into());
    })
    .await;
    let h = Harness::new().await;

    let err = h
        .manager
        .add_capture(capture(resized.url("report.pdf")))
        .await
        .unwrap_err();
    assert!(err.to_string().contains("different file"), "{err}");
    let err = h
        .manager
        .add_capture(capture(page.url("report.pdf")))
        .await
        .unwrap_err();
    assert!(err.to_string().contains("web page"), "{err}");
    assert!(h.manager.list_downloads().is_empty());
    // Nothing was written.
    let documents = h.folders.downloads.join("Documents");
    let left = std::fs::read_dir(&documents).map_or(0, |d| d.count());
    assert_eq!(left, 0, "files were left in {}", documents.display());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn while_waiting_a_duplicate_shares_the_outcome_and_a_withdrawal_wins() {
    let delay = Duration::from_millis(500);
    let server = capture_server(1024, |c| c.answer_delay = Some(delay)).await;
    let h = Harness::new().await;

    let first = capture(server.url("a.pdf"));
    let (a, b) = tokio::join!(h.manager.add_capture(first.clone()), async {
        tokio::time::sleep(Duration::from_millis(100)).await;
        h.manager.add_capture(first.clone()).await
    });
    let (a, b) = (a.unwrap(), b.unwrap());
    assert_eq!(a, b);
    assert_eq!(a.action, AddAction::Started);

    let second = capture(server.url("b.pdf"));
    let (added, withdrawn) = tokio::join!(h.manager.add_capture(second.clone()), async {
        tokio::time::sleep(Duration::from_millis(100)).await;
        h.manager.withdraw_capture(&second.handover_id).await
    });
    assert_eq!(withdrawn.unwrap(), WithdrawOutcome::Withdrawn);
    assert!(matches!(
        added,
        Err(wirecatch_core::CoreError::NotStarted(_))
    ));
    let ids: Vec<_> = h.manager.list_downloads().iter().map(|r| r.id).collect();
    assert_eq!(ids, vec![a.id.unwrap()]);
    // Once started, a withdrawal no longer takes it back.
    let outcome = h
        .manager
        .withdraw_capture(&first.handover_id)
        .await
        .unwrap();
    assert_eq!(outcome, WithdrawOutcome::AlreadyStarted);
}

/// One slow request must not hold up the others on the same connection: the
/// extension's withdrawal, in particular, has to arrive while the capture it
/// withdraws is still waiting for a silent server.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_slow_capture_does_not_hold_up_other_requests() {
    let silent = SilentServer::start().await;
    let h = Harness::with_capture_timeout(Duration::from_secs(5)).await;
    let mut client = Client::connected(&h.manager).await;
    let waiting = capture(silent.url("slow.pdf"));
    let added = client.request(Call::AddDownload(waiting.clone())).await;
    // The capture is now waiting for the server's answer.
    crate::common::wait_until("the capture to connect", || {
        (silent.accepted.load(std::sync::atomic::Ordering::SeqCst) > 0).then_some(())
    })
    .await;

    let asked = Instant::now();
    let reply = client.call(Call::ListQueues).await.unwrap();
    assert!(matches!(reply, Reply::Queues(_)));
    assert!(
        asked.elapsed() < Duration::from_secs(1),
        "ListQueues waited {:?} behind the capture",
        asked.elapsed()
    );

    let reply = client
        .call(Call::WithdrawDownload(WithdrawRequest {
            handover_id: waiting.handover_id.clone(),
        }))
        .await
        .unwrap();
    assert_eq!(reply, Reply::Withdrawn(WithdrawOutcome::Withdrawn));
    assert!(
        asked.elapsed() < Duration::from_secs(3),
        "the withdrawal waited {:?} behind the capture",
        asked.elapsed()
    );
    let err = client.response(added).await.unwrap_err();
    assert_eq!(err.code, ErrorCode::NotStarted);
    assert!(h.manager.list_downloads().is_empty());
}

/// A client may not pile up requests without limit: the ones beyond the limit get
/// an error at once, and the connection keeps working.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn requests_beyond_the_limit_are_refused() {
    let silent = SilentServer::start().await;
    let h = Harness::with_capture_timeout(Duration::from_secs(5)).await;
    let mut client = Client::connected(&h.manager).await;
    // The first add waits for the silent server; every repeat of it waits for the
    // first one's outcome.
    let slow = capture(silent.url("slow.pdf"));
    let mut ids = Vec::new();
    for _ in 0..=wirecatch_core::MAX_REQUESTS_IN_FLIGHT {
        ids.push(client.request(Call::AddDownload(slow.clone())).await);
    }
    let last = ids.pop().unwrap();
    let err = client.response(last).await.unwrap_err();
    assert_eq!(err.code, ErrorCode::InvalidRequest);
    assert!(err.message.contains("at once"), "{}", err.message);
    // Withdrawing ends the waiting requests; the connection works afterwards.
    crate::common::wait_until("the capture to connect", || {
        (silent.accepted.load(std::sync::atomic::Ordering::SeqCst) > 0).then_some(())
    })
    .await;
    let outcome = h.manager.withdraw_capture(&slow.handover_id).await.unwrap();
    assert_eq!(outcome, WithdrawOutcome::Withdrawn);
    for id in ids {
        let err = client.response(id).await.unwrap_err();
        assert_eq!(err.code, ErrorCode::NotStarted, "{}", err.message);
    }
    assert!(client.call(Call::ListQueues).await.is_ok());
}

fn framed(body: &[u8]) -> Vec<u8> {
    let mut out = (body.len() as u32).to_le_bytes().to_vec();
    out.extend_from_slice(body);
    out
}
