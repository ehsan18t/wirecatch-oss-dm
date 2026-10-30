//! Notices in the browser (D65): the person is told something in the tab they are looking at, by
//! the extension of the browser in front, instead of in a system notification. Every connected
//! browser is asked; the one in front shows the notice and says so, and when none does within
//! [`ANSWER_WAIT`] (no browser runs, or it is not in front, or its tab cannot show one) the system
//! notification shows instead. The app keeps what each button does, so the extension and the page
//! never hold a download's id. A notice offers the system notification's buttons, Open included: a
//! website can lay a decoy over the notice so the person presses it unseen, but Open runs a program,
//! a script or a file of unknown type only after the shell's own warning, which no page can cover.

use std::collections::{BTreeSet, HashMap, VecDeque};
use std::sync::{Mutex, PoisonError};
use std::time::Duration;

use tauri::{AppHandle, Manager as _, Runtime};
use tokio::sync::oneshot;
use wirecatch_protocol::DownloadId;
use wirecatch_protocol::app::{DownloadStatus, UiEvent};
use wirecatch_protocol::extension::{BrowserNotice, NoticeId, NoticeKind, NoticePressed};
use wirecatch_protocol::wire::NOTICE_ANSWER_WAIT_MS;

use crate::host::Core;
use crate::notifications::{Act, Notification};
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
    /// What each notice's buttons do, and what it is about.
    kept: VecDeque<Kept>,
}

struct Kept {
    id: NoticeId,
    notification: Notification,
    kind: NoticeKind,
    /// The downloads it is about that still are as it says: in the list, and failed for a
    /// failure. The notice is taken back once none is (D97.4).
    still: BTreeSet<DownloadId>,
}

impl BrowserNotices {
    fn lock(&self) -> std::sync::MutexGuard<'_, Notices> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// A new notice of `kind` for `notification`, and the answer that it was shown.
    fn register(
        &self,
        notification: &Notification,
        kind: NoticeKind,
    ) -> (NoticeId, oneshot::Receiver<()>) {
        let mut notices = self.lock();
        notices.last_id += 1;
        let id = NoticeId(notices.last_id);
        let (shown, answer) = oneshot::channel();
        notices.waiting.insert(id, shown);
        if notices.kept.len() >= KEPT_NOTICES {
            notices.kept.pop_front();
        }
        let still = match &notification.click {
            Act::Show(reveal) => reveal.ids.iter().copied().collect(),
            _ => BTreeSet::new(),
        };
        notices.kept.push_back(Kept {
            id,
            notification: notification.clone(),
            kind,
            still,
        });
        (id, answer)
    }

    /// The notices that no longer apply after `event`, forgotten here: every download one is
    /// about was removed, or, for a failure, removed or tried again.
    fn outdated_by(&self, event: &UiEvent) -> Vec<NoticeId> {
        let mut notices = self.lock();
        let mut outdated = Vec::new();
        notices.kept.retain_mut(|kept| {
            if kept.still.is_empty() {
                return true;
            }
            match event {
                UiEvent::DownloadsRemoved(ids) => {
                    for id in ids {
                        kept.still.remove(id);
                    }
                }
                UiEvent::DownloadsChanged(rows) if kept.kind == NoticeKind::Failed => {
                    for row in rows.iter().filter(|r| r.status != DownloadStatus::Failed) {
                        kept.still.remove(&row.id);
                    }
                }
                _ => return true,
            }
            if !kept.still.is_empty() {
                return true;
            }
            outdated.push(kept.id);
            false
        });
        for id in &outdated {
            notices.waiting.remove(id);
        }
        outdated
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
            .find(|kept| kept.id == id)
            .map(|kept| kept.notification.clone())
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
    let (id, answer) = notices.register(&notification, kind);
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

/// Takes back the notices `event` made outdated, wherever a browser shows them.
pub fn follow<R: Runtime>(app: &AppHandle<R>, event: &UiEvent) {
    let (Some(notices), Some(manager)) = (
        app.try_state::<BrowserNotices>(),
        app.try_state::<Core>().and_then(|core| core.get().cloned()),
    ) else {
        return;
    };
    for id in notices.outdated_by(event) {
        manager.withdraw_notice(id);
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
        let (id, mut answer) =
            notices.register(&plain("a".into(), "b".into()), NoticeKind::Finished);
        assert_eq!(answer.try_recv(), Err(oneshot::error::TryRecvError::Empty));
        notices.shown(id);
        notices.shown(id);
        assert_eq!(answer.try_recv(), Ok(()));
        let (late, _) = notices.register(&plain("c".into(), "d".into()), NoticeKind::Finished);
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
            .map(|_| {
                notices
                    .register(&plain("a".into(), String::new()), NoticeKind::Finished)
                    .0
            })
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

    /// A notice about downloads that are gone, or a failure tried again, is taken back (D97.4).
    #[test]
    fn a_notice_is_taken_back_once_its_downloads_are_gone_or_tried_again() {
        use crate::notifications::tests::row;
        use wirecatch_protocol::app::DownloadStatus::{Failed, Queued};
        let notices = BrowserNotices::default();
        let failed = |ids: &[i64]| {
            let rows: Vec<_> = ids.iter().map(|&id| row(id, "downloads", Failed)).collect();
            crate::notifications::failed(&rows.iter().collect::<Vec<_>>()).unwrap()
        };
        let (two, _) = notices.register(&failed(&[1, 2]), NoticeKind::Failed);
        let (three, _) = notices.register(&failed(&[3]), NoticeKind::Failed);

        let retried = UiEvent::DownloadsChanged(vec![row(1, "downloads", Queued)]);
        assert_eq!(notices.outdated_by(&retried), []);
        let removed = UiEvent::DownloadsRemoved(vec![DownloadId(2)]);
        assert_eq!(notices.outdated_by(&removed), [two]);
        assert!(notices.notification(two).is_none());

        let still_failed = UiEvent::DownloadsChanged(vec![row(3, "downloads", Failed)]);
        assert_eq!(notices.outdated_by(&still_failed), []);
        assert!(notices.notification(three).is_some());
    }
}
