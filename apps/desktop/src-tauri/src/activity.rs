//! What the tray needs to know about downloads: how many run, how fast, and which just finished.
//! Kept up to date from the core's events, so nothing is polled.

use std::collections::HashMap;

use wirecatch_engine::numeric::approx_f64;
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
    #[must_use]
    pub fn active(&self) -> usize {
        self.rows.values().filter(|r| is_active(r.status)).count()
    }

    /// Combined speed of every running download, in bytes per second.
    #[must_use]
    pub fn speed(&self) -> u64 {
        self.rows
            .values()
            .filter(|r| is_active(r.status))
            .map(|r| r.speed)
            .sum()
    }

    /// The tray tooltip for the current state.
    #[must_use]
    pub fn tooltip(&self) -> String {
        tooltip(self.active(), self.speed())
    }
}

/// Bytes as a person reads them, matching the interface: `512 B`, `1.50 KB`, `12.4 MB`.
///
/// Three significant digits: two decimals below 10, one below 100, none above. The unit and the
/// decimals are chosen for the value as shown, after rounding: 10239 B is `10.0 KB`, not
/// `10.00 KB`, and 1048575 B is `1.00 MB`, not `1024 KB`.
#[must_use]
pub fn format_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    const STEP: f64 = 1024.0;
    let mut value = approx_f64(bytes);
    let mut unit = 0;
    while value >= STEP && unit < UNITS.len() - 1 {
        value /= STEP;
        unit += 1;
    }
    if unit == 0 {
        return format!("{bytes} B");
    }
    let (mut shown, mut digits) = rounded(value);
    // Rounding up can reach the next unit: 1023.996 KB shows as 1.00 MB.
    if shown >= STEP && unit < UNITS.len() - 1 {
        unit += 1;
        (shown, digits) = rounded(value / STEP);
    }
    format!("{shown:.digits$} {}", UNITS[unit])
}

/// `value` rounded to three significant digits (at most two decimals), and the decimals to
/// show. Decimals are chosen again after rounding, since 9.996 rounds to 10.00 and 99.96 to
/// 100.0, which show with fewer.
fn rounded(value: f64) -> (f64, usize) {
    let decimals = |v: f64| {
        if v >= 100.0 {
            0
        } else if v >= 10.0 {
            1
        } else {
            2
        }
    };
    let round = |v: f64, digits: usize| {
        let scale = [1.0, 10.0, 100.0][digits];
        (v * scale).round() / scale
    };
    let first = decimals(value);
    let shown = round(value, first);
    let digits = decimals(shown);
    (round(value, digits), digits)
}

/// The tray tooltip: which app this is (by its channel's name), what runs and how fast.
#[must_use]
pub fn tooltip(active: usize, speed: u64) -> String {
    let name = crate::host::CHANNEL.name();
    match active {
        0 => format!("{name}: nothing downloading"),
        1 => format!("{name}: 1 download, {}/s", format_bytes(speed)),
        n => format!("{name}: {n} downloads, {}/s", format_bytes(speed)),
    }
}

/// Title and body of the notification for downloads that finished close together.
#[must_use]
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
    use wirecatch_engine::units::{KIB, MIB};
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
        assert_eq!(
            a.apply(&UiEvent::DownloadsChanged(vec![row(
                1,
                DownloadStatus::Completed,
                0
            )])),
            []
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
        // The running app names itself: "Wirecatch Dev" in a debug build.
        let name = crate::host::CHANNEL.name();
        assert_eq!(tooltip(0, 0), format!("{name}: nothing downloading"));
        assert_eq!(tooltip(1, 1536), format!("{name}: 1 download, 1.50 KB/s"));
        assert_eq!(
            tooltip(3, 12 * MIB + 400 * KIB),
            format!("{name}: 3 downloads, 12.4 MB/s")
        );
    }

    /// The sizes `formatBytes` in `@wirecatch/units` is held to, so both read alike.
    const SHARED_SIZE_CASES: &str =
        include_str!("../../../../packages/units/src/format-bytes.cases.json");

    #[derive(serde::Deserialize)]
    struct SizeCase {
        bytes: u64,
        shown: String,
    }

    /// Rounding can reach the next unit or need fewer decimals: the shown value
    /// is what is checked, never the value before rounding.
    #[test]
    fn sizes_read_as_in_the_units_package() {
        let cases: Vec<SizeCase> = serde_json::from_str(SHARED_SIZE_CASES).unwrap();
        assert!(!cases.is_empty());
        for case in cases {
            assert_eq!(format_bytes(case.bytes), case.shown, "{} bytes", case.bytes);
        }
        // Beyond what a JavaScript number holds exactly, so not a shared case.
        assert_eq!(format_bytes(u64::MAX), "16777216 TB");
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
