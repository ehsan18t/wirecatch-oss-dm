//! Notices in the browser (D65): the person is told something in the tab they are looking at, by
//! the extension of the browser in front, instead of in a system notification. Every connected
//! browser is asked; the one in front shows the notice and says so, and when none does within
//! [`ANSWER_WAIT`] (no browser runs, or it is not in front, or its tab cannot show one) the system
//! notification shows instead. The app keeps what each button does, so the extension and the page
//! never hold a download's id. A notice offers the system notification's buttons, Open included: a
//! website can lay a decoy over the notice so the person presses it unseen, but Open runs a program,
//! a script or a file of unknown type only after the shell's own warning, which no page can cover.

use std::collections::{HashMap, VecDeque};
use std::sync::{Mutex, PoisonError};
use std::time::Duration;

use tauri::{AppHandle, Manager as _, Runtime};
use tokio::sync::oneshot;
use wirecatch_protocol::extension::{BrowserNotice, NoticeId, NoticeKind, NoticePressed};
use wirecatch_protocol::wire::NOTICE_ANSWER_WAIT_MS;

use crate::host::Core;
use crate::notifications::Notification;
use crate::system_notifications;

/// How long the app waits for a browser to say it showed a notice before it shows the system
/// notification instead. A browser in front answers within milliseconds.
pub const ANSWER_WAIT: Duration = Duration::from_millis(NOTICE_ANSWER_WAIT_MS);

/// Notices whose buttons still work, the newest last: a notice stays until it is closed, but past
/// this many the oldest are forgotten and their buttons do nothing.
const KEPT_NOTICES: usize = 32;

/// The notices asked for, managed by the app.
#[derive(Default)]
pub struct BrowserNotices(Mutex<Notices>);

#[derive(Default)]
struct Notices {
    last_id: u64,
    /// Notices no browser has said it showed yet, with who waits for the answer.
    waiting: HashMap<NoticeId, oneshot::Sender<()>>,
    /// What each notice's buttons do.
    kept: VecDeque<(NoticeId, Notification)>,
}

impl BrowserNotices {
    fn lock(&self) -> std::sync::MutexGuard<'_, Notices> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// A new notice for `notification`, and the answer that it was shown.
    fn register(&self, notification: &Notification) -> (NoticeId, oneshot::Receiver<()>) {
        let mut notices = self.lock();
        notices.last_id += 1;
        let id = NoticeId(notices.last_id);
        let (shown, answer) = oneshot::channel();
        notices.waiting.insert(id, shown);
        if notices.kept.len() >= KEPT_NOTICES {
            notices.kept.pop_front();
        }
        notices.kept.push_back((id, notification.clone()));
        (id, answer)
    }

    /// No browser said it showed the notice `id` in time. Its buttons keep working, for a browser
    /// that shows it late.
    fn give_up(&self, id: NoticeId) {
        self.lock().waiting.remove(&id);
    }

    /// A browser showed the notice `id`; a second browser saying so changes nothing.
    fn shown(&self, id: NoticeId) {
        if let Some(shown) = self.lock().waiting.remove(&id)
            && shown.send(()).is_err()
        {
            tracing::debug!("a browser showed a notice after its notification had shown");
        }
    }

    /// What the notice `id` stood for, while it is kept.
    fn notification(&self, id: NoticeId) -> Option<Notification> {
        self.lock()
            .kept
            .iter()
            .find(|(kept, _)| *kept == id)
            .map(|(_, notification)| notification.clone())
    }
}

/// Tells `notification` in a notice in the browser in front, of `kind`, which stays `stays` (`None` until
/// it is closed), or in a system notification when no browser shows it.
pub fn tell<R: Runtime>(
    app: &AppHandle<R>,
    notification: Notification,
    kind: NoticeKind,
    stays: Option<Duration>,
) {
    let (Some(notices), Some(manager)) = (
        app.try_state::<BrowserNotices>(),
        app.try_state::<Core>().and_then(|core| core.get().cloned()),
    ) else {
        system_notifications::show(app, notification);
        return;
    };
    let (id, answer) = notices.register(&notification);
    let notice = browser_notice(id, kind, &notification, stays);
    if manager.request_notice(notice) == 0 {
        notices.give_up(id);
        system_notifications::show(app, notification);
        return;
    }
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        if tokio::time::timeout(ANSWER_WAIT, answer)
            .await
            .is_ok_and(|r| r.is_ok())
        {
            return;
        }
        if let Some(notices) = app.try_state::<BrowserNotices>() {
            notices.give_up(id);
        }
        system_notifications::show(&app, notification);
    });
}

