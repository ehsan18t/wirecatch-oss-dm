//! Notes in the browser (D65): the person is told something in the tab they are looking at, by
//! the extension of the browser in front, instead of in a system notification. Every connected
//! browser is asked; the one in front shows the note and says so, and when none does within
//! [`ANSWER_WAIT`] (no browser runs, or it is not in front, or its tab cannot show one) the system
//! notification shows instead. The app keeps what each button does, so the extension and the page
//! never hold a download's id. A note offers no Open: a website can lay a decoy over the note so
//! the person presses it unseen, and opening runs the file the website may just have had them
//! download; Show in folder and Try again run nothing.

use std::collections::{HashMap, VecDeque};
use std::sync::{Mutex, PoisonError};
use std::time::Duration;

use tauri::{AppHandle, Manager as _, Runtime};
use tokio::sync::oneshot;
use wirecatch_protocol::extension::{BrowserNote, NoteId, NoteKind, NotePressed};
use wirecatch_protocol::wire::NOTE_ANSWER_WAIT_MS;

use crate::host::Core;
use crate::notices::{Act, Notice};
use crate::system_notices;

/// How long the app waits for a browser to say it showed a note before it shows the system
/// notification instead. A browser in front answers within milliseconds.
pub const ANSWER_WAIT: Duration = Duration::from_millis(NOTE_ANSWER_WAIT_MS);

/// Notes whose buttons still work, the newest last: a note stays until it is closed, but past
/// this many the oldest are forgotten and their buttons do nothing.
const KEPT_NOTES: usize = 32;

/// The notes asked for, managed by the app.
#[derive(Default)]
pub struct BrowserNotes(Mutex<Notes>);

#[derive(Default)]
struct Notes {
    last_id: u64,
    /// Notes no browser has said it showed yet, with who waits for the answer.
    waiting: HashMap<NoteId, oneshot::Sender<()>>,
    /// What each note's buttons do.
    kept: VecDeque<(NoteId, Notice)>,
}

impl BrowserNotes {
    fn lock(&self) -> std::sync::MutexGuard<'_, Notes> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// A new note for `notice`, and the answer that it was shown.
    fn register(&self, notice: &Notice) -> (NoteId, oneshot::Receiver<()>) {
        let mut notes = self.lock();
        notes.last_id += 1;
        let id = NoteId(notes.last_id);
        let (shown, answer) = oneshot::channel();
        notes.waiting.insert(id, shown);
        if notes.kept.len() >= KEPT_NOTES {
            notes.kept.pop_front();
        }
        notes.kept.push_back((id, notice.clone()));
        (id, answer)
    }

    /// No browser said it showed the note `id` in time. Its buttons keep working, for a browser
    /// that shows it late.
    fn give_up(&self, id: NoteId) {
        self.lock().waiting.remove(&id);
    }

    /// A browser showed the note `id`; a second browser saying so changes nothing.
    fn shown(&self, id: NoteId) {
        if let Some(shown) = self.lock().waiting.remove(&id)
            && shown.send(()).is_err()
        {
            tracing::debug!("a browser showed a note after its notification had shown");
        }
    }

    /// What the note `id` stood for, while it is kept.
    fn notice(&self, id: NoteId) -> Option<Notice> {
        self.lock()
            .kept
            .iter()
            .find(|(kept, _)| *kept == id)
            .map(|(_, notice)| notice.clone())
    }
}

/// Tells `notice` in a note in the browser in front, of `kind`, which stays `stays` (`None` until
/// it is closed), or in a system notification when no browser shows it.
pub fn tell<R: Runtime>(
    app: &AppHandle<R>,
    notice: Notice,
    kind: NoteKind,
    stays: Option<Duration>,
) {
    let (Some(notes), Some(manager)) = (
        app.try_state::<BrowserNotes>(),
        app.try_state::<Core>().and_then(|core| core.get().cloned()),
    ) else {
        system_notices::show(app, notice);
        return;
    };
    let in_browser = without_open(&notice);
    let (id, answer) = notes.register(&in_browser);
    let note = BrowserNote {
        id,
        kind,
        title: in_browser.title,
        body: in_browser.body,
        buttons: in_browser
            .buttons
            .iter()
            .map(|b| b.label.to_owned())
            .collect(),
        stays_ms: stays.map(|d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX)),
    };
    if manager.request_note(note) == 0 {
        notes.give_up(id);
        system_notices::show(app, notice);
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
        if let Some(notes) = app.try_state::<BrowserNotes>() {
            notes.give_up(id);
        }
        system_notices::show(&app, notice);
    });
}

/// `notice` as a note in the browser offers it: without Open (see the module documentation).
fn without_open(notice: &Notice) -> Notice {
    let mut note = notice.clone();
    note.buttons.retain(|b| !matches!(b.act, Act::Open(_)));
    note
}

/// A browser showed the note `id`.
pub fn shown<R: Runtime>(app: &AppHandle<R>, id: NoteId) {
    if let Some(notes) = app.try_state::<BrowserNotes>() {
        notes.shown(id);
    }
}

/// The person pressed one of a note's buttons: does what it stands for, as the same button on
/// the system notification would.
pub fn pressed<R: Runtime>(app: &AppHandle<R>, pressed: &NotePressed) {
    let Some(notice) = app
        .try_state::<BrowserNotes>()
        .and_then(|notes| notes.notice(pressed.id))
    else {
        return;
    };
    let button = usize::try_from(pressed.button).ok();
    if let Some(act) = button.and_then(|at| system_notices::chosen(&notice, Some(at))) {
        system_notices::act(app, act);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::system_notices::plain;

    #[test]
    fn a_note_is_answered_once_and_its_buttons_work_after_the_wait() {
        let notes = BrowserNotes::default();
        let (id, mut answer) = notes.register(&plain("a".into(), "b".into()));
        assert_eq!(answer.try_recv(), Err(oneshot::error::TryRecvError::Empty));
        notes.shown(id);
        notes.shown(id);
        assert_eq!(answer.try_recv(), Ok(()));
        let (late, _) = notes.register(&plain("c".into(), "d".into()));
        notes.give_up(late);
        notes.shown(late);
        assert!(
            notes.notice(late).is_some(),
            "shown late, its buttons still work"
        );
    }

    #[test]
    fn a_note_in_the_browser_offers_no_open() {
        use crate::notices::Button;
        use wirecatch_protocol::DownloadId;
        let notice = Notice {
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
        let labels = |n: &Notice| n.buttons.iter().map(|b| b.label).collect::<Vec<_>>();
        assert_eq!(labels(&notice), ["Open", "Show in folder"]);
        assert_eq!(labels(&without_open(&notice)), ["Show in folder"]);
    }

    #[test]
    fn only_the_newest_notes_keep_their_buttons() {
        let notes = BrowserNotes::default();
        let ids: Vec<NoteId> = (0..=KEPT_NOTES)
            .map(|_| notes.register(&plain("a".into(), String::new())).0)
            .collect();
        assert!(notes.notice(ids[0]).is_none(), "the oldest is forgotten");
        assert!(ids[1..].iter().all(|id| notes.notice(*id).is_some()));
    }
}
