//! Questions the core asks the person (confirm a browser download, pick a video quality,
//! install the media add-on), and what each core event means for the desktop.
//!
//! Every question follows something the person just did in the browser, and the browser has
//! already handed the download over, so each one opens its own window in front (D23, see
//! [`crate::download_windows`]), whether the main window is open, hidden in the tray or closed.
//! A storage failure is not a question but must not go unseen: with no main window it becomes a
//! system notification.
//!
//! The core keeps every open question (`pending_confirmations`, `pending_media_choices`,
//! `pending_addon_requests`, `media_being_read`), so a window opened or reloaded later loads its
//! question from there with [`Question`].

use serde::Serialize;
use wirecatch_core::{AddonRequestItem, MediaReading};
use wirecatch_protocol::app::{MediaChoice, PendingCapture, UiEvent};

/// What a core event needs from the desktop besides reaching the open windows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Attention {
    /// Nothing more.
    None,
    /// A question with this id is asked (or moves on to its next step): its window opens if it
    /// is not open yet.
    Ask(u64),
    /// The question with this id is no longer open; it no longer waits for a window.
    Closed(u64),
    /// Something went wrong that the person must know about: a system notification when no
    /// main window shows it.
    Notify {
        /// Notification title.
        title: String,
        /// Notification text.
        body: String,
    },
}

/// What `event` needs from the desktop.
pub fn attention(event: &UiEvent) -> Attention {
    match event {
        UiEvent::ConfirmationRequested(capture) => Attention::Ask(capture.id),
        UiEvent::MediaAnalysing { id, .. } => Attention::Ask(*id),
        UiEvent::MediaChoiceRequested(choice) => Attention::Ask(choice.id),
        UiEvent::AddonRequired { id, .. } => Attention::Ask(*id),
        UiEvent::ConfirmationClosed { id } => Attention::Closed(*id),
        UiEvent::StorageFailed(error) => Attention::Notify {
            title: "Wirecatch could not save its download list".to_owned(),
            body: error.message.clone(),
        },
        _ => Attention::None,
    }
}

/// One open question, as a download window shows it (`get_question`). The interface mirrors
/// this as `Question` in `apps/desktop/src/lib/backend/types.ts`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", content = "data", rename_all = "snake_case")]
pub enum Question {
    /// A browser file waiting for Download now, Download later or Cancel.
    Capture(PendingCapture),
    /// A stream or video page being read for its qualities.
    Reading(MediaReading),
    /// A video waiting for the person to pick a quality.
    Choice(MediaChoice),
    /// A video page waiting for the media add-on.
    Addon(AddonRequestItem),
}

/// Every open question's id, oldest first (ids only grow), each once.
pub fn open_question_ids(
    captures: &[PendingCapture],
    choices: &[MediaChoice],
    addon_pages: &[AddonRequestItem],
    reading: &[MediaReading],
) -> Vec<u64> {
    let mut ids: Vec<u64> = captures
        .iter()
        .map(|c| c.id)
        .chain(choices.iter().map(|c| c.id))
        .chain(addon_pages.iter().map(|a| a.id))
        .chain(reading.iter().map(|r| r.id))
        .collect();
    ids.sort_unstable();
    ids.dedup();
    ids
}

#[cfg(test)]
mod tests {
    use super::*;
    use wirecatch_protocol::QueueId;
    use wirecatch_protocol::app::{AddonInfo, AddonState, Category, UserError};

    fn choice(id: u64) -> MediaChoice {
        MediaChoice {
            id,
            title: "Talk".into(),
            page_url: None,
            options: vec![],
            folder: "C:/Videos".into(),
            queue: QueueId(1),
            subtitles: Vec::new(),
            chapters: false,
            duration_ms: None,
        }
    }

    fn addon(state: AddonState) -> AddonInfo {
        AddonInfo {
            id: "media".into(),
            name: "Media add-on".into(),
            description: "Downloads video".into(),
            state,
            installed_version: None,
            latest_version: None,
            size: None,
            note: None,
        }
    }

    fn capture(id: u64) -> PendingCapture {
        PendingCapture {
            id,
            url: "https://example.com/a.zip".into(),
            file_name: "a.zip".into(),
            folder: "C:/Downloads".into(),
            category: Category::Archive,
            size: None,
            queue: QueueId(1),
            page_url: None,
        }
    }

    #[test]
    fn every_step_of_a_question_asks_for_its_window() {
        assert_eq!(
            attention(&UiEvent::ConfirmationRequested(capture(1))),
            Attention::Ask(1)
        );
        assert_eq!(
            attention(&UiEvent::MediaAnalysing {
                id: 2,
                url: "https://videos.example/watch".into()
            }),
            Attention::Ask(2)
        );
        assert_eq!(
            attention(&UiEvent::MediaChoiceRequested(choice(2))),
            Attention::Ask(2)
        );
        assert_eq!(
            attention(&UiEvent::AddonRequired {
                addon: addon(AddonState::NotInstalled),
                url: "https://youtube.com/x".into(),
                id: 3,
            }),
            Attention::Ask(3)
        );
    }

    #[test]
    fn a_closed_question_and_a_storage_failure_are_told_apart() {
        assert_eq!(
            attention(&UiEvent::ConfirmationClosed { id: 4 }),
            Attention::Closed(4)
        );
        let failed = attention(&UiEvent::StorageFailed(UserError {
            message: "The disk is full.".into(),
            fix: None,
        }));
        assert!(matches!(failed, Attention::Notify { body, .. } if body == "The disk is full."));
        assert_eq!(
            attention(&UiEvent::DownloadsRemoved(vec![])),
            Attention::None
        );
    }

    #[test]
    fn open_questions_are_listed_oldest_first_once() {
        let reading = [MediaReading {
            id: 5,
            url: "https://videos.example/a".into(),
        }];
        let pages = [AddonRequestItem {
            id: 3,
            addon: addon(AddonState::NotInstalled),
            url: "https://youtube.com/x".into(),
        }];
        let ids = open_question_ids(&[capture(4), capture(1)], &[choice(2)], &pages, &reading);
        assert_eq!(ids, [1, 2, 3, 4, 5]);
    }

    /// The interface reads this shape (`Question` in `src/lib/backend/types.ts`).
    #[test]
    fn a_question_is_sent_with_its_kind() {
        let json = serde_json::to_value(Question::Reading(MediaReading {
            id: 7,
            url: "https://videos.example/a".into(),
        }))
        .unwrap();
        assert_eq!(
            json,
            serde_json::json!({"kind": "reading", "data": {"id": 7, "url": "https://videos.example/a"}})
        );
        let json = serde_json::to_value(Question::Capture(capture(1))).unwrap();
        assert_eq!(json["kind"], "capture");
        assert_eq!(json["data"]["file_name"], "a.zip");
    }
}
