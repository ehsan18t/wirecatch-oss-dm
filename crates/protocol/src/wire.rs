//! Messages on local connections: extension <-> bridge <-> app.
//!
//! Every frame carries one [`Message`]. A client sends [`Message::Request`]s; the
//! app answers each with a [`Message::Response`] carrying the same `id`, and may
//! push [`Message::Event`]s at any time. The first request on a connection must be
//! [`Call::Hello`], which settles the protocol version the connection speaks (D32): from then on
//! each side sends only calls, events and error codes that version has.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::QueueId;
use crate::extension::{
    AddedDownload, ExtensionSettings, ExtensionSettingsPatch, NewDownload, WithdrawOutcome,
    WithdrawRequest,
};
use crate::version::{VersionError, VersionRange};

crate::exported::exported_consts! {
    /// Request id the app uses for an error it cannot tie to a request (for example an
    /// unreadable frame). Clients start their ids at 1.
    pub const UNKNOWN_REQUEST_ID: u64 = 0;

    /// How long the bridge waits for the app, in milliseconds, from its first connection
    /// attempt to giving up, starting the app included. A client that may make the bridge
    /// start the app waits longer than this for the answer to `hello`.
    pub const BRIDGE_START_BUDGET_MS: u64 = 5_000;
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

impl Call {
    /// The method this call names.
    #[must_use]
    pub fn method(&self) -> CallMethod {
        match self {
            Call::Hello(_) => CallMethod::Hello,
            Call::AddDownload(_) => CallMethod::AddDownload,
            Call::WithdrawDownload(_) => CallMethod::WithdrawDownload,
            Call::GetExtensionSettings => CallMethod::GetExtensionSettings,
            Call::UpdateExtensionSettings(_) => CallMethod::UpdateExtensionSettings,
            Call::ListQueues => CallMethod::ListQueues,
            Call::PauseAll => CallMethod::PauseAll,
            Call::ResumeAll => CallMethod::ResumeAll,
            Call::ShowWindow => CallMethod::ShowWindow,
        }
    }
}

/// The method a [`Call`] names, read without its parameters, so that a request the app cannot
/// read whole still tells a method the app does not have from a call it has but cannot read.
/// [`CallMethod::since`] says which protocol version added each.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CallMethod {
    /// [`Call::Hello`].
    Hello,
    /// [`Call::AddDownload`].
    AddDownload,
    /// [`Call::WithdrawDownload`].
    WithdrawDownload,
    /// [`Call::GetExtensionSettings`].
    GetExtensionSettings,
    /// [`Call::UpdateExtensionSettings`].
    UpdateExtensionSettings,
    /// [`Call::ListQueues`].
    ListQueues,
    /// [`Call::PauseAll`].
    PauseAll,
    /// [`Call::ResumeAll`].
    ResumeAll,
    /// [`Call::ShowWindow`].
    ShowWindow,
    /// A method this build does not have, from a newer client.
    #[serde(other)]
    Unknown,
}

/// The id and method of a request, read without the call's parameters (see [`CallMethod`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RequestHead {
    /// The request's id.
    pub id: u64,
    /// The method it names.
    pub method: CallMethod,
}

impl RequestHead {
    /// The head of a frame body that is a request with a usable id and a method name, or `None`
    /// for anything else.
    #[must_use]
    pub fn read(body: &[u8]) -> Option<RequestHead> {
        #[derive(Deserialize)]
        struct Head {
            kind: MessageKind,
            id: u64,
            call: CallHead,
        }
        #[derive(Deserialize)]
        struct CallHead {
            method: CallMethod,
        }
        let head: Head = serde_json::from_slice(body).ok()?;
        (head.kind == MessageKind::Request).then_some(RequestHead {
            id: head.id,
            method: head.call.method,
        })
    }
}

/// Whether a `hello` allows the app to be started, read without the rest of the hello, so a hello this build cannot read whole (a newer client kind, a field typed differently) is still honoured.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HelloHead {
    /// [`Hello::start_app`].
    pub start_app: bool,
}

impl HelloHead {
    /// The head of a frame body that is a `hello` request, or `None` for anything else, also for a hello whose `start_app` is not a boolean.
    #[must_use]
    pub fn read(body: &[u8]) -> Option<HelloHead> {
        #[derive(Deserialize)]
        struct Head {
            kind: MessageKind,
            call: CallHead,
        }
        #[derive(Deserialize)]
        struct CallHead {
            method: CallMethod,
            params: Params,
        }
        #[derive(Deserialize)]
        struct Params {
            #[serde(default = "start_app_by_default")]
            start_app: bool,
        }
        let head: Head = serde_json::from_slice(body).ok()?;
        (head.kind == MessageKind::Request && head.call.method == CallMethod::Hello).then_some(
            HelloHead {
                start_app: head.call.params.start_app,
            },
        )
    }
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

impl Event {
    /// Which event this is.
    #[must_use]
    pub fn kind(&self) -> EventKind {
        match self {
            Event::ExtensionSettingsChanged(_) => EventKind::ExtensionSettingsChanged,
            Event::QueuesChanged(_) => EventKind::QueuesChanged,
        }
    }
}

/// The kinds of [`Event`], named on the wire as its `type` names them. [`EventKind::since`]
/// says which protocol version added each.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EventKind {
    /// [`Event::ExtensionSettingsChanged`].
    ExtensionSettingsChanged,
    /// [`Event::QueuesChanged`].
    QueuesChanged,
}

