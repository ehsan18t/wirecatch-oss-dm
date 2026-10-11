//! Relays frames between the browser and the app.
//!
//! Rules, in the order they apply:
//!
//! 1. Nothing happens until the extension sends its first frame; that frame
//!    triggers the connection to the app (starting the app if needed). Frames
//!    that arrive meanwhile are kept in order, and if the browser closes the
//!    connection meanwhile the relay ends at once.
//! 2. While the app is connected, every frame from the extension is passed to the
//!    app and every frame from the app is passed to the extension, byte for byte.
//!    The ids of requests still waiting for a response are remembered; a request
//!    that reuses one of those ids is answered with `InvalidRequest` instead of
//!    being sent, so every id gets exactly one answer.
//! 3. If the first connection cannot be made, every request is answered with
//!    `AppUnavailable` until the browser closes the connection. The bridge does
//!    not retry on its own, so a broken install cannot cause a launch loop; the
//!    extension reconnects (which starts a new bridge) when it wants to try again.
//! 4. If the connection to the app ends during the session (for example the user
//!    quit Wirecatch from the tray), every request still waiting is answered with
//!    `AppUnavailable` and the relay ends. The bridge then exits, the browser sees
//!    the port disconnect, and the extension's normal reconnect starts a new
//!    bridge, which starts the app again.
//! 5. When the browser closes stdin, the relay ends.

use std::collections::{BTreeSet, VecDeque};
use std::future::Future;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use tokio::io::{AsyncRead, AsyncWrite, AsyncWriteExt, WriteHalf};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use wirecatch_protocol::wire::{ErrorCode, Message, Outcome, ProtocolError};

use crate::app::Unavailable;
use crate::raw_frame::{self, Envelope, RawFrame};

/// Request ids sent to the app and not answered yet.
type Pending = Arc<Mutex<BTreeSet<u64>>>;

/// How many frames from the extension are held while the app is starting. Past
/// this the relay stops reading (and so stops noticing a closed stdin) until the
/// connection attempt ends, which keeps memory bounded.
const MAX_HELD_WHILE_CONNECTING: usize = 32;

/// How the relay ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Ended {
    /// The browser closed the connection.
    BrowserClosed,
    /// The app closed the connection during the session.
    AppClosed,
}

/// Runs the relay until the browser closes `incoming`, or until the app goes away
/// after having been connected.
///
/// `outgoing` carries frames to the browser. `connect` is called at most once, with
/// the first frame (the extension's `hello`, which says whether the app may be started).
pub(crate) async fn run<C, F, Fut>(
    mut incoming: mpsc::Receiver<RawFrame>,
    outgoing: mpsc::Sender<RawFrame>,
    connect: F,
) -> Ended
where
    C: AsyncRead + AsyncWrite + Send + 'static,
    F: FnOnce(&RawFrame) -> Fut,
    Fut: Future<Output = Result<C, Unavailable>>,
{
    let Some(first) = incoming.recv().await else {
        return Ended::BrowserClosed;
    };

    // Connect, holding frames that arrive meanwhile and watching for the browser
    // to leave.
    let connecting = connect(&first);
    let mut held = VecDeque::from([first]);
    tokio::pin!(connecting);
    let connected = loop {
        tokio::select! {
            biased;
            result = &mut connecting => break result,
            frame = incoming.recv(), if held.len() < MAX_HELD_WHILE_CONNECTING => match frame {
                Some(frame) => held.push_back(frame),
                None => {
                    log!("the browser closed the connection while Wirecatch was starting");
                    return Ended::BrowserClosed;
                }
            },
        }
    };

    match connected {
        Ok(conn) => relay(conn, held, incoming, outgoing).await,
        Err(unavailable) => {
            for frame in held {
                refuse(&frame, &outgoing, &unavailable).await;
            }
            while let Some(frame) = incoming.recv().await {
                refuse(&frame, &outgoing, &unavailable).await;
            }
            Ended::BrowserClosed
        }
    }
}

/// Answers a request with `AppUnavailable`; anything else is dropped.
async fn refuse(frame: &RawFrame, outgoing: &mpsc::Sender<RawFrame>, why: &Unavailable) {
    match frame.envelope() {
        Envelope::Request(Some(id)) => {
            answer_error(outgoing, id, why.code, &why.message).await;
        }
        _ => log!("dropped a message from the extension because Wirecatch is unavailable"),
    }
}

