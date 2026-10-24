//! Which protocol versions the two ends of a local connection speak, and the one they agree on
//! (D32).
//!
//! Every build speaks each version from [`MIN_PROTOCOL_VERSION`] to [`PROTOCOL_VERSION`]. A
//! client names the versions it speaks in its [`Hello`](crate::wire::Hello); the app answers with
//! the newest version both speak, which the connection then uses, or refuses the client when they
//! share none.

use thiserror::Error;

use crate::wire::{CallMethod, ErrorCode, Event, EventKind, Message, Outcome};

crate::exported::exported_consts! {
    /// The newest protocol version this build speaks. Raised by every change to the wire format
    /// that a peer may need to know about, such as a new call, event, error code or field; a
    /// change only this build can see, such as a clearer error message, leaves it (D32).
    pub const PROTOCOL_VERSION: u32 = 2;
    /// The oldest protocol version this build still speaks. Raised only by a change an older
    /// peer cannot read, which then cannot connect at all and is told to update (D32).
    pub const MIN_PROTOCOL_VERSION: u32 = 1;
}

const _: () = assert!(MIN_PROTOCOL_VERSION <= PROTOCOL_VERSION);

/// The protocol versions one end speaks: every version from the oldest to the newest.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VersionRange {
    oldest: u32,
    newest: u32,
}

impl VersionRange {
    /// The versions this build speaks.
    pub const THIS_BUILD: VersionRange = VersionRange {
        oldest: MIN_PROTOCOL_VERSION,
        newest: PROTOCOL_VERSION,
    };

    /// The versions from `oldest` to `newest`, both included.
    ///
    /// # Errors
    ///
    /// [`VersionError::Empty`] when `newest` is older than `oldest`.
    pub fn new(oldest: u32, newest: u32) -> Result<Self, VersionError> {
        if newest < oldest {
            return Err(VersionError::Empty { oldest, newest });
        }
        Ok(Self { oldest, newest })
    }

    /// The oldest version in the range.
    #[must_use]
    pub fn oldest(self) -> u32 {
        self.oldest
    }

    /// The newest version in the range.
    #[must_use]
    pub fn newest(self) -> u32 {
        self.newest
    }

    /// Whether `version` is in the range.
    #[must_use]
    pub fn contains(self, version: u32) -> bool {
        (self.oldest..=self.newest).contains(&version)
    }

    /// The newest version this range shares with `peer`'s, which a connection between the two
    /// uses.
    ///
    /// # Errors
    ///
    /// [`VersionError::PeerTooOld`] when every version `peer` speaks is older than this range,
    /// and [`VersionError::PeerTooNew`] when every one is newer.
    pub fn agree(self, peer: VersionRange) -> Result<u32, VersionError> {
        if peer.newest < self.oldest {
            Err(VersionError::PeerTooOld { peer, ours: self })
        } else if peer.oldest > self.newest {
            Err(VersionError::PeerTooNew { peer, ours: self })
        } else {
            Ok(self.newest.min(peer.newest))
        }
    }
}

/// Why two ends of a connection cannot agree on a protocol version.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum VersionError {
    /// A peer named a range whose newest version is older than its oldest.
    #[error(
        "protocol versions {oldest} to {newest} are no range: the newest is older than the oldest"
    )]
    Empty {
        /// The oldest version named.
        oldest: u32,
        /// The newest version named.
        newest: u32,
    },
    /// The peer speaks only versions older than any this end speaks: the peer must be updated.
    #[error(
        "the peer speaks protocol versions {} to {}, all older than this end's oldest, {}",
        peer.oldest, peer.newest, ours.oldest
    )]
    PeerTooOld {
        /// The versions the peer speaks.
        peer: VersionRange,
        /// The versions this end speaks.
        ours: VersionRange,
    },
    /// The peer speaks only versions newer than any this end speaks: this end must be updated.
    #[error(
        "the peer speaks protocol versions {} to {}, all newer than this end's newest, {}",
        peer.oldest, peer.newest, ours.newest
    )]
    PeerTooNew {
        /// The versions the peer speaks.
        peer: VersionRange,
        /// The versions this end speaks.
        ours: VersionRange,
    },
}

