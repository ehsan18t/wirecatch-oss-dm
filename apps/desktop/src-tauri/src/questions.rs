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
//! A new window loads confirmations from the core. Media choices and add-on prompts are kept
//! here until answered, so a window opened (or reloaded) after the event still shows them. Once
//! the core offers `pending_media_choices()` (WP11), [`Questions::media_choices`] reads it from
//! there instead.

use std::sync::Mutex;

use wirecatch_protocol::app::{AddonInfo, AddonState, MediaChoice, UiEvent};

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

/// A capture that needs an add-on, waiting for the person's answer.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct AddonRequest {
    /// The add-on to offer.
    pub addon: AddonInfo,
    /// The page or link that needs it.
    pub url: String,
}

/// Open media choices and the add-on prompt, kept until answered.
#[derive(Default)]
pub struct Questions {
    media: Mutex<Vec<MediaChoice>>,
    addon: Mutex<Option<AddonRequest>>,
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

impl Questions {
    /// Keeps track of the questions in `event`.
    pub fn note(&self, event: &UiEvent) {
        match event {
            UiEvent::MediaChoiceRequested(choice) => {
                let mut media = lock(&self.media);
                if !media.iter().any(|c| c.id == choice.id) {
                    media.push(choice.clone());
                }
            }
            UiEvent::AddonRequired { addon, url } => {
                *lock(&self.addon) = Some(AddonRequest {
                    addon: addon.clone(),
                    url: url.clone(),
                });
            }
            UiEvent::AddonsChanged(addons) => {
                let mut asked = lock(&self.addon);
                let installed = asked.as_ref().is_some_and(|a| {
                    addons
                        .iter()
                        .any(|x| x.id == a.addon.id && x.state == AddonState::Installed)
                });
                if installed {
                    *asked = None;
                }
            }
            _ => {}
        }
    }

    /// The media choices still open.
    pub fn media_choices(&self) -> Vec<MediaChoice> {
        lock(&self.media).clone()
    }

    /// Forgets a media choice once it is answered either way.
    pub fn answered_media(&self, id: u64) {
        lock(&self.media).retain(|c| c.id != id);
    }

    /// Hands the waiting add-on prompt over, once.
    pub fn take_addon_request(&self) -> Option<AddonRequest> {
        lock(&self.addon).take()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wirecatch_protocol::QueueId;
    use wirecatch_protocol::app::{Category, PendingCapture, UserError};

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
    }

    #[test]
    fn open_questions_are_kept_until_answered() {
        let q = Questions::default();
        q.note(&UiEvent::MediaChoiceRequested(choice(1)));
        q.note(&UiEvent::MediaChoiceRequested(choice(2)));
        q.note(&UiEvent::MediaChoiceRequested(choice(1)));
        assert_eq!(q.media_choices().len(), 2);
        q.answered_media(1);
        assert_eq!(
            q.media_choices().iter().map(|c| c.id).collect::<Vec<_>>(),
            [2]
        );

        q.note(&UiEvent::AddonRequired {
            addon: addon(AddonState::NotInstalled),
            url: "https://youtube.com/x".into(),
        });
        assert!(q.take_addon_request().is_some());
        assert!(q.take_addon_request().is_none());

        // Installing the add-on answers its prompt.
        q.note(&UiEvent::AddonRequired {
            addon: addon(AddonState::NotInstalled),
            url: "https://youtube.com/x".into(),
        });
        q.note(&UiEvent::AddonsChanged(vec![addon(AddonState::Installed)]));
        assert!(q.take_addon_request().is_none());
    }
}