/// The first call on a connection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Hello {
    /// What kind of program is calling.
    pub client: ClientKind,
    /// The client's own version; the app writes it to its log.
    pub client_version: String,
    /// The oldest protocol version the client speaks ([`MIN_PROTOCOL_VERSION`](crate::MIN_PROTOCOL_VERSION)). Apps from version 1 read only this field and accept a client only when it is 1, so a client that still speaks version 1 must name it here (D32).
    pub protocol_version: u32,
    /// The newest protocol version the client speaks ([`PROTOCOL_VERSION`](crate::PROTOCOL_VERSION)). Missing means the same as `protocol_version`, as clients from version 1 send it. The app answers with the newest version both speak, or refuses with [`ErrorCode::UnsupportedVersion`] a client it shares no version with.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub max_protocol_version: Option<u32>,
    /// Set by the extension; the bridge passes it through.
    pub browser: Option<BrowserInfo>,
    /// Whether the bridge may start the app when it is not running. The extension sets it only when the person did something that needs the app, such as a download from the browser; its checks on waking and its reconnects leave it off, so a Wirecatch the person closed stays closed (D25). Missing means yes, as before.
    #[serde(default = "start_app_by_default")]
    pub start_app: bool,
}

fn start_app_by_default() -> bool {
    true
}

impl Hello {
    /// The protocol versions the client speaks.
    ///
    /// # Errors
    ///
    /// [`VersionError::Empty`] when the newest version it names is older than the oldest.
    pub fn versions(&self) -> Result<VersionRange, VersionError> {
        VersionRange::new(
            self.protocol_version,
            self.max_protocol_version.unwrap_or(self.protocol_version),
        )
    }
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
    /// The protocol version the connection speaks from now on: the newest version both the
    /// client and the app speak. A client from version 1 accepts only 1 here, and gets it.
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
    /// The app does not have the call: it names a method the app does not know, or one newer
    /// than the protocol version the connection speaks. Nothing was done, and the connection
    /// stays open for every other call. Added in protocol version 2; a connection on version 1
    /// gets [`ErrorCode::InvalidRequest`] instead.
    UnsupportedCall,
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

    /// A call naming `method`, or `None` for [`CallMethod::Unknown`]. A match, so a new method
    /// cannot be left out.
    fn sample_call(method: CallMethod) -> Option<Call> {
        Some(match method {
            CallMethod::Hello => Call::Hello(hello(1, Some(2))),
            CallMethod::AddDownload => Call::AddDownload(NewDownload {
                handover_id: "h-1".into(),
                url: "https://example.com/a.zip".into(),
                referrer: None,
                page_url: None,
                suggested_name: None,
                mime: None,
                size_hint: None,
                cookies: None,
                user_agent: None,
                extra_headers: Vec::new(),
                source: crate::extension::CaptureSource::BrowserDownload,
                queue: None,
                media: None,
            }),
            CallMethod::WithdrawDownload => Call::WithdrawDownload(WithdrawRequest {
                handover_id: "h-1".into(),
            }),
            CallMethod::GetExtensionSettings => Call::GetExtensionSettings,
            CallMethod::UpdateExtensionSettings => {
                Call::UpdateExtensionSettings(ExtensionSettingsPatch::default())
            }
            CallMethod::ListQueues => Call::ListQueues,
            CallMethod::PauseAll => Call::PauseAll,
            CallMethod::ResumeAll => Call::ResumeAll,
            CallMethod::ShowWindow => Call::ShowWindow,
            CallMethod::Unknown => return None,
        })
    }

    /// An event of `kind`. A match, so a new kind cannot be left out.
    fn sample_event(kind: EventKind) -> Event {
        match kind {
            EventKind::ExtensionSettingsChanged => {
                Event::ExtensionSettingsChanged(ExtensionSettings::default())
            }
            EventKind::QueuesChanged => Event::QueuesChanged(Vec::new()),
        }
    }

    fn hello(protocol_version: u32, max_protocol_version: Option<u32>) -> Hello {
        Hello {
            client: ClientKind::Extension,
            client_version: "1".into(),
            protocol_version,
            max_protocol_version,
            browser: None,
            start_app: true,
        }
    }

