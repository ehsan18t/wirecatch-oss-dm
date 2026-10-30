//! What the person is told when downloads finish or fail (D52), or start without a window of their
//! own (D63): the words and buttons of each notification. Shown by [`crate::system_notifications`]
//! or [`crate::browser_notices`]; when to tell is decided by [`crate::teller`].

use std::path::Path;

use wirecatch_protocol::app::{DownloadSummary, QueueReport, RETRY_FIXES};
use wirecatch_protocol::desktop::Reveal;
use wirecatch_protocol::{DownloadId, QueueId};

/// What a notification's button, or a click on the notification itself, does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Act {
    /// Opens the finished file; a program asks first (D43.1).
    Open(DownloadId),
    /// Shows the file selected in its folder.
    ShowInFolder(DownloadId),
    /// Tries the failed download again.
    Retry(DownloadId),
    /// Shows the main window with these downloads selected.
    Show(Reveal),
}

/// A notification's button.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Button {
    /// Its text.
    pub label: &'static str,
    /// What it does.
    pub act: Act,
}

/// How long a notification stays.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lasts {
    /// Until the person dismisses it, and in the system's list of notifications.
    Kept,
    /// This long, and never in the system's list: news the person needs no record of.
    Brief(std::time::Duration),
}

/// One system notification.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Notification {
    /// The first line.
    pub title: String,
    /// The text under it.
    pub body: String,
    /// Its buttons, in order.
    pub buttons: Vec<Button>,
    /// What a click on the notification itself does: never opens a file, since people click a
    /// notification to dismiss it, and the file may be a program.
    pub click: Act,
    /// How long it stays.
    pub lasts: Lasts,
    /// Whether it matters enough to show during the system's Do Not Disturb, while the person
    /// allows it (D64): what ended and what went wrong do; news does not.
    pub important: bool,
}

/// A notification with words only about something the person must know; a click shows the main
/// window.
#[must_use]
pub fn plain(title: String, body: String) -> Notification {
    Notification {
        title,
        body,
        buttons: Vec::new(),
        click: Act::Show(Reveal {
            queue: None,
            ids: Vec::new(),
            browser_settings: false,
            proxy_settings: false,
            details: false,
        }),
        lasts: Lasts::Kept,
        important: true,
    }
}

/// What a click did: a button by its place, or the notification itself.
#[must_use]
pub fn chosen(notification: &Notification, button: Option<usize>) -> Option<Act> {
    match button {
        None => Some(notification.click.clone()),
        Some(at) => notification.buttons.get(at).map(|b| b.act.clone()),
    }
}

const OPEN: &str = "Open";
const SHOW_IN_FOLDER: &str = "Show in folder";
const TRY_AGAIN: &str = "Try again";
const SHOW_IN_APP: &str = "Show in Wirecatch";

/// Shows `rows` in the app: in `queue`, else in the queue they share, so a click lands where they
/// are.
fn show(queue: Option<QueueId>, rows: &[&DownloadSummary]) -> Act {
    let shared = rows
        .first()
        .map(|first| first.queue)
        .filter(|queue| rows.iter().all(|r| r.queue == *queue));
    Act::Show(Reveal {
        queue: queue.or(shared),
        ids: rows.iter().map(|r| r.id).collect(),
        browser_settings: false,
        proxy_settings: false,
        details: false,
    })
}

/// The files' names, two of them before the rest are counted: "a, b and 3 more".
fn names(rows: &[&DownloadSummary]) -> String {
    match rows {
        [] => String::new(),
        [one] => one.file_name.clone(),
        [a, b] => format!("{} and {}", a.file_name, b.file_name),
        [a, b, c] => format!("{}, {} and {}", a.file_name, b.file_name, c.file_name),
        [a, b, rest @ ..] => format!("{}, {} and {} more", a.file_name, b.file_name, rest.len()),
    }
}

fn downloads(n: usize) -> String {
    if n == 1 {
        "1 download".to_owned()
    } else {
        format!("{n} downloads")
    }
}

/// The folder every one of `rows` is in, when they share one.
fn shared_folder<'a>(rows: &[&'a DownloadSummary]) -> Option<&'a Path> {
    let (first, rest) = rows.split_first()?;
    let folder = Path::new(&first.path).parent()?;
    rest.iter()
        .all(|r| Path::new(&r.path).parent() == Some(folder))
        .then_some(folder)
}

