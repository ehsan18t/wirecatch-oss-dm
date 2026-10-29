//! The download complete window (D52): one small window, at the bottom right of the main screen,
//! that collects what finished or failed until the person closes it. It opens without taking
//! the keyboard focus and never closes by itself: the person chose it to see what finished when
//! they come back. It is a download window (its label starts with
//! [`crate::download_windows::LABEL_PREFIX`]), so it gets their capability, and it may act only
//! on the downloads it lists.

use std::sync::{Mutex, PoisonError};

use tauri::{
    AppHandle, Emitter as _, LogicalPosition, Manager as _, Runtime, WebviewUrl,
    WebviewWindowBuilder,
};
use wirecatch_protocol::DownloadId;
use wirecatch_protocol::app::{
    AppSettingsPatch, DownloadStatus, DownloadSummary, NotifyOnFinish, UiEvent,
};
use wirecatch_protocol::desktop::{
    DOWNLOAD_WINDOW_PAGE, FINISHED_EVENT, FINISHED_PARAMETER, FinishedGroup, FinishedList,
    FinishedQueue, MAX_FINISHED_GROUPS, MAX_FINISHED_LISTED,
};

use crate::download_windows::{self, MIN_WINDOW_SIZE, SCREEN_MARGIN, WINDOW_SIZE, fitted_height};
use crate::window::{show_quietly, warn_if_failed};

/// The window's label.
pub const LABEL: &str = "download-finished";

/// The window's page, relative to the app's address.
#[must_use]
pub fn route() -> String {
    format!("{DOWNLOAD_WINDOW_PAGE}?{FINISHED_PARAMETER}=1")
}

/// What the window lists, kept by the app while it is open.
#[derive(Debug, Default)]
pub struct FinishedWindow(Mutex<Book>);

#[derive(Debug, Default)]
struct Book {
    list: FinishedList,
    next_key: u32,
}

impl Book {
    /// Adds a group, newest first: downloads told one by one join the newest group when it is
    /// one of those too. A download listed before (it failed, was tried again and finished)
    /// leaves its old place. Returns whether anything was added.
    fn add(&mut self, queue: Option<FinishedQueue>, ended: &[DownloadSummary]) -> bool {
        if ended.is_empty() {
            return false;
        }
        for group in &mut self.list.groups {
            group
                .downloads
                .retain(|d| !ended.iter().any(|e| e.id == d.id));
        }
        self.list.groups.retain(|g| !g.downloads.is_empty());
        let newest_first = ended.iter().rev().cloned();
        match self.list.groups.first_mut() {
            Some(top) if queue.is_none() && top.queue.is_none() => {
                let mut downloads: Vec<_> = newest_first.collect();
                downloads.append(&mut top.downloads);
                downloads.truncate(MAX_FINISHED_LISTED);
                top.downloads = downloads;
            }
            _ => {
                self.next_key = self.next_key.wrapping_add(1);
                self.list.groups.insert(
                    0,
                    FinishedGroup {
                        key: self.next_key,
                        queue,
                        downloads: newest_first.take(MAX_FINISHED_LISTED).collect(),
                    },
                );
                self.list.groups.truncate(MAX_FINISHED_GROUPS);
            }
        }
        true
    }

    /// Follows the rows it lists: a change replaces the row, and a download removed from the
    /// list leaves it, so a newer list sent to the window never brings back an old row.
    fn follow(&mut self, event: &UiEvent) {
        if self.list.groups.is_empty() {
            return;
        }
        match event {
            UiEvent::DownloadsChanged(rows) => {
                for listed in self
                    .list
                    .groups
                    .iter_mut()
                    .flat_map(|g| g.downloads.iter_mut())
                {
                    if let Some(row) = rows.iter().find(|r| r.id == listed.id) {
                        listed.clone_from(row);
                    }
                }
            }
            UiEvent::DownloadsRemoved(ids) => {
                for group in &mut self.list.groups {
                    group.downloads.retain(|d| !ids.contains(&d.id));
                }
                self.list.groups.retain(|g| !g.downloads.is_empty());
            }
            _ => {}
        }
    }

    fn lists(&self, id: DownloadId) -> bool {
        self.list
            .groups
            .iter()
            .any(|g| g.downloads.iter().any(|d| d.id == id))
    }
}

impl FinishedWindow {
    fn book(&self) -> std::sync::MutexGuard<'_, Book> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// What the window lists now.
    #[must_use]
    pub fn list(&self) -> FinishedList {
        self.book().list.clone()
    }
}

/// The summary header of a queue that is done.
#[must_use]
pub fn queue_header(
    id: wirecatch_protocol::QueueId,
    name: &str,
    ended: &[DownloadSummary],
) -> FinishedQueue {
    let count = |status| {
        u32::try_from(ended.iter().filter(|d| d.status == status).count()).unwrap_or(u32::MAX)
    };
    FinishedQueue {
        id,
        name: name.to_owned(),
        completed: count(DownloadStatus::Completed),
        failed: count(DownloadStatus::Failed),
    }
}

