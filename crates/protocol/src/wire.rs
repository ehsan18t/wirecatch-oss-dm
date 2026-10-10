//! Messages on local connections: extension <-> bridge <-> app.
//!
//! Every frame carries one [`Message`]. A client sends [`Message::Request`]s; the
//! app answers each with a [`Message::Response`] carrying the same `id`, and may
//! push [`Message::Event`]s at any time. The first request on a connection must be
//! [`Call::Hello`].

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::QueueId;
use crate::extension::{AddedDownload, ExtensionSettings, ExtensionSettingsPatch, NewDownload};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[ts(export)]
pub enum Message {
    Request {
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
    GetExtensionSettings,
    UpdateExtensionSettings(ExtensionSettingsPatch),
    ListQueues,
    PauseAll,
    ResumeAll,
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
    Internal,
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