/// Whether trying again could fix the failure from the notification. Other fixes (a fresh link,
/// another folder, keeping the original) need the main window, which a click opens.
fn retry_helps(row: &DownloadSummary) -> bool {
    row.error
        .as_ref()
        .and_then(|e| e.fix)
        .is_none_or(|fix| RETRY_FIXES.contains(&fix))
}

const ADDED: &str = "Download added";

/// For a browser download that started without a window of its own (D63): it `stays` briefly, since
/// the person is still browsing.
#[must_use]
pub fn started(row: &DownloadSummary, stays: std::time::Duration) -> Notification {
    Notification {
        title: ADDED.to_owned(),
        body: row.file_name.clone(),
        buttons: Vec::new(),
        click: show(None, &[row]),
        lasts: Lasts::Brief(stays),
        important: false,
    }
}

/// For a video from the browser read for its qualities without a window (D67): told as soon as
/// it is handed over, by its `title`, before its file has a name.
#[must_use]
pub fn video_added(title: &str, stays: std::time::Duration) -> Notification {
    Notification {
        title: ADDED.to_owned(),
        body: format!("{title}: getting video details"),
        buttons: Vec::new(),
        click: show(None, &[]),
        lasts: Lasts::Brief(stays),
        important: false,
    }
}

/// For downloads that finished close together. `None` for none.
#[must_use]
pub fn finished(rows: &[&DownloadSummary]) -> Option<Notification> {
    finished_in(None, rows)
}

fn finished_in(queue: Option<QueueId>, rows: &[&DownloadSummary]) -> Option<Notification> {
    let notification = match rows {
        [] => return None,
        [one] => Notification {
            title: "Download finished".to_owned(),
            body: one.file_name.clone(),
            buttons: vec![
                Button {
                    label: OPEN,
                    act: Act::Open(one.id),
                },
                Button {
                    label: SHOW_IN_FOLDER,
                    act: Act::ShowInFolder(one.id),
                },
            ],
            click: show(queue, rows),
            lasts: Lasts::Kept,
            important: true,
        },
        [first, ..] => Notification {
            title: format!("{} finished", downloads(rows.len())),
            body: names(rows),
            buttons: vec![folder_or_app(rows, first.id, queue)],
            click: show(queue, rows),
            lasts: Lasts::Kept,
            important: true,
        },
    };
    Some(notification)
}

/// Show in folder when every file is in one folder, else Show in Wirecatch.
fn folder_or_app(rows: &[&DownloadSummary], first: DownloadId, queue: Option<QueueId>) -> Button {
    if shared_folder(rows).is_some() {
        Button {
            label: SHOW_IN_FOLDER,
            act: Act::ShowInFolder(first),
        }
    } else {
        Button {
            label: SHOW_IN_APP,
            act: show(queue, rows),
        }
    }
}

/// For downloads that failed close together, never mixed with finished ones. `None` for none.
#[must_use]
pub fn failed(rows: &[&DownloadSummary]) -> Option<Notification> {
    failed_in(None, rows)
}

fn failed_in(queue: Option<QueueId>, rows: &[&DownloadSummary]) -> Option<Notification> {
    let notification = match rows {
        [] => return None,
        [one] => Notification {
            title: "Download failed".to_owned(),
            body: match &one.error {
                Some(error) => format!("{}: {}", one.file_name, error.message),
                None => one.file_name.clone(),
            },
            buttons: retry_helps(one)
                .then_some(Button {
                    label: TRY_AGAIN,
                    act: Act::Retry(one.id),
                })
                .into_iter()
                .collect(),
            click: show(queue, rows),
            lasts: Lasts::Kept,
            important: true,
        },
        _ => Notification {
            title: format!("{} failed", downloads(rows.len())),
            body: names(rows),
            buttons: vec![Button {
                label: SHOW_IN_APP,
                act: show(queue, rows),
            }],
            click: show(queue, rows),
            lasts: Lasts::Kept,
            important: true,
        },
    };
    Some(notification)
}

