//! Relays frames between the browser and the app.
//!
//! Rules, in the order they apply:
//!
//! 1. Nothing happens until the extension sends its first frame; that frame
//!    triggers the connection to the app (starting the app if needed).
//! 2. While the app is connected, every frame from the extension is passed to the
//!    app and every frame from the app is passed to the extension, byte for byte.
//!    The ids of requests still waiting for a response are remembered.
//! 3. If the app cannot be reached, or the connection ends, every request still
//!    waiting and every later request is answered with `AppUnavailable`, so the
//!    extension never waits in silence. The bridge does not reconnect on its own,
//!    because a new app connection must start with `hello`; the extension
//!    reconnects (which starts a new bridge) when it wants to try again.
//! 4. When the browser closes stdin, the relay ends.

use std::collections::BTreeSet;
use std::future::Future;
use std::mem;
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

enum State<C> {
    /// Waiting for the first frame.
    NotConnected,
    Connected {
        to_app: WriteHalf<C>,
        from_app: JoinHandle<()>,
    },
    /// The app is out of reach for the rest of this session.
    Gone(Unavailable),
}

/// Runs the relay until `incoming` (frames from the browser) closes.
///
/// `outgoing` carries frames to the browser. `connect` is called at most once, on
/// the first frame.
pub(crate) async fn run<C, F, Fut>(
    mut incoming: mpsc::Receiver<RawFrame>,
    outgoing: mpsc::Sender<RawFrame>,
    connect: F,
) where
    C: AsyncRead + AsyncWrite + Send + 'static,
    F: FnOnce() -> Fut,
    Fut: Future<Output = Result<C, Unavailable>>,
{
    let pending: Pending = Arc::default();
    let mut connect = Some(connect);
    let mut state = State::<C>::NotConnected;

    loop {
        let next = match &mut state {
            State::Connected { from_app, .. } => tokio::select! {
                frame = incoming.recv() => Some(frame),
                _ = from_app => None,
            },
            _ => Some(incoming.recv().await),
        };
        let frame = match next {
            // The app side ended (closed, failed, or the browser stopped reading).
            // The reader task has finished, so everything it received is already
            // on its way to the browser.
            None => {
                state = State::Gone(Unavailable::LOST);
                answer_pending(&pending, &outgoing, Unavailable::LOST).await;
                continue;
            }
            Some(None) => break,
            Some(Some(frame)) => frame,
        };

        if let Some(connect) = connect.take() {
            state = match connect().await {
                Ok(conn) => start_reading(conn, pending.clone(), outgoing.clone()),
                Err(unavailable) => State::Gone(unavailable),
            };
        }

        let envelope = frame.envelope();
        match &mut state {
            State::Connected { to_app, .. } => {
                // Remember the id before sending, so a fast response always finds
                // it.
                if let Envelope::Request(Some(id)) = envelope {
                    lock(&pending).insert(id);
                }
                if let Err(e) = send(to_app, &frame).await {
                    log!("lost the connection to Wirecatch while sending: {e}");
                    if let State::Connected { to_app, from_app } =
                        mem::replace(&mut state, State::Gone(Unavailable::LOST))
                    {
                        drop(to_app);
                        finish_reading(from_app).await;
                    }
                    answer_pending(&pending, &outgoing, Unavailable::LOST).await;
                }
            }
            State::Gone(unavailable) => match envelope {
                Envelope::Request(Some(id)) => {
                    answer_unavailable(&outgoing, id, unavailable.message).await;
                }
                _ => log!("dropped a message from the extension because Wirecatch is unavailable"),
            },
            State::NotConnected => {
                // `connect` was taken above, so the state has moved on.
                log!("internal error: the relay has no connection state");
            }
        }
    }

    if let State::Connected { to_app, from_app } = state {
        drop(to_app);
        from_app.abort();
    }
}

async fn send<W: AsyncWrite + Unpin>(to_app: &mut W, frame: &RawFrame) -> std::io::Result<()> {
    to_app.write_all(frame.as_bytes()).await?;
    to_app.flush().await
}

/// Splits the connection and starts the task that passes app frames to the
/// browser.
fn start_reading<C>(conn: C, pending: Pending, outgoing: mpsc::Sender<RawFrame>) -> State<C>
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
    State::Connected { to_app, from_app }
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

async fn answer_pending(pending: &Pending, outgoing: &mpsc::Sender<RawFrame>, why: Unavailable) {
    let ids = mem::take(&mut *lock(pending));
    for id in ids {
        answer_unavailable(outgoing, id, why.message).await;
    }
}

async fn answer_unavailable(outgoing: &mpsc::Sender<RawFrame>, id: u64, message: &str) {
    let response = Message::Response {
        id,
        outcome: Outcome::Err(ProtocolError {
            code: ErrorCode::AppUnavailable,
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
        raw_frame::read_blocking(&mut cursor).unwrap().unwrap()
    }

    fn unavailable(id: u64, why: Unavailable) -> RawFrame {
        RawFrame::encode(&Message::Response {
            id,
            outcome: Outcome::Err(ProtocolError {
                code: ErrorCode::AppUnavailable,
                message: why.message.into(),
            }),
        })
        .unwrap()
    }

    struct Harness {
        to_relay: mpsc::Sender<RawFrame>,
        from_relay: mpsc::Receiver<RawFrame>,
        relay: JoinHandle<()>,
    }

    fn harness(app: Result<DuplexStream, Unavailable>, connected: Arc<AtomicBool>) -> Harness {
        let (to_relay, incoming) = mpsc::channel(8);
        let (outgoing, from_relay) = mpsc::channel(8);
        let relay = tokio::spawn(run(incoming, outgoing, move || async move {
            connected.store(true, Ordering::SeqCst);
            app
        }));
        Harness {
            to_relay,
            from_relay,
            relay,
        }
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
        h.relay.await.unwrap();
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
        h.relay.await.unwrap();
        assert!(!connected.load(Ordering::SeqCst));
    }

    #[tokio::test]
    async fn every_request_gets_app_unavailable_when_the_app_cannot_be_reached() {
        let mut h = harness(Err(Unavailable::NOT_STARTED), Arc::default());
        h.to_relay.send(request(1)).await.unwrap();
        h.to_relay
            .send(raw(r#"{"kind":"event","event":{}}"#))
            .await
            .unwrap();
        h.to_relay.send(request(2)).await.unwrap();
        assert_eq!(
            h.from_relay.recv().await.unwrap(),
            unavailable(1, Unavailable::NOT_STARTED)
        );
        assert_eq!(
            h.from_relay.recv().await.unwrap(),
            unavailable(2, Unavailable::NOT_STARTED)
        );
        drop(h.to_relay);
        h.relay.await.unwrap();
        assert!(h.from_relay.recv().await.is_none(), "nothing else was sent");
    }

    #[tokio::test]
    async fn requests_in_flight_are_answered_when_the_app_goes_away() {
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
            unavailable(2, Unavailable::LOST)
        );
        // Later requests are answered too, instead of being dropped.
        h.to_relay.send(request(3)).await.unwrap();
        assert_eq!(
            h.from_relay.recv().await.unwrap(),
            unavailable(3, Unavailable::LOST)
        );
        drop(h.to_relay);
        h.relay.await.unwrap();
        assert!(h.from_relay.recv().await.is_none());
    }
}