impl CallMethod {
    /// Every method this build has. The generated `CALL_SINCE` is typed as a record of every
    /// method the TypeScript `Call` names, so a method left out here fails the type check there.
    pub const KNOWN: &[CallMethod] = &[
        CallMethod::Hello,
        CallMethod::AddDownload,
        CallMethod::WithdrawDownload,
        CallMethod::GetExtensionSettings,
        CallMethod::UpdateExtensionSettings,
        CallMethod::ListQueues,
        CallMethod::PauseAll,
        CallMethod::ResumeAll,
        CallMethod::ShowWindow,
        CallMethod::GetBrowserControls,
        CallMethod::UpdateBrowserControls,
        CallMethod::ShowBrowserSettings,
        CallMethod::NoteShown,
        CallMethod::NotePressed,
    ];

    /// The protocol version that added the call, or `None` for a method this build does not
    /// have. A connection on an older version answers the call with
    /// [`ErrorCode::UnsupportedCall`], and a client sends it only on a connection that has it.
    #[must_use]
    pub fn since(self) -> Option<u32> {
        match self {
            CallMethod::Hello
            | CallMethod::AddDownload
            | CallMethod::WithdrawDownload
            | CallMethod::GetExtensionSettings
            | CallMethod::UpdateExtensionSettings
            | CallMethod::ListQueues
            | CallMethod::PauseAll
            | CallMethod::ResumeAll
            | CallMethod::ShowWindow => Some(1),
            CallMethod::GetBrowserControls
            | CallMethod::UpdateBrowserControls
            | CallMethod::ShowBrowserSettings
            | CallMethod::NoteShown
            | CallMethod::NotePressed => Some(2),
            CallMethod::Unknown => None,
        }
    }

    /// Whether a connection on protocol `version` has this call.
    #[must_use]
    pub fn is_in(self, version: u32) -> bool {
        self.since().is_some_and(|since| since <= version)
    }
}

impl EventKind {
    /// Every kind of event this build has (see [`CallMethod::KNOWN`]).
    pub const KNOWN: &[EventKind] = &[
        EventKind::ExtensionSettingsChanged,
        EventKind::QueuesChanged,
        EventKind::BrowserControlsChanged,
        EventKind::NoteRequested,
    ];

    /// The protocol version that added the event. The app sends it only on connections that
    /// speak this version or a newer one, since an older client could not read it.
    #[must_use]
    pub fn since(self) -> u32 {
        match self {
            EventKind::ExtensionSettingsChanged | EventKind::QueuesChanged => 1,
            EventKind::BrowserControlsChanged | EventKind::NoteRequested => 2,
        }
    }
}

impl Event {
    /// The protocol version that added the event (see [`EventKind::since`]).
    #[must_use]
    pub fn since(&self) -> u32 {
        self.kind().since()
    }
}

impl Message {
    /// This message as a connection on protocol `version` has it (D32): `None` for an event
    /// newer than the connection, and an error answer carrying the code that version has for
    /// the failure. Every message an app writes to a client passes through this.
    #[must_use]
    pub fn as_of(self, version: u32) -> Option<Message> {
        match self {
            Message::Event { event } if event.since() > version => None,
            Message::Response {
                id,
                outcome: Outcome::Err(mut error),
            } => {
                error.code = error.code.as_of(version);
                Some(Message::Response {
                    id,
                    outcome: Outcome::Err(error),
                })
            }
            message => Some(message),
        }
    }
}

impl ErrorCode {
    /// Every error code this build has (see [`CallMethod::KNOWN`]).
    pub const KNOWN: &[ErrorCode] = &[
        ErrorCode::UnsupportedVersion,
        ErrorCode::InvalidRequest,
        ErrorCode::InvalidUrl,
        ErrorCode::NotFound,
        ErrorCode::AppUnavailable,
        ErrorCode::AppNotRunning,
        ErrorCode::Internal,
        ErrorCode::NotStarted,
        ErrorCode::AddonRequired,
        ErrorCode::LimitReached,
        ErrorCode::WrongState,
        ErrorCode::UpdateCheckFailed,
        ErrorCode::UnsupportedCall,
    ];

