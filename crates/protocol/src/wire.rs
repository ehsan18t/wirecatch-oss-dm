//! Messages on local connections: extension <-> bridge <-> app.
//!
//! Every frame carries one [`Message`]. A client sends [`Message::Request`]s; the
//! app answers each with a [`Message::Response`] carrying the same `id`, and may
//! push [`Message::Event`]s at any time. The first request on a connection must be
//! [`Call::Hello`].

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::QueueId;
use crate::extension::{
    AddedDownload, ExtensionSettings, ExtensionSettingsPatch, NewDownload, WithdrawOutcome,
    WithdrawRequest,
};

crate::exported::exported_consts! {
    /// Request id the app uses for an error it cannot tie to a request (for example an
    /// unreadable frame). Clients start their ids at 1.
    pub const UNKNOWN_REQUEST_ID: u64 = 0;
}

/// One frame on a local connection.
#[expect(
    clippy::large_enum_variant,
    reason = "messages are decoded, handled and dropped one at a time, so the size gap between variants costs nothing worth an extra allocation per request"
)]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[ts(export)]
pub enum Message {
    /// A client asks the app to do something; the app answers with one [`Message::Response`].
    Request {
        /// Chosen by the client, unique per connection, never [`UNKNOWN_REQUEST_ID`].
        #[ts(type = "number")]
        id: u64,
        /// What the client asks for.
        call: Call,
    },
    /// The app's answer to one request.
    Response {
        /// The id of the request this answers, or [`UNKNOWN_REQUEST_ID`] for an error the
        /// app cannot tie to a request.
        #[ts(type = "number")]
        id: u64,
        /// The reply, or why the call failed.
        outcome: Outcome,
    },
    /// News the app pushes without being asked.
    Event {
        /// What changed.
        event: Event,
    },
}

impl Message {
    /// Which kind of message this is, as its `kind` field names it on the wire.
    #[must_use]
    pub fn kind(&self) -> MessageKind {
        match self {
            Message::Request { .. } => MessageKind::Request,
            Message::Response { .. } => MessageKind::Response,
            Message::Event { .. } => MessageKind::Event,
        }
    }
}

/// The kinds of [`Message`], named on the wire exactly as its variants are.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MessageKind {
    /// A [`Message::Request`].
    Request,
    /// A [`Message::Response`].
    Response,
    /// A [`Message::Event`].
    Event,
}

/// The fields every [`Message`] is told apart by, read without decoding what it carries, for a
/// relay that passes frames on as they are.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
pub struct Envelope {
    /// What kind of message it is.
    pub kind: MessageKind,
    /// The request id of a request or a response; `None` for an event.
    #[serde(default)]
    pub id: Option<u64>,
}

impl Envelope {
    /// The envelope of a frame body, or `None` when the body is not a message this build
    /// knows the kind of, or its id is not a request id.
    #[must_use]
    pub fn read(body: &[u8]) -> Option<Envelope> {
        serde_json::from_slice(body).ok()
    }
}

/// What a client asks the app to do.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(tag = "method", content = "params", rename_all = "snake_case")]
#[ts(export)]
pub enum Call {
    /// Opens the conversation: says who the client is and which protocol it speaks. Must be
    /// the first call on a connection; answered with [`Reply::Hello`].
    Hello(Hello),
    /// Hands over a download captured in the browser; answered with [`Reply::DownloadAdded`].
    AddDownload(NewDownload),
    /// Asks the app to forget a handover the extension gave up waiting for; answered with
    /// [`Reply::Withdrawn`].
    WithdrawDownload(WithdrawRequest),
    /// Asks for the settings the extension shows; answered with [`Reply::ExtensionSettings`].
    GetExtensionSettings,
    /// Changes some of the extension's settings; answered with the new
    /// [`Reply::ExtensionSettings`], and every client also gets
    /// [`Event::ExtensionSettingsChanged`].
    UpdateExtensionSettings(ExtensionSettingsPatch),
    /// Asks for the queues a download can be added to; answered with [`Reply::Queues`].
    ListQueues,
    /// Pauses every download that is running or waiting; answered with [`Reply::Done`].
    PauseAll,
    /// Puts every paused download back in line; answered with [`Reply::Done`].
    ResumeAll,
    /// Brings the app's main window to the front, starting the app first like any call (the extension's "Open Wirecatch").
    ShowWindow,
}

