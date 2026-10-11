//! What the tray needs to know about downloads: how many run, how fast, and which just finished.
//! Kept up to date from the core's events, so nothing is polled.

use std::collections::HashMap;

use wirecatch_protocol::DownloadId;
use wirecatch_protocol::app::{DownloadStatus, DownloadSummary, UiEvent};

#[derive(Debug, Clone, Copy)]
struct Row {
    status: DownloadStatus,
    speed: u64,
}

/// The live state of every download, as far as the tray cares.
#[derive(Debug, Default)]
pub struct Activity {
    rows: HashMap<DownloadId, Row>,
}

/// A download that just finished.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finished {
    /// The download.
    pub id: DownloadId,
    /// Its file name, for the notification.
    pub file_name: String,
}

fn is_active(status: DownloadStatus) -> bool {
    matches!(
        status,
        DownloadStatus::Connecting | DownloadStatus::Downloading | DownloadStatus::Processing
    )
}

impl Activity {
    /// Starts over from the full list (at startup, and after missing events). Downloads that
    /// are already finished do not count as just finished.
    pub fn load(&mut self, rows: &[DownloadSummary]) {
        self.rows = rows
            .iter()
            .map(|r| {
                (
                    r.id,
                    Row {
                        status: r.status,
                        speed: r.speed,
                    },
                )
            })
            .collect();
    }

    /// Applies one core event and returns the downloads it finished.
    pub fn apply(&mut self, event: &UiEvent) -> Vec<Finished> {
        let mut finished = Vec::new();
        match event {
            UiEvent::DownloadsChanged(rows) => {
                for r in rows {
                    let before = self.rows.insert(
                        r.id,
                        Row {
                            status: r.status,
                            speed: r.speed,
                        },
                    );
                    let was_done = before.is_some_and(|b| b.status == DownloadStatus::Completed);
                    if r.status == DownloadStatus::Completed && !was_done {
                        finished.push(Finished {
                            id: r.id,
                            file_name: r.file_name.clone(),
                        });
                    }
                }
            }
            UiEvent::DownloadsRemoved(ids) => {
                for id in ids {
                    self.rows.remove(id);
                }
            }
            _ => {}
        }
        finished
    }

    /// Downloads moving bytes (or being joined) right now.
    pub fn active(&self) -> usize {
        self.rows.values().filter(|r| is_active(r.status)).count()
    }

    /// Combined speed of every running download, in bytes per second.
    pub fn speed(&self) -> u64 {
        self.rows
            .values()
            .filter(|r| is_active(r.status))
            .map(|r| r.speed)
            .sum()
    }

    /// The tray tooltip for the current state.
    pub fn tooltip(&self) -> String {
        tooltip(self.active(), self.speed())
    }
}

/// Bytes as a person reads them, matching the interface: `512 B`, `1.50 KB`, `12.4 MB`.
pub fn format_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        return format!("{bytes} B");
    }
    let digits = if value >= 100.0 {
        0
    } else if value >= 10.0 {
        1
    } else {
        2
    };
    format!("{value:.digits$} {}", UNITS[unit])
}

/// The tray tooltip: what runs and how fast.
pub fn tooltip(active: usize, speed: u64) -> String {
    match active {
        0 => "Wirecatch: nothing downloading".to_owned(),
        1 => format!("Wirecatch: 1 download, {}/s", format_bytes(speed)),
        n => format!("Wirecatch: {n} downloads, {}/s", format_bytes(speed)),
    }
}