/// Adds what ended to the window, opening it if it is closed. A download whose own progress
/// window is open is left out: that window shows it finished, so it is never shown twice.
pub fn add<R: Runtime>(
    app: &AppHandle<R>,
    queue: Option<FinishedQueue>,
    ended: &[DownloadSummary],
) {
    let Some(window) = app.try_state::<FinishedWindow>() else {
        return;
    };
    let ended: Vec<DownloadSummary> = ended
        .iter()
        .filter(|d| !download_windows::shows_download(app, d.id))
        .cloned()
        .collect();
    // Counted from what the window lists, so the heading agrees with the rows under it.
    let queue = queue.map(|q| queue_header(q.id, &q.name, &ended));
    let list = {
        let mut book = window.book();
        if !book.add(queue, &ended) {
            return;
        }
        book.list.clone()
    };
    let handle = app.clone();
    let run = app.run_on_main_thread(move || match handle.get_webview_window(LABEL) {
        Some(open) => {
            warn_if_failed(
                "update the download complete window",
                open.emit_to(LABEL, FINISHED_EVENT, &list),
            );
            show_quietly(&open);
        }
        None => create(&handle),
    });
    if let Err(e) = run {
        tracing::warn!("could not reach the window thread: {e}");
    }
}

/// Keeps what the window lists in step with the core's events.
pub fn follow<R: Runtime>(app: &AppHandle<R>, event: &UiEvent) {
    if let Some(window) = app.try_state::<FinishedWindow>() {
        window.book().follow(event);
    }
}

/// Whether the window `label` may make the settings change `patch`: only this window, and only
/// its "Use notifications instead" (D52). A switch to anything else, such as nothing at all, is
/// Settings' alone, since a script in a page showing a website's text must not silence failures.
#[must_use]
pub fn may_change(label: &str, patch: &AppSettingsPatch) -> bool {
    label == LABEL
        && *patch
            == AppSettingsPatch {
                notify_on_finish: Some(NotifyOnFinish::Notification),
                ..AppSettingsPatch::default()
            }
}

/// Whether the window `label` is this one and lists download `id`.
pub fn lists<R: Runtime>(app: &AppHandle<R>, label: &str, id: DownloadId) -> bool {
    label == LABEL
        && app
            .try_state::<FinishedWindow>()
            .is_some_and(|window| window.book().lists(id))
}

/// The window is gone: what it listed is forgotten.
pub fn gone<R: Runtime>(app: &AppHandle<R>, label: &str) {
    if label != LABEL {
        return;
    }
    if let Some(window) = app.try_state::<FinishedWindow>() {
        *window.book() = Book::default();
    }
}

/// The main screen's work area, in logical pixels: `(x, y, width, height)`.
fn work_area<R: Runtime>(window: &tauri::WebviewWindow<R>) -> Option<(f64, f64, f64, f64)> {
    let monitor = window.primary_monitor().ok().flatten()?;
    let scale = monitor.scale_factor();
    let work = monitor.work_area();
    Some((
        f64::from(work.position.x) / scale,
        f64::from(work.position.y) / scale,
        f64::from(work.size.width) / scale,
        f64::from(work.size.height) / scale,
    ))
}

/// Puts the window `height` tall at the bottom right of the main screen, a margin from its edges.
fn place<R: Runtime>(window: &tauri::WebviewWindow<R>, height: f64) {
    let Some((x, y, width, screen)) = work_area(window) else {
        return;
    };
    let height = fitted_height(height, Some(screen));
    let margin = SCREEN_MARGIN / 2.0;
    warn_if_failed(
        "size the download complete window",
        window.set_size(tauri::LogicalSize::new(WINDOW_SIZE.0, height)),
    );
    warn_if_failed(
        "place the download complete window",
        window.set_position(LogicalPosition::new(
            (x + width - WINDOW_SIZE.0 - margin).max(x),
            (y + screen - height - margin).max(y),
        )),
    );
}

/// Makes the window as tall as its content, keeping it at the bottom right.
pub fn fit<R: Runtime>(window: &tauri::WebviewWindow<R>, height: f64) {
    place(window, height);
}