    /// The protocol version that added the code.
    #[must_use]
    pub fn since(self) -> u32 {
        match self {
            ErrorCode::UnsupportedVersion
            | ErrorCode::InvalidRequest
            | ErrorCode::InvalidUrl
            | ErrorCode::NotFound
            | ErrorCode::AppUnavailable
            | ErrorCode::AppNotRunning
            | ErrorCode::Internal
            | ErrorCode::NotStarted
            | ErrorCode::AddonRequired => 1,
            ErrorCode::LimitReached
            | ErrorCode::WrongState
            | ErrorCode::UpdateCheckFailed
            | ErrorCode::UnsupportedCall => 2,
        }
    }

    /// The code a connection older than [`ErrorCode::since`] gets for the same failure: an
    /// older code, or this one for a code from the oldest version.
    fn older_equivalent(self) -> ErrorCode {
        match self {
            ErrorCode::UnsupportedVersion
            | ErrorCode::InvalidRequest
            | ErrorCode::InvalidUrl
            | ErrorCode::NotFound
            | ErrorCode::AppUnavailable
            | ErrorCode::AppNotRunning
            | ErrorCode::Internal
            | ErrorCode::NotStarted
            | ErrorCode::AddonRequired => self,
            ErrorCode::LimitReached
            | ErrorCode::WrongState
            | ErrorCode::UpdateCheckFailed
            | ErrorCode::UnsupportedCall => ErrorCode::InvalidRequest,
        }
    }

    /// The code a connection on protocol `version` is sent for this failure: this code, or for
    /// a code added after `version`, the code that version had for it (D32).
    #[must_use]
    pub fn as_of(self, version: u32) -> ErrorCode {
        let older = self.older_equivalent();
        if self.since() <= version || older == self {
            self
        } else {
            older.as_of(version)
        }
    }
}

