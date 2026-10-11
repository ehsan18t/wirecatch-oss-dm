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

/// Request id the app uses for an error it cannot tie to a request (for example an
/// unreadable frame). Clients start their ids at 1.
pub const UNKNOWN_REQUEST_ID: u64 = 0;

// Messages are decoded, handled and dropped one at a time, so the size gap
// between variants costs nothing worth an extra allocation per request.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[ts(export)]
pub enum Message {
    Request {
        /// Chosen by the client, unique per connection, never [`UNKNOWN_REQUEST_ID`].
        #[ts(type = "number")]
        id: u64,
        call: Call,
    },
    Response {
        #[ts(type = "number")]
        id: u64,
        outcome: Outcome,
    },
    Event {
        event: Event,
    },
}

/// What a client asks the app to do.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(tag = "method", content = "params", rename_all = "snake_case")]
#[ts(export)]
pub enum Call {
    Hello(Hello),
    AddDownload(NewDownload),
    WithdrawDownload(WithdrawRequest),
    GetExtensionSettings,
    UpdateExtensionSettings(ExtensionSettingsPatch),
    ListQueues,
    PauseAll,
    ResumeAll,
    /// Brings the app's main window to the front, starting the app first like any call (the extension's "Open Wirecatch").
    ShowWindow,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(tag = "status", content = "value", rename_all = "snake_case")]
#[ts(export)]
pub enum Outcome {
    Ok(Reply),
    Err(ProtocolError),
}

/// The successful answer to a [`Call`]; the variant matches the call.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(tag = "type", content = "data", rename_all = "snake_case")]
#[ts(export)]
pub enum Reply {
    Hello(HelloReply),
    DownloadAdded(AddedDownload),
    Withdrawn(WithdrawOutcome),
    ExtensionSettings(ExtensionSettings),
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
    QueuesChanged(Vec<QueueSummary>),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Hello {
    pub client: ClientKind,
    pub client_version: String,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum ClientKind {
    Extension,
    Bridge,
    Cli,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct BrowserInfo {
    /// For example `chrome`, `edge`, `brave`, `firefox`.
    pub name: String,
    pub version: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct HelloReply {
    pub app_version: String,
    pub protocol_version: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct QueueSummary {
    pub id: QueueId,
    pub name: String,
    pub is_default: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ProtocolError {
    pub code: ErrorCode,
    /// Plain-language explanation, safe to show to a user.
    pub message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum ErrorCode {
    /// The client speaks a protocol version the app does not.
    UnsupportedVersion,
    /// The first call was not `hello`, or the message made no sense.
    InvalidRequest,
    InvalidUrl,
    NotFound,
    /// Produced by the bridge when it cannot reach or start the app.
    AppUnavailable,
    /// Produced by the bridge when the app is not running and the client asked not to start it (`Hello::start_app` off).
    AppNotRunning,
    Internal,
    /// The app could not take over a captured download (the server did not answer
    /// in time, or sent something other than what the browser saw). The browser
    /// keeps its own download.
    NotStarted,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

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
