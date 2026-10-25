//! What the tray and the notices need to know about downloads: how many run, how fast, and
//! which just finished or failed.
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

/// Downloads that just finished or failed, as their rows were then.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Ended {
    /// Those that finished.
    pub completed: Vec<DownloadSummary>,
    /// Those that failed.
    pub failed: Vec<DownloadSummary>,
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

    /// Applies one core event and returns the downloads it finished or failed. A row seen for
    /// the first time already ended (a tiny file) counts too.
    pub fn apply(&mut self, event: &UiEvent) -> Ended {
        let mut ended = Ended::default();
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
                    if before.is_some_and(|b| b.status == r.status) {
                        continue;
                    }
                    match r.status {
                        DownloadStatus::Completed => ended.completed.push(r.clone()),
                        DownloadStatus::Failed => ended.failed.push(r.clone()),
                        _ => {}
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
        ended
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
/// Three significant digits: two decimals below 10, one below 100, none above, and never four
/// digits: a unit is 1024 of the one below, but a figure reads in the next unit from 1000 of its
/// own, so 1021 KB is `1.00 MB`. The unit and the decimals are chosen for the value as shown,
/// after rounding: 10239 B is `10.0 KB`, not `10.00 KB`, and 999.7 KB is `0.98 MB`, not
/// `1000 KB`.
#[must_use]
pub fn format_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    const STEP: f64 = 1024.0;
    const NEXT_UNIT_FROM: f64 = 1000.0;
    let mut value = approx_f64(bytes);
    let mut unit = 0;
    while value >= NEXT_UNIT_FROM && unit < UNITS.len() - 1 {
        value /= STEP;
        unit += 1;
    }
    if unit == 0 {
        return format!("{bytes} B");
    }
    let (mut shown, mut digits) = rounded(value);
    // Rounding up can reach four digits: 999.7 KB shows as 0.98 MB.
    if shown >= NEXT_UNIT_FROM && unit < UNITS.len() - 1 {
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
            queue_reason: wirecatch_protocol::claims::QueueReason::DefaultQueue,
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
        let ended = a.apply(&UiEvent::DownloadsChanged(vec![
            row(1, DownloadStatus::Completed, 0),
            row(2, DownloadStatus::Completed, 0),
        ]));
        assert_eq!(
            ended.completed.iter().map(|r| r.id.0).collect::<Vec<_>>(),
            [1]
        );
        // The same row again is not news.
        assert_eq!(
            a.apply(&UiEvent::DownloadsChanged(vec![row(
                1,
                DownloadStatus::Completed,
                0
            )])),
            Ended::default()
        );
        // A row seen for the first time already finished (a tiny file) is news.
        let ended = a.apply(&UiEvent::DownloadsChanged(vec![row(
            9,
            DownloadStatus::Completed,
            0,
        )]));
        assert_eq!(ended.completed.len(), 1);
    }

    #[test]
    fn reports_each_failure_once_and_again_after_a_retry() {
        let mut a = Activity::default();
        a.load(&[row(1, DownloadStatus::Downloading, 10)]);
        let failed = |a: &mut Activity| {
            a.apply(&UiEvent::DownloadsChanged(vec![row(
                1,
                DownloadStatus::Failed,
                0,
            )]))
            .failed
            .len()
        };
        assert_eq!(failed(&mut a), 1);
        assert_eq!(failed(&mut a), 0, "the same failure again is not news");
        a.apply(&UiEvent::DownloadsChanged(vec![row(
            1,
            DownloadStatus::Queued,
            0,
        )]));
        assert_eq!(failed(&mut a), 1);
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
}
