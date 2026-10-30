//! A download's numbers and states in short words, as its row says them.

use chrono::{Local, TimeZone as _};
use wirecatch_protocol::app::{
    Category, DownloadStatus, DownloadSummary, FixAction, MediaSummary, PER_MILLE_DONE,
    ProcessingStep, TimeOfDay, WaitReason,
};
use wirecatch_protocol::extension::MediaKind;
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

/// What each processing step is, in a sentence.
pub fn processing_detail(step: ProcessingStep) -> &'static str {
    match step {
        ProcessingStep::Joining => {
            "The video and audio are downloaded; Wirecatch is merging them into one file."
        }
        ProcessingStep::Finishing => {
            "The download is done; Wirecatch is adding the extras you chose, such as subtitles or chapters. This takes a few seconds."
        }
        ProcessingStep::Converting => {
            "The download is done; Wirecatch is converting it to the format you chose. Pausing stops the conversion; the downloaded file is kept, and conversion starts over when you resume."
        }
    }
}

/// Why a download waits, in a full sentence for its details.
pub fn wait_long(wait: &WaitReason, queue: &str) -> String {
    match wait {
        WaitReason::QueuePaused => format!("{queue} is paused. Resume the queue to start it."),
        WaitReason::QueueSchedule {
            starts_at: Some(at),
        } => {
            format!("Waiting for {queue}, which starts at {}.", time(*at))
        }
        WaitReason::QueueSchedule { starts_at: None } => {
            format!("{queue} is outside its schedule.")
        }
        WaitReason::QueueFull { running, limit } => format!(
            "{queue} is running {running} of {limit}. This starts when one of them finishes."
        ),
        WaitReason::GlobalLimit { running, limit } => format!(
            "Global limit: {running} of {limit} downloads are running. This starts when one of them finishes."
        ),
        WaitReason::InLine { position } => format!("{} in line in {queue}.", ordinal(*position)),
        WaitReason::ServerRefused { retry_at } => format!(
            "The video's server refused it for now, which sites sometimes do for a few minutes. Wirecatch tries again automatically at {} and keeps what was already downloaded.",
            clock(*retry_at)
        ),
    }
}

/// What a download is doing in a sentence, when there is more to say than its status.
pub fn status_detail(row: &DownloadSummary, queue: &str) -> Option<String> {
    if let Some(wait) = shown_wait(row) {
        return Some(wait_long(wait, queue));
    }
    (row.status == DownloadStatus::Processing).then(|| {
        processing_detail(
            row.processing
                .as_ref()
                .map_or(ProcessingStep::Joining, |p| p.step),
        )
        .to_owned()
    })
}

/// What a media download is: its quality, where it came from, and the resolution it stands in
/// for when it was picked without asking (D50).
pub fn media_line(media: &MediaSummary) -> String {
    let from = match media.source {
        MediaKind::Hls | MediaKind::Dash => "from a video stream",
        MediaKind::Direct => "from a media file",
        MediaKind::Page => "from a video page",
    };
    let line = format!("{}, {from}.", media.quality);
    match &media.instead_of {
        Some(wanted) => format!("{line} {wanted} wasn’t offered."),
        None => line,
    }
}

/// The fix a failure offers, as its button says it.
pub fn fix_label(fix: FixAction) -> &'static str {
    match fix {
        FixAction::Retry => "Try again",
        FixAction::RefreshLink => "Get a new link",
        FixAction::ChooseFolder => "Change folder",
        FixAction::FreeDiskSpace => "Open folder",
        FixAction::KeepOriginal => "Keep the original",
        FixAction::ProxySettings => "Proxy settings",
        FixAction::ChangeQueue => "Change queue",
    }
}

/// The kind of file: its type in capitals (`ZIP file`), or the category without one.
pub fn file_kind(name: &str, category: Category) -> String {
    match name.rsplit_once('.') {
        Some((stem, ext))
            if !stem.is_empty()
                && (1..=8).contains(&ext.len())
                && ext.chars().all(|c| c.is_ascii_alphanumeric()) =>
        {
            format!("{} file", ext.to_ascii_uppercase())
        }
        _ => category_label(category).to_owned(),
    }
}