/// How a [`Call`] ended.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(tag = "status", content = "value", rename_all = "snake_case")]
#[ts(export)]
pub enum Outcome {
    /// The call succeeded.
    Ok(Reply),
    /// The call failed.
    Err(ProtocolError),
}

/// The successful answer to a [`Call`]; the variant matches the call.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(tag = "type", content = "data", rename_all = "snake_case")]
#[ts(export)]
pub enum Reply {
    /// Answers [`Call::Hello`].
    Hello(HelloReply),
    /// Answers [`Call::AddDownload`].
    DownloadAdded(AddedDownload),
    /// Answers [`Call::WithdrawDownload`].
    Withdrawn(WithdrawOutcome),
    /// Answers [`Call::GetExtensionSettings`] and [`Call::UpdateExtensionSettings`] with the
    /// settings as they are now.
    ExtensionSettings(ExtensionSettings),
    /// Answers [`Call::ListQueues`], in sidebar order.
    Queues(Vec<QueueSummary>),
    /// For calls with nothing to return.
    Done,
}

/// Pushed by the app without a request.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(tag = "type", content = "data", rename_all = "snake_case")]
#[ts(export)]
pub enum Event {
    /// Sent to every connected client whenever these settings change, from any side.
    ExtensionSettingsChanged(ExtensionSettings),
    /// Sent to every connected client whenever the queues change: the whole list, in sidebar
    /// order.
    QueuesChanged(Vec<QueueSummary>),
}

/// The first call on a connection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Hello {
    /// What kind of program is calling.
    pub client: ClientKind,
    /// The client's own version; the app writes it to its log.
    pub client_version: String,
    /// The [`PROTOCOL_VERSION`](crate::PROTOCOL_VERSION) the client speaks; the app refuses
    /// one it does not speak with [`ErrorCode::UnsupportedVersion`].
    pub protocol_version: u32,
    /// Set by the extension; the bridge passes it through.
    pub browser: Option<BrowserInfo>,
    /// Whether the bridge may start the app when it is not running. The extension sets it only when the person did something that needs the app, such as a download from the browser; its checks on waking and its reconnects leave it off, so a Wirecatch the person closed stays closed (D25). Missing means yes, as before.
    #[serde(default = "start_app_by_default")]
    pub start_app: bool,
}

fn start_app_by_default() -> bool {
    true
}

/// The kind of program on the other end of a connection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum ClientKind {
    /// The browser extension, through the bridge.
    Extension,
    /// The native messaging bridge, connecting on its own behalf.
    Bridge,
    /// The `wirecatch` command-line tool.
    Cli,
}

/// The browser the extension runs in.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct BrowserInfo {
    /// For example `chrome`, `edge`, `brave`, `firefox`.
    pub name: String,
    /// The browser's version, when the extension can tell.
    pub version: Option<String>,
}

/// The app's answer to [`Call::Hello`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct HelloReply {
    /// The app's version.
    pub app_version: String,
    /// The [`PROTOCOL_VERSION`](crate::PROTOCOL_VERSION) the app speaks.
    pub protocol_version: u32,
}

/// A queue as clients outside the app see it: enough to pick one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct QueueSummary {
    /// The queue to name in [`NewDownload::queue`].
    pub id: QueueId,
    /// The name the person gave it.
    pub name: String,
    /// Downloads go to this queue when none is named.
    pub is_default: bool,
}

/// Why a [`Call`] failed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ProtocolError {
    /// What kind of failure it is, for the client to act on; never decide from `message`.
    pub code: ErrorCode,
    /// Plain-language explanation, safe to show to a user.
    pub message: String,
}