fn create<R: Runtime>(app: &AppHandle<R>) {
    let title = app
        .config()
        .product_name
        .clone()
        .unwrap_or_else(|| crate::host::CHANNEL.name().to_owned());
    let builder = WebviewWindowBuilder::new(app, LABEL, WebviewUrl::App(route().into()));
    let built = crate::appearance::frameless(app, builder)
        .title(title)
        .inner_size(WINDOW_SIZE.0, MIN_WINDOW_SIZE.1)
        .min_inner_size(MIN_WINDOW_SIZE.0, MIN_WINDOW_SIZE.1)
        .resizable(true)
        .maximizable(false)
        .focused(false)
        .visible(false)
        .skip_taskbar(false)
        .disable_drag_drop_handler()
        .build();
    match built {
        Ok(window) => {
            crate::appearance::paint_background(&window);
            place(&window, MIN_WINDOW_SIZE.1);
            show_quietly(&window);
        }
        Err(e) => tracing::error!("could not open the download complete window: {e}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wirecatch_protocol::QueueId;
    use wirecatch_protocol::app::Category;

    fn ended(id: i64, status: DownloadStatus) -> DownloadSummary {
        DownloadSummary {
            id: DownloadId(id),
            url: String::new(),
            file_name: format!("file-{id}"),
            path: String::new(),
            queue: QueueId(1),
            queue_reason: wirecatch_protocol::rules::QueueReason::DefaultQueue,
            category: Category::Other,
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

    fn ids(group: &FinishedGroup) -> Vec<i64> {
        group.downloads.iter().map(|d| d.id.0).collect()
    }

    #[test]
    fn downloads_told_one_by_one_join_the_newest_group_newest_first() {
        let mut book = Book::default();
        assert!(!book.add(None, &[]));
        assert!(book.add(None, &[ended(1, DownloadStatus::Completed)]));
        assert!(book.add(
            None,
            &[
                ended(2, DownloadStatus::Completed),
                ended(3, DownloadStatus::Failed)
            ]
        ));
        let [group] = book.list.groups.as_slice() else {
            panic!("{:?}", book.list);
        };
        assert_eq!(ids(group), [3, 2, 1]);
        assert!(book.lists(DownloadId(2)) && !book.lists(DownloadId(4)));
    }

    #[test]
    fn a_queue_summary_is_a_group_of_its_own_above_the_rest() {
        let mut book = Book::default();
        book.add(None, &[ended(1, DownloadStatus::Completed)]);
        let rows = [
            ended(2, DownloadStatus::Completed),
            ended(3, DownloadStatus::Failed),
        ];
        book.add(Some(queue_header(QueueId(4), "Night", &rows)), &rows);
        book.add(None, &[ended(5, DownloadStatus::Completed)]);
        let groups = &book.list.groups;
        assert_eq!(
            groups.iter().map(ids).collect::<Vec<_>>(),
            [vec![5], vec![3, 2], vec![1]]
        );
        let night = groups[1].queue.as_ref().unwrap();
        assert_eq!(
            (night.name.as_str(), night.completed, night.failed),
            ("Night", 1, 1)
        );
        let keys: Vec<_> = groups.iter().map(|g| g.key).collect();
        assert_eq!(
            keys.len(),
            keys.iter().collect::<std::collections::BTreeSet<_>>().len()
        );
    }

    #[test]
    fn a_download_tried_again_moves_to_where_it_ended_last() {
        let mut book = Book::default();
        book.add(None, &[ended(1, DownloadStatus::Failed)]);
        let rows = [ended(1, DownloadStatus::Completed)];
        book.add(Some(queue_header(QueueId(4), "Night", &rows)), &rows);
        assert_eq!(
            book.list.groups.iter().map(ids).collect::<Vec<_>>(),
            [vec![1]]
        );
    }

    #[test]
    fn the_list_follows_what_happens_to_its_downloads() {
        let mut book = Book::default();
        book.add(
            None,
            &[
                ended(1, DownloadStatus::Failed),
                ended(2, DownloadStatus::Completed),
            ],
        );
        book.follow(&UiEvent::DownloadsChanged(vec![ended(
            1,
            DownloadStatus::Downloading,
        )]));
        assert_eq!(
            book.list.groups[0].downloads[1].status,
            DownloadStatus::Downloading
        );
        book.follow(&UiEvent::DownloadsRemoved(vec![
            DownloadId(1),
            DownloadId(2),
        ]));
        assert!(book.list.groups.is_empty(), "{:?}", book.list);
    }

    #[test]
    fn only_this_window_may_switch_to_notifications_and_nothing_else() {
        let to = |notification| AppSettingsPatch {
            notify_on_finish: Some(notification),
            ..AppSettingsPatch::default()
        };
        assert!(may_change(LABEL, &to(NotifyOnFinish::Notification)));
        assert!(!may_change(LABEL, &to(NotifyOnFinish::Nothing)));
        assert!(!may_change(
            "download-d1",
            &to(NotifyOnFinish::Notification)
        ));
    }

    #[test]
    fn the_window_keeps_a_bounded_list() {
        let mut book = Book::default();
        let many: Vec<_> = (0..300)
            .map(|n| ended(n, DownloadStatus::Completed))
            .collect();
        book.add(None, &many);
        assert_eq!(book.list.groups[0].downloads.len(), MAX_FINISHED_LISTED);
        for n in 0..50 {
            let rows = [ended(1000 + n, DownloadStatus::Completed)];
            book.add(Some(queue_header(QueueId(2), "Q", &rows)), &rows);
        }
        assert_eq!(book.list.groups.len(), MAX_FINISHED_GROUPS);
    }
}