/// `notification` as notice `id` in the browser offers it, with the same buttons.
fn browser_notice(
    id: NoticeId,
    kind: NoticeKind,
    notification: &Notification,
    stays: Option<Duration>,
) -> BrowserNotice {
    BrowserNotice {
        id,
        kind,
        title: notification.title.clone(),
        body: notification.body.clone(),
        buttons: notification
            .buttons
            .iter()
            .map(|b| b.label.to_owned())
            .collect(),
        stays_ms: stays.map(|d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX)),
    }
}

/// A browser showed the notice `id`.
pub fn shown<R: Runtime>(app: &AppHandle<R>, id: NoticeId) {
    if let Some(notices) = app.try_state::<BrowserNotices>() {
        notices.shown(id);
    }
}

/// The person pressed one of a notice's buttons: does what it stands for, as the same button on
/// the system notification would.
pub fn pressed<R: Runtime>(app: &AppHandle<R>, pressed: &NoticePressed) {
    let Some(notification) = app
        .try_state::<BrowserNotices>()
        .and_then(|notices| notices.notification(pressed.id))
    else {
        return;
    };
    let button = usize::try_from(pressed.button).ok();
    if let Some(act) = button.and_then(|at| system_notifications::chosen(&notification, Some(at))) {
        system_notifications::act(app, act);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::notifications::Act;
    use crate::system_notifications::plain;

    #[test]
    fn a_note_is_answered_once_and_its_buttons_work_after_the_wait() {
        let notices = BrowserNotices::default();
        let (id, mut answer) = notices.register(&plain("a".into(), "b".into()));
        assert_eq!(answer.try_recv(), Err(oneshot::error::TryRecvError::Empty));
        notices.shown(id);
        notices.shown(id);
        assert_eq!(answer.try_recv(), Ok(()));
        let (late, _) = notices.register(&plain("c".into(), "d".into()));
        notices.give_up(late);
        notices.shown(late);
        assert!(
            notices.notification(late).is_some(),
            "shown late, its buttons still work"
        );
    }

    /// Open runs nothing a website chose unseen: a program, a script or an unknown type opens
    /// only after the shell's own warning, which no page can cover (D65.4).
    #[test]
    fn a_notice_in_the_browser_offers_the_same_buttons() {
        use crate::notifications::Button;
        use wirecatch_protocol::DownloadId;
        let notification = Notification {
            buttons: vec![
                Button {
                    label: "Open",
                    act: Act::Open(DownloadId(1)),
                },
                Button {
                    label: "Show in folder",
                    act: Act::ShowInFolder(DownloadId(1)),
                },
            ],
            ..plain("Download finished".into(), "a.zip".into())
        };
        let notice = browser_notice(NoticeId(7), NoticeKind::Finished, &notification, None);
        assert_eq!(notice.buttons, ["Open", "Show in folder"]);
        assert_eq!(
            system_notifications::chosen(&notification, Some(0)),
            Some(Act::Open(DownloadId(1)))
        );
    }

    #[test]
    fn only_the_newest_notes_keep_their_buttons() {
        let notices = BrowserNotices::default();
        let ids: Vec<NoticeId> = (0..=KEPT_NOTICES)
            .map(|_| notices.register(&plain("a".into(), String::new())).0)
            .collect();
        assert!(
            notices.notification(ids[0]).is_none(),
            "the oldest is forgotten"
        );
        assert!(
            ids[1..]
                .iter()
                .all(|id| notices.notification(*id).is_some())
        );
    }
}
