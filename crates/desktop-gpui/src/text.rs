//! A download's numbers and states in short words, as its row says them.

use chrono::{Local, TimeZone as _};
use wirecatch_protocol::app::{
    DownloadStatus, DownloadSummary, PER_MILLE_DONE, ProcessingStep, TimeOfDay, WaitReason,
};
use wirecatch_shell::activity::format_bytes;

const SECONDS_PER_MINUTE: u64 = 60;
const MINUTES_PER_HOUR: u64 = 60;
const HOURS_PER_DAY: u64 = 24;
/// Up to two days, time left is said in hours and minutes; beyond that, in days.
const ETA_IN_HOURS_BELOW: u64 = 2 * HOURS_PER_DAY;
const PERCENT_MAX: u64 = 100;

/// Seconds left as a compact duration: `45 s`, `3 min`, `1 h 20 min`, `2 days`.
pub fn eta(seconds: u64) -> String {
    if seconds < SECONDS_PER_MINUTE {
        return format!("{} s", seconds.max(1));
    }
    let minutes = (seconds + SECONDS_PER_MINUTE / 2) / SECONDS_PER_MINUTE;
    if minutes < MINUTES_PER_HOUR {
        return format!("{minutes} min");
    }
    let hours = minutes / MINUTES_PER_HOUR;
    if hours < ETA_IN_HOURS_BELOW {
        let rest = minutes % MINUTES_PER_HOUR;
        return if rest == 0 {
            format!("{hours} h")
        } else {
            format!("{hours} h {rest} min")
        };
    }
    format!("{} days", (hours + HOURS_PER_DAY / 2) / HOURS_PER_DAY)
}

/// How full a download's bar is, in tenths of a percent (0 to [`PER_MILLE_DONE`]), or `None`
/// while it runs with no way to tell.
pub fn per_mille(row: &DownloadSummary) -> Option<u16> {
    match row.status {
        DownloadStatus::Completed => Some(PER_MILLE_DONE),
        DownloadStatus::Processing => row
            .processing
            .as_ref()
            .and_then(|p| p.per_mille)
            .map(|done| done.min(PER_MILLE_DONE)),
        _ => match row.total {
            Some(total) if total > 0 => {
                let done = row.done.min(total) * u64::from(PER_MILLE_DONE) / total;
                Some(u16::try_from(done).unwrap_or(PER_MILLE_DONE))
            }
            _ if matches!(
                row.status,
                DownloadStatus::Downloading | DownloadStatus::Connecting
            ) =>
            {
                None
            }
            _ => Some(0),
        },
    }
}

/// Work under way as a whole percentage, rounded down and never 100: everything may be
/// downloaded while the file is still written, and 100% would say it is done.
pub fn percent(row: &DownloadSummary) -> Option<String> {
    let total = row.total.filter(|total| *total > 0)?;
    if row.status == DownloadStatus::Completed || row.done >= total {
        return Some(format!("{PERCENT_MAX}%"));
    }
    Some(format!(
        "{}%",
        (row.done * PERCENT_MAX / total).min(PERCENT_MAX - 1)
    ))
}

/// `1.2 GB of 4.7 GB`, `4.7 GB`, or what is known so far.
pub fn size(row: &DownloadSummary) -> String {
    match row.total {
        None if row.done > 0 => format_bytes(row.done),
        None if matches!(
            row.status,
            DownloadStatus::Failed | DownloadStatus::Cancelled
        ) =>
        {
            String::new()
        }
        None => "Size unknown".to_owned(),
        Some(total)
            if row.status == DownloadStatus::Completed || row.done >= total || row.done == 0 =>
        {
            format_bytes(total)
        }
        Some(total) => format!("{} of {}", format_bytes(row.done), format_bytes(total)),
    }
}

/// A speed: `4.2 MB/s`.
pub fn speed(bytes_per_second: u64) -> String {
    format!("{}/s", format_bytes(bytes_per_second))
}

pub fn status_label(status: DownloadStatus) -> &'static str {
    match status {
        DownloadStatus::Queued => "Waiting",
        DownloadStatus::Connecting => "Connecting",
        DownloadStatus::Downloading => "Downloading",
        DownloadStatus::Paused => "Paused",
        DownloadStatus::Completed => "Finished",
        DownloadStatus::Failed => "Needs attention",
        DownloadStatus::Cancelled => "Cancelled",
        DownloadStatus::Processing => "Merging video and audio",
    }
}