    /// Every method this build has reads back from a request as itself, under the name its call
    /// is written with, so a call this build has is never mistaken for one it does not.
    #[test]
    fn every_known_method_reads_as_itself_under_its_calls_name() {
        for &method in CallMethod::KNOWN {
            let call = sample_call(method).expect("a known method has a call");
            assert_eq!(call.method(), method);
            let body = serde_json::to_value(Message::Request { id: 7, call }).unwrap();
            assert_eq!(
                body["call"]["method"],
                serde_json::to_value(method).unwrap()
            );
            let body = serde_json::to_vec(&body).unwrap();
            assert_eq!(
                RequestHead::read(&body),
                Some(RequestHead { id: 7, method }),
                "{method:?}"
            );
        }
        assert_eq!(sample_call(CallMethod::Unknown), None);
        for &kind in EventKind::KNOWN {
            let event = serde_json::to_value(sample_event(kind)).unwrap();
            assert_eq!(event["type"], serde_json::to_value(kind).unwrap());
        }
    }

    #[test]
    fn a_hello_says_whether_the_app_may_be_started_even_when_the_rest_is_unreadable() {
        let hello = |params: &str| {
            HelloHead::read(
                format!(
                    r#"{{"kind":"request","id":1,"call":{{"method":"hello","params":{params}}}}}"#
                )
                .as_bytes(),
            )
        };
        let start_app = |start_app| Some(HelloHead { start_app });
        assert_eq!(
            hello(r#"{"client":"phone","start_app":false}"#),
            start_app(false)
        );
        assert_eq!(
            hello(r#"{"client":"extension","start_app":true}"#),
            start_app(true)
        );
        assert_eq!(hello("{}"), start_app(true));
        assert_eq!(hello(r#"{"start_app":"no"}"#), None);
        assert_eq!(
            HelloHead::read(br#"{"kind":"request","id":2,"call":{"method":"pause_all"}}"#),
            None
        );
        assert_eq!(HelloHead::read(b"not json"), None);
    }

    #[test]
    fn a_method_this_build_lacks_is_told_from_a_call_it_cannot_read() {
        let unknown =
            br#"{"kind":"request","id":5,"call":{"method":"teleport","params":{"to":"mars"}}}"#;
        assert!(serde_json::from_slice::<Message>(unknown).is_err());
        assert_eq!(
            RequestHead::read(unknown),
            Some(RequestHead {
                id: 5,
                method: CallMethod::Unknown
            })
        );
        assert!(!CallMethod::Unknown.is_in(u32::MAX));

        let unreadable =
            br#"{"kind":"request","id":6,"call":{"method":"add_download","params":{"url":3}}}"#;
        assert!(serde_json::from_slice::<Message>(unreadable).is_err());
        assert_eq!(
            RequestHead::read(unreadable),
            Some(RequestHead {
                id: 6,
                method: CallMethod::AddDownload
            })
        );

        for no_head in [
            &br#"{"kind":"request","call":{"method":"teleport"}}"#[..],
            br#"{"kind":"request","id":-1,"call":{"method":"teleport"}}"#,
            br#"{"kind":"request","id":"5","call":{"method":"teleport"}}"#,
            br#"{"kind":"request","id":5,"call":{}}"#,
            br#"{"kind":"request","id":5,"call":{"method":null}}"#,
            br#"{"kind":"response","id":5,"call":{"method":"teleport"}}"#,
            b"not json",
        ] {
            assert_eq!(
                RequestHead::read(no_head),
                None,
                "{}",
                String::from_utf8_lossy(no_head)
            );
        }
    }

    /// A hello as clients from version 1 write it still reads, and names only that version.
    #[test]
    fn a_hello_without_a_newest_version_speaks_one_version() {
        let old = br#"{"client":"extension","client_version":"0.1.0","protocol_version":1,"browser":null}"#;
        let read: Hello = serde_json::from_slice(old).unwrap();
        assert_eq!(read.max_protocol_version, None);
        assert_eq!(read.versions(), VersionRange::new(1, 1));

        let without = serde_json::to_value(hello(1, None)).unwrap();
        assert!(without.get("max_protocol_version").is_none(), "{without}");
        let with = serde_json::to_value(hello(1, Some(2))).unwrap();
        assert_eq!(with["protocol_version"], json!(1));
        assert_eq!(with["max_protocol_version"], json!(2));
        assert_eq!(hello(1, Some(2)).versions(), VersionRange::new(1, 2));
    }

    #[test]
    fn a_hello_with_impossible_versions_is_refused() {
        assert_eq!(
            hello(3, Some(2)).versions(),
            Err(VersionError::Empty {
                oldest: 3,
                newest: 2
            })
        );
        for version in [
            r#""protocol_version":-1"#,
            r#""protocol_version":4294967296"#,
            r#""protocol_version":"2""#,
            r#""protocol_version":1.5"#,
            r#""protocol_version":null"#,
            r#""protocol_version":1,"max_protocol_version":-2"#,
            r#""protocol_version":1,"max_protocol_version":1e100"#,
            r#""max_protocol_version":2"#,
        ] {
            let body = format!(
                r#"{{"client":"extension","client_version":"1",{version},"browser":null}}"#
            );
            assert!(serde_json::from_str::<Hello>(&body).is_err(), "{body}");
        }
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