/// Relays over an established connection.
async fn relay<C>(
    conn: C,
    mut held: VecDeque<RawFrame>,
    mut incoming: mpsc::Receiver<RawFrame>,
    outgoing: mpsc::Sender<RawFrame>,
) -> Ended
where
    C: AsyncRead + AsyncWrite + Send + 'static,
{
    let pending: Pending = Arc::default();
    let (mut to_app, mut from_app) = start_reading(conn, pending.clone(), outgoing.clone());

    loop {
        let frame = match held.pop_front() {
            Some(frame) => frame,
            None => tokio::select! {
                frame = incoming.recv() => match frame {
                    Some(frame) => frame,
                    None => {
                        drop(to_app);
                        from_app.abort();
                        return Ended::BrowserClosed;
                    }
                },
                // The app side ended (closed, failed, or the browser stopped
                // reading). The reader task has finished, so everything it
                // received is already on its way to the browser.
                _ = &mut from_app => {
                    answer_pending(&pending, &outgoing).await;
                    return Ended::AppClosed;
                }
            },
        };

        if let Envelope::Request(Some(id)) = frame.envelope() {
            // Remember the id before sending, so a fast response always finds it.
            let fresh = lock(&pending).insert(id);
            if !fresh {
                log!("refused a request whose id {id} is already waiting for an answer");
                answer_error(
                    &outgoing,
                    id,
                    ErrorCode::InvalidRequest,
                    &format!(
                        "Request id {id} is already waiting for an answer. Use a new id for each request."
                    ),
                )
                .await;
                continue;
            }
        }
        if let Err(e) = send(&mut to_app, &frame).await {
            log!("lost the connection to Wirecatch while sending: {e}");
            drop(to_app);
            finish_reading(from_app).await;
            answer_pending(&pending, &outgoing).await;
            return Ended::AppClosed;
        }
    }
}

async fn send<W: AsyncWrite + Unpin>(to_app: &mut W, frame: &RawFrame) -> std::io::Result<()> {
    to_app.write_all(frame.as_bytes()).await?;
    to_app.flush().await
}

/// Splits the connection and starts the task that passes app frames to the
/// browser.
fn start_reading<C>(
    conn: C,
    pending: Pending,
    outgoing: mpsc::Sender<RawFrame>,
) -> (WriteHalf<C>, JoinHandle<()>)
where
    C: AsyncRead + AsyncWrite + Send + 'static,
{
    let (mut reader, to_app) = tokio::io::split(conn);
    let from_app = tokio::spawn(async move {
        loop {
            match raw_frame::read_async(&mut reader).await {
                Ok(Some(frame)) => {
                    let answered = match frame.envelope() {
                        Envelope::Response(Some(id)) => Some(id),
                        _ => None,
                    };
                    if outgoing.send(frame).await.is_err() {
                        // The browser side is gone.
                        return;
                    }
                    // Only after the response is queued for the browser, so a
                    // request is never both unanswered and forgotten.
                    if let Some(id) = answered {
                        lock(&pending).remove(&id);
                    }
                }
                Ok(None) => {
                    log!("Wirecatch closed the connection");
                    return;
                }
                Err(e) => {
                    log!("could not read from Wirecatch: {e}");
                    return;
                }
            }
        }
    });
    (to_app, from_app)
}

/// Lets the reader pass on what it already has, then stops it.
async fn finish_reading(mut from_app: JoinHandle<()>) {
    if tokio::time::timeout(Duration::from_secs(1), &mut from_app)
        .await
        .is_err()
    {
        from_app.abort();
        let _ = from_app.await;
    }
}

/// Answers every request still waiting with `AppUnavailable`.
async fn answer_pending(pending: &Pending, outgoing: &mpsc::Sender<RawFrame>) {
    let ids = std::mem::take(&mut *lock(pending));
    let why = Unavailable::lost();
    for id in ids {
        answer_error(outgoing, id, why.code, &why.message).await;
    }
}

async fn answer_error(outgoing: &mpsc::Sender<RawFrame>, id: u64, code: ErrorCode, message: &str) {
    let response = Message::Response {
        id,
        outcome: Outcome::Err(ProtocolError {
            code,
            message: message.to_string(),
        }),
    };
    match RawFrame::encode(&response) {
        // If the browser side is gone there is nobody left to tell.
        Ok(frame) => drop(outgoing.send(frame).await),
        Err(e) => log!("could not encode an error response: {e}"),
    }
}