fn processing_label(step: ProcessingStep) -> &'static str {
    match step {
        ProcessingStep::Joining => "Merging video and audio",
        ProcessingStep::Finishing => "Finishing the file",
        ProcessingStep::Converting => "Converting",
    }
}

/// The wait a row shows: why a queued download waits, or when a refused video tries again.
pub fn shown_wait(row: &DownloadSummary) -> Option<&WaitReason> {
    let wait = row.waiting.as_ref()?;
    match (row.status, wait) {
        (DownloadStatus::Queued, _)
        | (DownloadStatus::Connecting, WaitReason::ServerRefused { .. }) => Some(wait),
        _ => None,
    }
}

/// Why a download waits, in a couple of words.
pub fn wait_short(wait: &WaitReason) -> String {
    match wait {
        WaitReason::QueuePaused => "Queue paused".to_owned(),
        WaitReason::QueueSchedule {
            starts_at: Some(at),
        } => format!("Starts {}", time(*at)),
        WaitReason::QueueSchedule { starts_at: None } => "Outside schedule".to_owned(),
        WaitReason::QueueFull { .. } => "Waiting for a slot".to_owned(),
        WaitReason::GlobalLimit { .. } => "Global limit".to_owned(),
        WaitReason::InLine { position } => format!("{} in line", ordinal(*position)),
        WaitReason::ServerRefused { retry_at } => format!("Retrying at {}", clock(*retry_at)),
    }
}

/// A download's status in a couple of words; while ffmpeg works on it, which step and how far.
pub fn status_short(row: &DownloadSummary) -> String {
    if let Some(wait) = shown_wait(row) {
        return wait_short(wait);
    }
    if let (DownloadStatus::Processing, Some(processing)) = (row.status, &row.processing) {
        let label = processing_label(processing.step);
        return match per_mille(row) {
            Some(done) => format!("{label}, {}%", (done / 10).min(99)),
            None => label.to_owned(),
        };
    }
    status_label(row.status).to_owned()
}

/// The host of a link, for the line under a running download's name.
pub fn host(url: &str) -> &str {
    let rest = url.split_once("://").map_or(url, |(_, rest)| rest);
    let authority = rest.split(['/', '?', '#']).next().unwrap_or(rest);
    authority
        .rsplit_once('@')
        .map_or(authority, |(_, host)| host)
}

/// The first sentence of an error, for a row; the whole message shows in the details.
pub fn first_sentence(message: &str) -> &str {
    message.split_once(". ").map_or(message, |(first, _)| first)
}

fn time(at: TimeOfDay) -> String {
    format!("{:02}:{:02}", at.hour, at.minute)
}

/// The local clock time of a Unix time in milliseconds: `07:05`.
pub fn clock(ms: i64) -> String {
    Local
        .timestamp_millis_opt(ms)
        .single()
        .map_or_else(String::new, |at| at.format("%H:%M").to_string())
}

fn ordinal(n: u32) -> String {
    let suffix = match (n % 10, n % 100) {
        (_, 11..=13) => "th",
        (1, _) => "st",
        (2, _) => "nd",
        (3, _) => "rd",
        _ => "th",
    };
    format!("{n}{suffix}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn time_left_reads_like_a_person_says_it() {
        assert_eq!(eta(0), "1 s");
        assert_eq!(eta(45), "45 s");
        assert_eq!(eta(180), "3 min");
        assert_eq!(eta(80 * 60), "1 h 20 min");
        assert_eq!(eta(2 * 3600), "2 h");
        assert_eq!(eta(3 * 86_400), "3 days");
    }

    #[test]
    fn ordinals_follow_english() {
        let said: Vec<_> = [1, 2, 3, 4, 11, 12, 13, 21, 22, 101].map(ordinal).into();
        assert_eq!(
            said,
            [
                "1st", "2nd", "3rd", "4th", "11th", "12th", "13th", "21st", "22nd", "101st"
            ]
        );
    }

    #[test]
    fn the_host_leaves_out_the_user_and_the_path() {
        assert_eq!(
            host("https://user:pw@example.com:8080/a?b"),
            "example.com:8080"
        );
        assert_eq!(host("example.com/file"), "example.com");
    }
}