/// Title and body of the notification for downloads that finished close together.
pub fn notification_text(finished: &[Finished]) -> Option<(String, String)> {
    match finished {
        [] => None,
        [one] => Some(("Download finished".to_owned(), one.file_name.clone())),
        [first, second] => Some((
            "2 downloads finished".to_owned(),
            format!("{} and {}", first.file_name, second.file_name),
        )),
        [first, rest @ ..] => Some((
            format!("{} downloads finished", finished.len()),
            format!("{} and {} more", first.file_name, rest.len()),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wirecatch_protocol::QueueId;
    use wirecatch_protocol::app::Category;

    fn row(id: i64, status: DownloadStatus, speed: u64) -> DownloadSummary {
        DownloadSummary {
            id: DownloadId(id),
            url: format!("https://example.com/{id}"),
            file_name: format!("file-{id}.zip"),
            path: format!("C:/Downloads/file-{id}.zip"),
            queue: QueueId(1),
            category: Category::Archive,
            status,
            total: Some(100),
            done: 0,
            speed,
            connections: 0,
            eta_secs: None,
            error: None,
            waiting: None,
            media: None,
            added_at: 0,
            completed_at: None,
            processing: None,
        }
    }

    #[test]
    fn counts_running_downloads_and_their_speed() {
        let mut a = Activity::default();
        a.load(&[
            row(1, DownloadStatus::Downloading, 1_000),
            row(2, DownloadStatus::Connecting, 0),
            row(3, DownloadStatus::Paused, 500),
            row(4, DownloadStatus::Completed, 0),
        ]);
        assert_eq!((a.active(), a.speed()), (2, 1_000));
        a.apply(&UiEvent::DownloadsChanged(vec![row(
            2,
            DownloadStatus::Downloading,
            3_000,
        )]));
        assert_eq!((a.active(), a.speed()), (2, 4_000));
        a.apply(&UiEvent::DownloadsRemoved(vec![DownloadId(1)]));
        assert_eq!((a.active(), a.speed()), (1, 3_000));
    }

    #[test]
    fn reports_each_download_once_when_it_finishes() {
        let mut a = Activity::default();
        a.load(&[
            row(1, DownloadStatus::Downloading, 10),
            row(2, DownloadStatus::Completed, 0),
        ]);
        let done = a.apply(&UiEvent::DownloadsChanged(vec![
            row(1, DownloadStatus::Completed, 0),
            row(2, DownloadStatus::Completed, 0),
        ]));
        assert_eq!(done.iter().map(|f| f.id.0).collect::<Vec<_>>(), [1]);
        // The same row again is not news.
        assert!(
            a.apply(&UiEvent::DownloadsChanged(vec![row(
                1,
                DownloadStatus::Completed,
                0
            )]))
            .is_empty()
        );
        // A row seen for the first time already finished (a tiny file) is news.
        let done = a.apply(&UiEvent::DownloadsChanged(vec![row(
            9,
            DownloadStatus::Completed,
            0,
        )]));
        assert_eq!(done.len(), 1);
    }

    #[test]
    fn tooltip_reads_plainly() {
        assert_eq!(tooltip(0, 0), "Wirecatch: nothing downloading");
        assert_eq!(tooltip(1, 1536), "Wirecatch: 1 download, 1.50 KB/s");
        assert_eq!(
            tooltip(3, 12 * 1024 * 1024 + 400 * 1024),
            "Wirecatch: 3 downloads, 12.4 MB/s"
        );
        assert_eq!(format_bytes(512), "512 B");
        assert_eq!(format_bytes(200 * 1024 * 1024), "200 MB");
    }

    #[test]
    fn several_finished_downloads_make_one_notification() {
        let f = |n: i64| Finished {
            id: DownloadId(n),
            file_name: format!("f{n}.zip"),
        };
        assert_eq!(notification_text(&[]), None);
        assert_eq!(
            notification_text(&[f(1)]),
            Some(("Download finished".into(), "f1.zip".into()))
        );
        assert_eq!(
            notification_text(&[f(1), f(2)]),
            Some(("2 downloads finished".into(), "f1.zip and f2.zip".into()))
        );
        assert_eq!(
            notification_text(&[f(1), f(2), f(3), f(4)]),
            Some(("4 downloads finished".into(), "f1.zip and 3 more".into()))
        );
    }
}
