//! Questions the core asks the person (confirm a browser download, pick a video quality,
//! install the media add-on, what to do with a file already in the folder), and what each core
//! event means for the desktop.
//!
//! Every question follows something the person just did in the browser, and the browser has
//! already handed the download over, so each one opens its own window in front (D23, see
//! [`crate::download_windows`]), whether the main window is open, hidden in the tray or closed.
//! A storage failure is not a question but must not go unseen: with no main window it becomes a
//! system notification.
//!
//! The core keeps every open question (`pending_confirmations`, `pending_media_choices`,
//! `pending_addon_requests`, `media_being_read`), so a window opened or reloaded later loads its
//! question from there as a [`Question`](wirecatch_protocol::app::Question).

use wirecatch_protocol::QuestionId;
use wirecatch_protocol::app::{
    AddonRequestItem, DownloadSummary, FileExistsQuestion, MediaChoice, MediaReading,
    PendingCapture, Theme, UiEvent,
};
use wirecatch_protocol::extension::{NoticeId, NoticePressed};

/// What a core event needs from the desktop besides reaching the open windows.
#[derive(Debug, Clone, PartialEq)]
pub enum Attention {
    /// Nothing more.
    None,
    /// A question with this id is asked (or moves on to its next step): its window opens if it
    /// is not open yet.
    Ask(QuestionId),
    /// The question with this id is no longer open; it no longer waits for a window.
    Closed(QuestionId),
    /// A client asked to see Wirecatch: the main window comes to the front.
    ShowMain,
    /// A client asked for the browser settings (D61): the main window comes to the front on
    /// them, and a page still loading takes them later ([`crate::window::PendingSelection`]).
    ShowBrowserSettings,
    /// A browser download started without a window of its own (`ask_before_download` off, or a
    /// video downloaded with the saved choices): told briefly as the settings say, and no window
    /// comes up, since the person is still browsing (D63, D65).
    Started(Box<DownloadSummary>),
    /// A video from the browser is read for its qualities without a window: told as added at
    /// once, by its title (D67).
    VideoAdded(String),
    /// A browser showed the notice with this id (D65).
    NoticeShown(NoticeId),
    /// The person pressed one of a browser notice's buttons (D65).
    NoticePressed(NoticePressed),
    /// The settings changed: every window takes on their theme (see [`crate::appearance`]).
    Theme(Theme),
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
#[must_use]
pub fn attention(event: &UiEvent) -> Attention {
    match event {
        UiEvent::ConfirmationRequested(capture) => Attention::Ask(capture.id),
        UiEvent::MediaAnalysing { id, .. } | UiEvent::AddonRequired { id, .. } => {
            Attention::Ask(*id)
        }
        UiEvent::MediaChoiceRequested(choice) => Attention::Ask(choice.id),
        UiEvent::FileExistsRequested(question) => Attention::Ask(question.id),
        UiEvent::ConfirmationClosed { id } => Attention::Closed(*id),
        UiEvent::WindowRequested => Attention::ShowMain,
        UiEvent::BrowserSettingsRequested => Attention::ShowBrowserSettings,
        UiEvent::Captured(row) => Attention::Started(Box::new(row.clone())),
        UiEvent::MediaCaptured { title, .. } => Attention::VideoAdded(title.clone()),
        UiEvent::NoticeShown(id) => Attention::NoticeShown(*id),
        UiEvent::NoticePressed(pressed) => Attention::NoticePressed(pressed.clone()),
        UiEvent::SettingsChanged(settings) => Attention::Theme(settings.theme),
        UiEvent::StorageFailed(error) => Attention::Notify {
            title: format!(
                "{} could not save its download list",
                crate::host::CHANNEL.name()
            ),
            body: error.message.clone(),
        },
        _ => Attention::None,
    }
}

/// Every open question's id, oldest first (ids only grow), each once.
#[must_use]
pub fn open_question_ids(
    captures: &[PendingCapture],
    choices: &[MediaChoice],
    addon_pages: &[AddonRequestItem],
    reading: &[MediaReading],
    files: &[FileExistsQuestion],
) -> Vec<QuestionId> {
    let mut ids: Vec<QuestionId> = captures
        .iter()
        .map(|c| c.id)
        .chain(choices.iter().map(|c| c.id))
        .chain(addon_pages.iter().map(|a| a.id))
        .chain(reading.iter().map(|r| r.id))
        .chain(files.iter().map(|f| f.id))
        .collect();
    ids.sort_unstable();
    ids.dedup();
    ids
}

#[cfg(test)]
mod tests {
    use super::*;
    use wirecatch_protocol::app::{
        AddonId, AddonInfo, AddonState, Category, DownloadStatus, MediaSelection, OutputFormat,
        UserError,
    };
    use wirecatch_protocol::{DownloadId, QueueId};

