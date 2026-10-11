//! Questions the core asks the person (confirm a browser download, pick a video quality,
//! install the media add-on), and what happens when no window is open to ask them in.
//!
//! Every one of these follows something the person just did in the browser, and the browser
//! download waits on the answer, so the window opens (or is recreated) to ask. The app is only
//! ever started in the background by the bridge (for exactly such a browser action) or at
//! sign-in (when no browser action is pending), so opening the window never surprises anyone.
//! A storage failure is not a question but must not go unseen: with no window it becomes a
//! system notification.
//!
//! The core keeps every open question (`pending_confirmations`, `pending_media_choices`,
//! `pending_addon_requests`), so a window opened or reloaded later loads them from there.

use wirecatch_protocol::app::UiEvent;

/// What a core event needs from the desktop when no window is open.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Attention {
    /// Nothing; the interface catches up when it opens.
    None,
    /// A question: open the window to ask it.
    OpenWindow,
    /// Something went wrong that the person must know about: a system notification.
    Notify {
        /// Notification title.
        title: String,
        /// Notification text.
        body: String,
    },
}

/// What `event` needs while no window is open.
pub fn attention(event: &UiEvent) -> Attention {
    match event {
        UiEvent::ConfirmationRequested(_)
        | UiEvent::MediaChoiceRequested(_)
        | UiEvent::AddonRequired { .. } => Attention::OpenWindow,
        UiEvent::StorageFailed(error) => Attention::Notify {
            title: "Wirecatch could not save its download list".to_owned(),
            body: error.message.clone(),
        },
        _ => Attention::None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wirecatch_protocol::QueueId;
    use wirecatch_protocol::app::{
        AddonInfo, AddonState, Category, MediaChoice, PendingCapture, UserError,
    };

    fn choice(id: u64) -> MediaChoice {
        MediaChoice {
            id,
            title: "Talk".into(),
            page_url: None,
            options: vec![],
            folder: "C:/Videos".into(),
            queue: QueueId(1),
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
        }
    }

    #[test]
    fn questions_open_the_window_and_storage_failures_notify() {
        let capture = PendingCapture {
            id: 1,
            url: "https://example.com/a.zip".into(),
            file_name: "a.zip".into(),
            folder: "C:/Downloads".into(),
            category: Category::Archive,
            size: None,
            queue: QueueId(1),
            page_url: None,
        };
        assert_eq!(
            attention(&UiEvent::ConfirmationRequested(capture)),
            Attention::OpenWindow
        );
        assert_eq!(
            attention(&UiEvent::MediaChoiceRequested(choice(1))),
            Attention::OpenWindow
        );
        assert_eq!(
            attention(&UiEvent::AddonRequired {
                addon: addon(AddonState::NotInstalled),
                url: "https://youtube.com/x".into()
            }),
            Attention::OpenWindow
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
        assert_eq!(
            attention(&UiEvent::ConfirmationClosed { id: 1 }),
            Attention::None
        );
    }
}