/// For a queue that is done. A run of one download reads like that download's own notification.
#[must_use]
pub fn queue_done(report: &QueueReport) -> Option<Notification> {
    let completed: Vec<&DownloadSummary> = report.completed.iter().collect();
    let failed: Vec<&DownloadSummary> = report.failed.iter().collect();
    let queue = Some(report.queue);
    match (completed.as_slice(), failed.as_slice()) {
        ([], []) => None,
        ([_], []) => finished_in(queue, &completed),
        ([], [_]) => failed_in(queue, &failed),
        _ => {
            let all: Vec<&DownloadSummary> = completed.iter().chain(&failed).copied().collect();
            let title = match (completed.len(), failed.len()) {
                (0, f) => format!("{}: {} failed", report.name, downloads(f)),
                (c, 0) => format!("{}: {} finished", report.name, downloads(c)),
                (c, f) => format!("{}: {} finished and {f} failed", report.name, downloads(c)),
            };
            let button = match completed.first() {
                Some(first) => folder_or_app(&completed, first.id, queue),
                None => Button {
                    label: SHOW_IN_APP,
                    act: show(queue, &all),
                },
            };
            Some(Notification {
                title,
                body: names(&all),
                buttons: vec![button],
                click: show(queue, &all),
                lasts: Lasts::Kept,
                important: true,
            })
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use wirecatch_protocol::app::{Category, DownloadStatus, FixAction, UserError};

    pub(crate) fn row(id: i64, folder: &str, status: DownloadStatus) -> DownloadSummary {
        DownloadSummary {
            id: DownloadId(id),
            url: String::new(),
            file_name: format!("file-{id}.zip"),
            path: Path::new(folder)
                .join(format!("file-{id}.zip"))
                .to_string_lossy()
                .into_owned(),
            queue: QueueId(1),
            queue_reason: wirecatch_protocol::rules::QueueReason::DefaultQueue,
            category: Category::Archive,
            status,
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
        }
    }

    fn done(id: i64) -> DownloadSummary {
        row(id, "downloads", DownloadStatus::Completed)
    }

    fn broken(id: i64, fix: Option<FixAction>) -> DownloadSummary {
        let mut r = row(id, "downloads", DownloadStatus::Failed);
        r.error = Some(UserError {
            message: "The server refused.".into(),
            fix,
        });
        r
    }

    fn reveal(queue: Option<QueueId>, ids: &[i64]) -> Act {
        Act::Show(Reveal {
            queue,
            ids: ids.iter().copied().map(DownloadId).collect(),
            browser_settings: false,
            proxy_settings: false,
            details: false,
        })
    }

    fn labels(notification: &Notification) -> Vec<&str> {
        notification.buttons.iter().map(|b| b.label).collect()
    }

    const BRIEFLY: std::time::Duration = std::time::Duration::from_secs(3);

    #[test]
    fn a_download_from_the_browser_is_told_briefly_as_added() {
        let started = row(1, "videos", DownloadStatus::Downloading);
        let notification = super::started(&started, BRIEFLY);
        assert_eq!(notification.title, "Download added");
        assert_eq!(notification.body, "file-1.zip");
        assert_eq!(notification.buttons, []);
        assert_eq!(notification.click, reveal(Some(QueueId(1)), &[1]));
        assert_eq!(notification.lasts, Lasts::Brief(BRIEFLY));
        assert!(
            !notification.important,
            "news, never through Do Not Disturb"
        );
        assert_eq!(finished(&[&done(2)]).unwrap().lasts, Lasts::Kept);

        let video = video_added("Keynote", BRIEFLY);
        assert_eq!(
            (video.title.as_str(), video.body.as_str()),
            ("Download added", "Keynote: getting video details")
        );
        assert_eq!(video.lasts, Lasts::Brief(BRIEFLY));
    }

    /// A click shows the downloads in the queue they share, so the list opens where they are.
    #[test]
    fn a_click_shows_the_queue_its_downloads_share() {
        let mut other = done(2);
        other.queue = QueueId(5);
        let notification = finished(&[&other]).unwrap();
        assert_eq!(notification.click, reveal(Some(QueueId(5)), &[2]));
        let both = finished(&[&done(1), &other]).unwrap();
        assert_eq!(both.click, reveal(None, &[1, 2]));
    }

    #[test]
    fn one_finished_download_offers_to_open_it_and_a_click_shows_it() {
        let notification = finished(&[&done(1)]).unwrap();
        assert_eq!(notification.title, "Download finished");
        assert_eq!(notification.body, "file-1.zip");
        assert_eq!(labels(&notification), [OPEN, SHOW_IN_FOLDER]);
        assert_eq!(notification.buttons[0].act, Act::Open(DownloadId(1)));
        assert_eq!(notification.click, reveal(Some(QueueId(1)), &[1]));
        assert_eq!(finished(&[]), None);
    }

    #[test]
    fn downloads_finishing_together_share_one_notification() {
        let rows = [done(1), done(2), done(3), done(4)];
        let all: Vec<_> = rows.iter().collect();
        let notification = finished(&all[..2]).unwrap();
        assert_eq!(notification.title, "2 downloads finished");
        assert_eq!(notification.body, "file-1.zip and file-2.zip");
        assert_eq!(
            finished(&all[..3]).unwrap().body,
            "file-1.zip, file-2.zip and file-3.zip"
        );
        let notification = finished(&all).unwrap();
        assert_eq!(notification.body, "file-1.zip, file-2.zip and 2 more");
        assert_eq!(
            labels(&notification),
            [SHOW_IN_FOLDER],
            "they share a folder"
        );
        assert_eq!(notification.click, reveal(Some(QueueId(1)), &[1, 2, 3, 4]));
    }

    #[test]
    fn downloads_in_different_folders_are_shown_in_wirecatch() {
        let other = row(2, "videos", DownloadStatus::Completed);
        let notification = finished(&[&done(1), &other]).unwrap();
        assert_eq!(labels(&notification), [SHOW_IN_APP]);
    }

    #[test]
    fn a_failure_offers_to_try_again_only_when_that_can_fix_it() {
        let notification = failed(&[&broken(1, Some(FixAction::Retry))]).unwrap();
        assert_eq!(notification.title, "Download failed");
        assert_eq!(notification.body, "file-1.zip: The server refused.");
        assert_eq!(notification.buttons[0].act, Act::Retry(DownloadId(1)));
        assert_eq!(labels(&failed(&[&broken(1, None)]).unwrap()), [TRY_AGAIN]);
        let expired = failed(&[&broken(1, Some(FixAction::RefreshLink))]).unwrap();
        assert!(expired.buttons.is_empty(), "a fresh link needs the window");
        assert_eq!(expired.click, reveal(Some(QueueId(1)), &[1]));
        let two = failed(&[&broken(1, None), &broken(2, None)]).unwrap();
        assert_eq!(two.title, "2 downloads failed");
        assert_eq!(labels(&two), [SHOW_IN_APP]);
    }

    fn report(completed: Vec<DownloadSummary>, failed: Vec<DownloadSummary>) -> QueueReport {
        QueueReport {
            queue: QueueId(3),
            name: "Night".into(),
            completed,
            failed,
        }
    }

    #[test]
    fn a_queue_sums_up_what_finished_and_failed() {
        let notification =
            queue_done(&report(vec![done(1), done(2)], vec![broken(3, None)])).unwrap();
        assert_eq!(
            notification.title,
            "Night: 2 downloads finished and 1 failed"
        );
        assert_eq!(notification.body, "file-1.zip, file-2.zip and file-3.zip");
        assert_eq!(labels(&notification), [SHOW_IN_FOLDER]);
        assert_eq!(notification.click, reveal(Some(QueueId(3)), &[1, 2, 3]));
        let all_failed =
            queue_done(&report(vec![], vec![broken(1, None), broken(2, None)])).unwrap();
        assert_eq!(all_failed.title, "Night: 2 downloads failed");
        assert_eq!(labels(&all_failed), [SHOW_IN_APP]);
        assert_eq!(queue_done(&report(vec![], vec![])), None);
    }

    #[test]
    fn a_queue_run_of_one_download_reads_like_its_own_notice() {
        let notification = queue_done(&report(vec![done(1)], vec![])).unwrap();
        assert_eq!(notification.title, "Download finished");
        assert_eq!(labels(&notification), [OPEN, SHOW_IN_FOLDER]);
        assert_eq!(notification.click, reveal(Some(QueueId(3)), &[1]));
    }
}