/// The tables of which version added each call, event and error code, written into the
/// generated TypeScript, so the extension can tell what a connection has.
#[cfg(test)]
pub(crate) fn since_tables() -> Vec<crate::exported::Constant> {
    use std::collections::BTreeMap;

    use crate::exported::Constant;

    fn table<T: Copy + serde::Serialize>(
        known: &[T],
        since: impl Fn(T) -> u32,
    ) -> BTreeMap<String, u32> {
        known
            .iter()
            .map(|&item| {
                let name = serde_json::to_value(item).unwrap();
                (name.as_str().unwrap().to_owned(), since(item))
            })
            .collect()
    }
    vec![
        Constant::json(
            "CALL_SINCE",
            &[
                " The protocol version that added each call (D32): a client sends a call only on a connection whose version has it.",
            ],
            "Readonly<Record<Call['method'], number>>",
            &["Call"],
            &table(CallMethod::KNOWN, |method| {
                method.since().expect("a known method has a version")
            }),
        ),
        Constant::json(
            "EVENT_SINCE",
            &[
                " The protocol version that added each event (D32): the app sends an event only on a connection whose version has it.",
            ],
            "Readonly<Record<Event['type'], number>>",
            &["Event"],
            &table(EventKind::KNOWN, EventKind::since),
        ),
        Constant::json(
            "ERROR_CODE_SINCE",
            &[
                " The protocol version that added each error code (D32): the app sends a connection only codes its version has.",
            ],
            "Readonly<Record<ErrorCode, number>>",
            &["ErrorCode"],
            &table(ErrorCode::KNOWN, ErrorCode::since),
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn range(oldest: u32, newest: u32) -> VersionRange {
        VersionRange::new(oldest, newest).unwrap()
    }

    #[test]
    fn two_ranges_agree_on_the_newest_version_both_speak() {
        let ours = range(2, 5);
        assert_eq!(ours.agree(range(1, 3)), Ok(3));
        assert_eq!(ours.agree(range(3, 9)), Ok(5));
        assert_eq!(ours.agree(range(4, 4)), Ok(4));
        assert_eq!(ours.agree(range(5, u32::MAX)), Ok(5));
        assert_eq!(ours.agree(range(0, 2)), Ok(2));
    }

    #[test]
    fn ranges_that_share_no_version_say_which_end_is_behind() {
        let ours = range(2, 5);
        assert_eq!(
            ours.agree(range(0, 1)),
            Err(VersionError::PeerTooOld {
                peer: range(0, 1),
                ours
            })
        );
        assert_eq!(
            ours.agree(range(6, u32::MAX)),
            Err(VersionError::PeerTooNew {
                peer: range(6, u32::MAX),
                ours
            })
        );
    }

    #[test]
    fn a_range_whose_newest_is_older_than_its_oldest_is_refused() {
        assert_eq!(
            VersionRange::new(3, 2),
            Err(VersionError::Empty {
                oldest: 3,
                newest: 2
            })
        );
        assert_eq!(
            VersionRange::new(u32::MAX, 0),
            Err(VersionError::Empty {
                oldest: u32::MAX,
                newest: 0
            })
        );
    }

    /// What one released protocol version has: its generated `Call`, `Event` and `ErrorCode` types, kept byte for byte in `crates/protocol/released` as the release generated them.
    struct Released {
        version: u32,
        calls: &'static str,
        events: &'static str,
        codes: &'static str,
    }

    /// Every released protocol version, frozen. A released version never changes: what it has is what peers built for it can read. Releasing a new version copies its generated types into `released/v<version>` and adds its entry here (D32); the `protocol` gate check fails when a snapshot already on main changes.
    const RELEASED: &[Released] = &[Released {
        version: 1,
        calls: include_str!("../released/v1/Call.ts"),
        events: include_str!("../released/v1/Event.ts"),
        codes: include_str!("../released/v1/ErrorCode.ts"),
    }];

    /// Every name written in `source` as `<before>'<name>'`, the way ts-rs writes the method of each `Call`, the type of each `Event` and each value of `ErrorCode`.
    fn quoted_after(source: &str, before: &str) -> std::collections::BTreeSet<String> {
        let opening = format!("{before}'");
        source
            .split(opening.as_str())
            .skip(1)
            .map(|rest| rest.split('\'').next().unwrap().to_owned())
            .collect()
    }

    /// The wire names of `known` that a connection on `version` has.
    fn names_in<T: Copy + serde::Serialize>(
        known: &[T],
        since: impl Fn(T) -> u32,
        version: u32,
    ) -> std::collections::BTreeSet<String> {
        known
            .iter()
            .filter(|&&item| since(item) <= version)
            .map(|&item| {
                serde_json::to_value(item)
                    .unwrap()
                    .as_str()
                    .unwrap()
                    .to_owned()
            })
            .collect()
    }

    fn call_since(method: CallMethod) -> u32 {
        method.since().unwrap()
    }

    /// A call, event or code added to an old version's arm would reach peers built for that
    /// version, which cannot read it. New ones belong to the version being built.
    #[test]
    fn every_released_version_keeps_exactly_what_it_had() {
        for released in RELEASED {
            let v = released.version;
            assert_eq!(
                names_in(CallMethod::KNOWN, call_since, v),
                quoted_after(released.calls, "method: "),
                "calls of {v}"
            );
            assert_eq!(
                names_in(EventKind::KNOWN, EventKind::since, v),
                quoted_after(released.events, "type: "),
                "events of {v}"
            );
            assert_eq!(
                names_in(ErrorCode::KNOWN, ErrorCode::since, v),
                quoted_after(released.codes, "| "),
                "codes of {v}"
            );
        }
    }

    /// The version is raised once per release: everything since the last released version is in
    /// the version being built, which is the last released one or the next.
    #[test]
    fn everything_after_the_last_release_is_in_the_version_being_built() {
        let last = RELEASED.last().unwrap().version;
        assert!(
            PROTOCOL_VERSION == last || PROTOCOL_VERSION == last + 1,
            "PROTOCOL_VERSION {PROTOCOL_VERSION} skips past the last release, {last}"
        );
        let versions = CallMethod::KNOWN
            .iter()
            .map(|&method| call_since(method))
            .chain(EventKind::KNOWN.iter().map(|&kind| kind.since()))
            .chain(ErrorCode::KNOWN.iter().map(|&code| code.since()));
        for since in versions {
            assert!(
                since <= last || since == PROTOCOL_VERSION,
                "version {since} is neither released nor the one being built"
            );
        }
    }

    /// A connection never gets an error code newer than its version: each code added after the
    /// oldest version this build speaks names an older code for the same failure.
    #[test]
    fn every_newer_error_code_has_an_older_equivalent() {
        for &code in ErrorCode::KNOWN {
            if code.since() > MIN_PROTOCOL_VERSION {
                let older = code.older_equivalent();
                assert!(
                    older.since() < code.since(),
                    "{code:?} has no older equivalent"
                );
            }
            for version in MIN_PROTOCOL_VERSION..=PROTOCOL_VERSION {
                assert!(
                    code.as_of(version).since() <= version,
                    "{code:?} as of {version}"
                );
            }
        }
        for code in [
            ErrorCode::LimitReached,
            ErrorCode::WrongState,
            ErrorCode::UpdateCheckFailed,
            ErrorCode::UnsupportedCall,
        ] {
            assert_eq!(code.as_of(1), ErrorCode::InvalidRequest, "{code:?}");
            assert_eq!(code.as_of(2), code);
        }
        assert_eq!(
            ErrorCode::InvalidRequest.as_of(0),
            ErrorCode::InvalidRequest
        );
    }

    /// The values of the enums inside calls, replies and events, as the generated TypeScript
    /// checks them. A new value is a breaking change unless the sender maps it to an older value
    /// for connections that predate it (D32.6): decide which, then update this list.
    #[test]
    fn the_values_of_enums_inside_messages_are_settled() {
        use crate::extension::{AddAction, CaptureSource, MediaKind, NoteKind, WithdrawOutcome};
        use crate::wire::ClientKind;
        use ts_rs::TS;

        let cfg = ts_rs::Config::default();
        let enums = [
            ("ClientKind", ClientKind::inline(&cfg)),
            ("MediaKind", MediaKind::inline(&cfg)),
            ("CaptureSource", CaptureSource::inline(&cfg)),
            ("AddAction", AddAction::inline(&cfg)),
            ("WithdrawOutcome", WithdrawOutcome::inline(&cfg)),
            ("NoteKind", NoteKind::inline(&cfg)),
        ];
        let settled = [
            ("ClientKind", r#""extension" | "bridge" | "cli""#),
            ("MediaKind", r#""hls" | "dash" | "direct" | "page""#),
            (
                "CaptureSource",
                r#""browser_download" | "context_menu" | "media_button""#,
            ),
            (
                "AddAction",
                r#""started" | "queued" | "awaiting_confirmation""#,
            ),
            (
                "WithdrawOutcome",
                r#""withdrawn" | "not_found" | "already_started""#,
            ),
            ("NoteKind", r#""started" | "finished" | "failed""#),
        ];
        for ((name, values), (settled_name, settled_values)) in enums.iter().zip(settled) {
            assert_eq!((name, values.as_str()), (&settled_name, settled_values));
        }
    }

    #[test]
    fn a_message_is_written_as_the_connections_version_has_it() {
        use crate::wire::{ProtocolError, Reply};

        let event = Message::Event {
            event: Event::QueuesChanged(Vec::new()),
        };
        assert_eq!(event.clone().as_of(0), None);
        assert_eq!(event.clone().as_of(1), Some(event));

        let unsupported = |code| Message::Response {
            id: 4,
            outcome: Outcome::Err(ProtocolError {
                code,
                message: "No.".into(),
            }),
        };
        assert_eq!(
            unsupported(ErrorCode::UnsupportedCall).as_of(1),
            Some(unsupported(ErrorCode::InvalidRequest))
        );
        assert_eq!(
            unsupported(ErrorCode::UnsupportedCall).as_of(2),
            Some(unsupported(ErrorCode::UnsupportedCall))
        );
        let done = Message::Response {
            id: 5,
            outcome: Outcome::Ok(Reply::Done),
        };
        assert_eq!(done.clone().as_of(0), Some(done));
    }

    #[test]
    fn this_build_speaks_from_the_oldest_to_the_newest_version() {
        let ours = VersionRange::THIS_BUILD;
        assert!(ours.contains(MIN_PROTOCOL_VERSION) && ours.contains(PROTOCOL_VERSION));
        assert!(!ours.contains(MIN_PROTOCOL_VERSION - 1) && !ours.contains(PROTOCOL_VERSION + 1));
        assert_eq!(ours.agree(ours), Ok(PROTOCOL_VERSION));
    }
}