    fn choice(id: u64) -> MediaChoice {
        MediaChoice {
            id: QuestionId(id),
            title: "Talk".into(),
            page_url: None,
            options: vec![],
            folder: "C:/Videos".into(),
            queue: QueueId(1),
            queue_reason: wirecatch_protocol::rules::QueueReason::DefaultQueue,
            subtitles: Vec::new(),
            chapters: false,
            duration_ms: None,
            start: MediaSelection {
                option_id: String::new(),
                folder: None,
                file_name: None,
                queue: None,
                later: false,
                output: OutputFormat::Original,
                audio_kbps: None,
                subtitles: Vec::new(),
                subtitle_files: false,
                chapters: false,
                every_video: None,
            },
            missing_quality: None,
        }
    }

    fn addon(state: AddonState) -> AddonInfo {
        AddonInfo {
            id: AddonId::Media,
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
            id: QuestionId(id),
            url: "https://example.com/a.zip".into(),
            file_name: "a.zip".into(),
            folder: "C:/Downloads".into(),
            category: Category::Archive,
            size: None,
            queue: QueueId(1),
            queue_reason: wirecatch_protocol::rules::QueueReason::DefaultQueue,
            page_url: None,
        }
    }

    #[test]
    fn every_step_of_a_question_asks_for_its_window() {
        assert_eq!(
            attention(&UiEvent::ConfirmationRequested(capture(1))),
            Attention::Ask(QuestionId(1))
        );
        assert_eq!(
            attention(&UiEvent::MediaAnalysing {
                id: QuestionId(2),
                url: "https://videos.example/watch".into()
            }),
            Attention::Ask(QuestionId(2))
        );
        assert_eq!(
            attention(&UiEvent::MediaChoiceRequested(choice(2))),
            Attention::Ask(QuestionId(2))
        );
        assert_eq!(
            attention(&UiEvent::AddonRequired {
                addon: addon(AddonState::NotInstalled),
                url: "https://youtube.com/x".into(),
                id: QuestionId(3),
            }),
            Attention::Ask(QuestionId(3))
        );
    }

    #[test]
    fn a_closed_question_and_a_storage_failure_are_told_apart() {
        assert_eq!(
            attention(&UiEvent::ConfirmationClosed { id: QuestionId(4) }),
            Attention::Closed(QuestionId(4))
        );
        let failed = attention(&UiEvent::StorageFailed(UserError {
            message: "The disk is full.".into(),
            fix: None,
        }));
        assert!(matches!(&failed, Attention::Notify { body, .. } if body == "The disk is full."));
        // The running app names itself: "Wirecatch Dev" in a debug build.
        let name = crate::host::CHANNEL.name();
        assert!(
            matches!(&failed, Attention::Notify { title, .. } if title == &format!("{name} could not save its download list")),
            "{failed:?}"
        );
        assert_eq!(
            attention(&UiEvent::DownloadsRemoved(vec![])),
            Attention::None
        );
        assert_eq!(attention(&UiEvent::WindowRequested), Attention::ShowMain);
        assert_eq!(
            attention(&UiEvent::BrowserSettingsRequested),
            Attention::ShowBrowserSettings
        );
    }

    /// D63: a browser download started without asking opens no window; it is told instead.
    #[test]
    fn a_download_started_without_asking_is_told() {
        let started = DownloadSummary {
            id: DownloadId(9),
            url: "https://example.com/a.zip".into(),
            file_name: "a.zip".into(),
            path: "C:/Downloads/a.zip".into(),
            queue: QueueId(1),
            queue_reason: wirecatch_protocol::rules::QueueReason::DefaultQueue,
            category: Category::Archive,
            status: DownloadStatus::Downloading,
            total: None,
            done: 0,
            speed: 0,
            connections: 0,
            eta_secs: None,
            error: None,
            waiting: None,
            media: None,
            added_at: 0,
            completed_at: None,
            processing: None,
            proxy: None,
        };
        assert_eq!(
            attention(&UiEvent::Captured(started.clone())),
            Attention::Started(Box::new(started))
        );
    }

    #[test]
    fn changed_settings_carry_the_theme_to_every_window() {
        let mut settings = wirecatch_core::default_settings();
        settings.theme = Theme::Dark;
        assert_eq!(
            attention(&UiEvent::SettingsChanged(Box::new(settings))),
            Attention::Theme(Theme::Dark)
        );
    }

    #[test]
    fn open_questions_are_listed_oldest_first_once() {
        let reading = [MediaReading {
            id: QuestionId(5),
            url: "https://videos.example/a".into(),
        }];
        let pages = [AddonRequestItem {
            id: QuestionId(3),
            addon: addon(AddonState::NotInstalled),
            url: "https://youtube.com/x".into(),
        }];
        let files = [FileExistsQuestion {
            id: QuestionId(6),
            download: DownloadId(1),
            url: "https://example.com/report.pdf".into(),
            file_name: "report.pdf".into(),
            folder: "Downloads".into(),
            size: 1,
            saved_at: None,
        }];
        let ids = open_question_ids(
            &[capture(4), capture(1)],
            &[choice(2)],
            &pages,
            &reading,
            &files,
        );
        assert_eq!(ids, [1, 2, 3, 4, 5, 6].map(QuestionId));
    }
}