pub fn category_label(category: Category) -> &'static str {
    match category {
        Category::Video => "Video",
        Category::Audio => "Music and audio",
        Category::Archive => "Archives",
        Category::Document => "Documents",
        Category::Program => "Programs",
        Category::Image => "Images",
        Category::Other => "Other files",
    }
}

/// A moment for the details: `Today, 14:05`, `Yesterday, 09:12` or `3 Oct 2026, 09:12`.
pub fn date(ms: i64) -> String {
    let Some(at) = Local.timestamp_millis_opt(ms).single() else {
        return String::new();
    };
    let today = Local::now().date_naive();
    let day = at.date_naive();
    let time = at.format("%H:%M");
    if day == today {
        format!("Today, {time}")
    } else if today.pred_opt() == Some(day) {
        format!("Yesterday, {time}")
    } else {
        format!("{}, {time}", at.format("%-d %b %Y"))
    }
}

/// How long ago a moment was, for a row: `Just now`, `5 min ago`, `3 h ago`, `Yesterday` or
/// `3 Oct`.
pub fn ago(ms: i64) -> String {
    ago_from(ms, Local::now())
}

fn ago_from(ms: i64, now: chrono::DateTime<Local>) -> String {
    let Some(at) = Local.timestamp_millis_opt(ms).single() else {
        return String::new();
    };
    let minutes = (now - at).num_minutes();
    let day = at.date_naive();
    let today = now.date_naive();
    if minutes < 1 {
        "Just now".to_owned()
    } else if minutes < 60 {
        format!("{minutes} min ago")
    } else if day == today {
        format!("{} h ago", minutes / 60)
    } else if today.pred_opt() == Some(day) {
        "Yesterday".to_owned()
    } else if at.format("%Y").to_string() == now.format("%Y").to_string() {
        at.format("%-d %b").to_string()
    } else {
        at.format("%-d %b %Y").to_string()
    }
}

/// A link as it may be shown or copied: a password in it becomes `***`, and a user name without
/// one too, since some sites put a token there.
pub fn link_for_display(url: &str) -> String {
    let Some((scheme, rest)) = url.split_once("://") else {
        return url.to_owned();
    };
    let end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let Some(at) = rest[..end].rfind('@') else {
        return url.to_owned();
    };
    let masked = rest[..at]
        .split_once(':')
        .map_or_else(|| "***".to_owned(), |(name, _)| format!("{name}:***"));
    format!("{scheme}://{masked}{}", &rest[at..])
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
    fn a_finished_moment_reads_as_how_long_ago() {
        let now = Local.with_ymd_and_hms(2026, 10, 30, 15, 0, 0).unwrap();
        let at = |y, mo, d, h, mi| {
            Local
                .with_ymd_and_hms(y, mo, d, h, mi, 0)
                .unwrap()
                .timestamp_millis()
        };
        assert_eq!(ago_from(at(2026, 10, 30, 14, 59), now), "1 min ago");
        assert_eq!(ago_from(now.timestamp_millis(), now), "Just now");
        assert_eq!(ago_from(at(2026, 10, 30, 11, 30), now), "3 h ago");
        assert_eq!(ago_from(at(2026, 10, 29, 22, 0), now), "Yesterday");
        assert_eq!(ago_from(at(2026, 10, 3, 9, 0), now), "3 Oct");
        assert_eq!(ago_from(at(2025, 12, 31, 9, 0), now), "31 Dec 2025");
    }

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
    fn a_password_in_a_link_is_never_shown() {
        assert_eq!(
            link_for_display("https://ada:secret@example.com/a"),
            "https://ada:***@example.com/a"
        );
        assert_eq!(
            link_for_display("https://token@example.com/a@b"),
            "https://***@example.com/a@b"
        );
        assert_eq!(
            link_for_display("https://example.com/a"),
            "https://example.com/a"
        );
    }

    #[test]
    fn the_kind_is_the_type_or_the_category() {
        assert_eq!(file_kind("setup.exe", Category::Program), "EXE file");
        assert_eq!(file_kind("README", Category::Document), "Documents");
        assert_eq!(file_kind(".hidden", Category::Other), "Other files");
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