/// Shows the message, which is written for a person.
impl std::fmt::Display for ProtocolError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for ProtocolError {}

/// The kinds of failure a client can tell apart.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum ErrorCode {
    /// The client speaks a protocol version the app does not.
    UnsupportedVersion,
    /// The first call was not `hello`, or the message made no sense.
    InvalidRequest,
    /// The download's address is not one the app can download from.
    InvalidUrl,
    /// The download, queue or pending confirmation the call names does not exist.
    NotFound,
    /// Produced by the bridge when it cannot reach or start the app.
    AppUnavailable,
    /// Produced by the bridge when the app is not running and the client asked not to start it (`Hello::start_app` off).
    AppNotRunning,
    /// Something failed inside the app, such as its database or a file it keeps, or the app
    /// is shutting down.
    Internal,
    /// The app could not take over a captured download (the server did not answer
    /// in time, or sent something other than what the browser saw). The browser
    /// keeps its own download.
    NotStarted,
    /// The choice needs the media add-on, which is not installed. The app has
    /// already offered it ([`UiEvent::AddonRequired`](crate::app::UiEvent::AddonRequired)); the client keeps the
    /// choice open and goes on once the add-on is in.
    AddonRequired,
    /// The app already holds as many of something as it may (downloads waiting for an answer,
    /// queues, requests from one connection); the message says which. Trying again after some
    /// are answered or removed works.
    LimitReached,
    /// The download, queue or add-on the call names is not in a state the call applies to (a
    /// finished download cannot be cancelled, say); the message says what to do instead.
    WrongState,
    /// Looking for a newer add-on failed: its releases could not be reached, or a newer
    /// release is not signed by its developers. Nothing changed; trying again later may work.
    UpdateCheckFailed,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// The envelope reads the same `kind` and `id` that a whole message is written with, for
    /// every kind of message.
    #[test]
    fn the_envelope_of_every_message_matches_the_message() {
        let messages = [
            Message::Request {
                id: 3,
                call: Call::PauseAll,
            },
            Message::Response {
                id: 4,
                outcome: Outcome::Ok(Reply::Done),
            },
            Message::Event {
                event: Event::QueuesChanged(Vec::new()),
            },
        ];
        for message in messages {
            let body = serde_json::to_vec(&message).unwrap();
            let id = match &message {
                Message::Request { id, .. } | Message::Response { id, .. } => Some(*id),
                Message::Event { .. } => None,
            };
            assert_eq!(
                Envelope::read(&body),
                Some(Envelope {
                    kind: message.kind(),
                    id
                }),
                "{message:?}"
            );
        }
        assert_eq!(Envelope::read(br#"{"kind":"request","id":-1}"#), None);
        assert_eq!(Envelope::read(br#"{"kind":"future","id":1}"#), None);
        assert_eq!(Envelope::read(b"not json"), None);
    }

    #[test]
    fn wire_shape_is_stable() {
        let msg = Message::Request {
            id: 1,
            call: Call::PauseAll,
        };
        assert_eq!(
            serde_json::to_value(&msg).unwrap(),
            json!({"kind": "request", "id": 1, "call": {"method": "pause_all"}})
        );

        let msg = Message::Response {
            id: 1,
            outcome: Outcome::Err(ProtocolError {
                code: ErrorCode::NotFound,
                message: "gone".into(),
            }),
        };
        assert_eq!(
            serde_json::to_value(&msg).unwrap(),
            json!({"kind": "response", "id": 1, "outcome": {"status": "err", "value": {"code": "not_found", "message": "gone"}}})
        );

        let msg = Message::Response {
            id: 2,
            outcome: Outcome::Ok(Reply::Done),
        };
        assert_eq!(
            serde_json::to_value(&msg).unwrap(),
            json!({"kind": "response", "id": 2, "outcome": {"status": "ok", "value": {"type": "done"}}})
        );
    }
}