fn lock(pending: &Mutex<BTreeSet<u64>>) -> MutexGuard<'_, BTreeSet<u64>> {
    // The set stays consistent even if a holder panicked mid-update (inserts and
    // removes are single operations).
    pending.lock().unwrap_or_else(PoisonError::into_inner)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};

    use tokio::io::{AsyncReadExt, DuplexStream};
    use wirecatch_protocol::wire::{Call, Event, Reply};

    fn request(id: u64) -> RawFrame {
        RawFrame::encode(&Message::Request {
            id,
            call: Call::PauseAll,
        })
        .unwrap()
    }

    fn raw(body: &str) -> RawFrame {
        let mut bytes = (body.len() as u32).to_le_bytes().to_vec();
        bytes.extend_from_slice(body.as_bytes());
        let mut cursor = std::io::Cursor::new(bytes);
        match raw_frame::read_blocking(&mut cursor).unwrap().unwrap() {
            raw_frame::Incoming::Frame(frame) => frame,
            other => panic!("{other:?}"),
        }
    }

    fn error(id: u64, code: ErrorCode, message: &str) -> RawFrame {
        RawFrame::encode(&Message::Response {
            id,
            outcome: Outcome::Err(ProtocolError {
                code,
                message: message.into(),
            }),
        })
        .unwrap()
    }

    fn unavailable(id: u64, why: &Unavailable) -> RawFrame {
        error(id, why.code, &why.message)
    }

    struct Harness {
        to_relay: mpsc::Sender<RawFrame>,
        from_relay: mpsc::Receiver<RawFrame>,
        relay: JoinHandle<Ended>,
    }

    fn harness_with<Fut>(connect: impl FnOnce() -> Fut + Send + 'static) -> Harness
    where
        Fut: Future<Output = Result<DuplexStream, Unavailable>> + Send + 'static,
    {
        let (to_relay, incoming) = mpsc::channel(8);
        let (outgoing, from_relay) = mpsc::channel(8);
        let relay = tokio::spawn(run(incoming, outgoing, |_: &RawFrame| connect()));
        Harness {
            to_relay,
            from_relay,
            relay,
        }
    }

    fn harness(app: Result<DuplexStream, Unavailable>, connected: Arc<AtomicBool>) -> Harness {
        harness_with(move || async move {
            connected.store(true, Ordering::SeqCst);
            app
        })
    }

    async fn read_frame(stream: &mut DuplexStream) -> RawFrame {
        raw_frame::read_async(stream).await.unwrap().unwrap()
    }

    #[tokio::test]
    async fn frames_pass_both_ways_unchanged() {
        let (bridge_side, mut app) = tokio::io::duplex(4096);
        let mut h = harness(Ok(bridge_side), Arc::default());

        let hello = raw(
            r#"{"kind":"request","id":1,"call":{"method":"hello","params":{"client":"extension","client_version":"1","protocol_version":1,"browser":null}},"unknown":"kept"}"#,
        );
        h.to_relay.send(hello.clone()).await.unwrap();
        assert_eq!(read_frame(&mut app).await, hello);

        let reply = raw(
            r#"{"kind":"response", "id":1,"outcome":{"status":"ok","value":{"type":"done"}},"newer_field":[1]}"#,
        );
        app.write_all(reply.as_bytes()).await.unwrap();
        assert_eq!(h.from_relay.recv().await.unwrap(), reply);

        let event = RawFrame::encode(&Message::Event {
            event: Event::QueuesChanged(Vec::new()),
        })
        .unwrap();
        app.write_all(event.as_bytes()).await.unwrap();
        assert_eq!(h.from_relay.recv().await.unwrap(), event);

        drop(h.to_relay);
        assert_eq!(h.relay.await.unwrap(), Ended::BrowserClosed);
        // The bridge closed its side of the app connection.
        let mut rest = Vec::new();
        app.read_to_end(&mut rest).await.unwrap();
        assert!(rest.is_empty());
    }

    #[tokio::test]
    async fn closing_stdin_first_ends_without_contacting_the_app() {
        let connected = Arc::new(AtomicBool::new(false));
        let (bridge_side, _app) = tokio::io::duplex(64);
        let h = harness(Ok(bridge_side), connected.clone());
        drop(h.to_relay);
        assert_eq!(h.relay.await.unwrap(), Ended::BrowserClosed);
        assert!(!connected.load(Ordering::SeqCst));
    }

    #[tokio::test]
    async fn closing_stdin_while_the_app_starts_ends_at_once() {
        // A connection attempt that never finishes.
        let h = harness_with(std::future::pending::<Result<DuplexStream, Unavailable>>);
        h.to_relay.send(request(1)).await.unwrap();
        h.to_relay.send(request(2)).await.unwrap();
        drop(h.to_relay);
        let ended = tokio::time::timeout(Duration::from_secs(2), h.relay)
            .await
            .expect("the relay must end while still connecting")
            .unwrap();
        assert_eq!(ended, Ended::BrowserClosed);
    }

    #[tokio::test]
    async fn frames_sent_while_the_app_starts_are_relayed_in_order() {
        let (bridge_side, mut app) = tokio::io::duplex(4096);
        let (release, wait) = tokio::sync::oneshot::channel::<()>();
        let h = harness_with(move || async move {
            let _ = wait.await;
            Ok(bridge_side)
        });
        for id in 1..=3 {
            h.to_relay.send(request(id)).await.unwrap();
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
        release.send(()).unwrap();
        for id in 1..=3 {
            assert_eq!(read_frame(&mut app).await, request(id));
        }
        drop(h.to_relay);
        assert_eq!(h.relay.await.unwrap(), Ended::BrowserClosed);
    }

    #[tokio::test]
    async fn every_request_gets_app_unavailable_when_the_first_connection_fails() {
        let why = Unavailable::timed_out(Duration::from_secs(5));
        let mut h = harness(Err(why.clone()), Arc::default());
        h.to_relay.send(request(1)).await.unwrap();
        h.to_relay
            .send(raw(r#"{"kind":"event","event":{}}"#))
            .await
            .unwrap();
        h.to_relay.send(request(2)).await.unwrap();
        assert_eq!(h.from_relay.recv().await.unwrap(), unavailable(1, &why));
        assert_eq!(h.from_relay.recv().await.unwrap(), unavailable(2, &why));
        // The bridge stays up (no relaunch loop) until the browser leaves.
        assert!(!h.relay.is_finished());
        drop(h.to_relay);
        assert_eq!(h.relay.await.unwrap(), Ended::BrowserClosed);
        assert!(h.from_relay.recv().await.is_none(), "nothing else was sent");
    }

    #[tokio::test]
    async fn the_relay_ends_after_answering_when_the_app_goes_away() {
        let (bridge_side, mut app) = tokio::io::duplex(4096);
        let mut h = harness(Ok(bridge_side), Arc::default());

        h.to_relay.send(request(1)).await.unwrap();
        h.to_relay.send(request(2)).await.unwrap();
        read_frame(&mut app).await;
        read_frame(&mut app).await;
        // The app answers 1, then disappears without answering 2.
        let answer = RawFrame::encode(&Message::Response {
            id: 1,
            outcome: Outcome::Ok(Reply::Done),
        })
        .unwrap();
        app.write_all(answer.as_bytes()).await.unwrap();
        drop(app);

        assert_eq!(h.from_relay.recv().await.unwrap(), answer);
        assert_eq!(
            h.from_relay.recv().await.unwrap(),
            unavailable(2, &Unavailable::lost())
        );
        // The relay ends on its own, so the bridge exits and the extension
        // reconnects (starting the app again) instead of being stuck.
        assert_eq!(h.relay.await.unwrap(), Ended::AppClosed);
        assert!(h.from_relay.recv().await.is_none());
    }

    #[tokio::test]
    async fn a_reused_id_in_flight_is_refused_and_the_first_still_answered() {
        let (bridge_side, mut app) = tokio::io::duplex(4096);
        let mut h = harness(Ok(bridge_side), Arc::default());

        h.to_relay.send(request(7)).await.unwrap();
        assert_eq!(read_frame(&mut app).await, request(7));
        h.to_relay.send(request(7)).await.unwrap();
        let refused = h.from_relay.recv().await.unwrap();
        let msg: Message = wirecatch_protocol::frame::decode(refused.body()).unwrap();
        assert!(
            matches!(
                msg,
                Message::Response {
                    id: 7,
                    outcome: Outcome::Err(ProtocolError {
                        code: ErrorCode::InvalidRequest,
                        ..
                    })
                }
            ),
            "{msg:?}"
        );

        // The duplicate never reached the app; the next frame it sees is 8.
        h.to_relay.send(request(8)).await.unwrap();
        assert_eq!(read_frame(&mut app).await, request(8));

        // Once 7 is answered, the id can be used again.
        let answer = RawFrame::encode(&Message::Response {
            id: 7,
            outcome: Outcome::Ok(Reply::Done),
        })
        .unwrap();
        app.write_all(answer.as_bytes()).await.unwrap();
        assert_eq!(h.from_relay.recv().await.unwrap(), answer);
        h.to_relay.send(request(7)).await.unwrap();
        assert_eq!(read_frame(&mut app).await, request(7));

        drop(h.to_relay);
        assert_eq!(h.relay.await.unwrap(), Ended::BrowserClosed);
    }
}
