//! A download's numbers and states in short words, as its row says them.

use std::sync::atomic::{AtomicBool, Ordering};

use chrono::{Datelike as _, Local, NaiveDate, NaiveTime, TimeZone as _};
use wirecatch_protocol::UnixMillis;
use wirecatch_protocol::app::{
    Category, ClockFormat, DownloadStatus, DownloadSummary, FixAction, MediaSummary,
    PER_MILLE_DONE, ProcessingStep, Schedule, TimeOfDay, WaitReason, Weekday,
};
use wirecatch_protocol::extension::MediaKind;
use wirecatch_protocol::rules::{LinkProtocol, Rule};
use wirecatch_shell::activity::format_bytes;

const SECONDS_PER_MINUTE: u64 = 60;
const MINUTES_PER_HOUR: u64 = 60;
const HOURS_PER_DAY: u64 = 24;
/// Up to two days, time left is said in hours and minutes; beyond that, in days.
const ETA_IN_HOURS_BELOW: u64 = 2 * HOURS_PER_DAY;
const PERCENT_MAX: u64 = 100;

/// The clock format the settings last gave; 12-hour until they are read.
static TWENTY_FOUR_HOUR: AtomicBool = AtomicBool::new(false);

/// Every time the app shows follows `format` from now on.
pub fn set_clock(format: ClockFormat) {
    TWENTY_FOUR_HOUR.store(format == ClockFormat::TwentyFourHour, Ordering::Relaxed);
}

fn clock_format() -> ClockFormat {
    if TWENTY_FOUR_HOUR.load(Ordering::Relaxed) {
        ClockFormat::TwentyFourHour
    } else {
        ClockFormat::TwelveHour
    }
}

/// `time` in `format`: `3:42 PM` or `15:42`, with the seconds when `seconds`.
fn format_time(time: NaiveTime, seconds: bool, format: ClockFormat) -> String {
    let pattern = match (format, seconds) {
        (ClockFormat::TwelveHour, false) => "%-I:%M %p",
        (ClockFormat::TwelveHour, true) => "%-I:%M:%S %p",
        (ClockFormat::TwentyFourHour, false) => "%H:%M",
        (ClockFormat::TwentyFourHour, true) => "%H:%M:%S",
    };
    time.format(pattern).to_string()
}

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

/// What `rule` matches: `youtube.com`, `files named *.iso`, `.iso files`, `videos`.
pub fn rule_phrase(rule: &Rule) -> String {
    match rule {
        Rule::Site(site) => site.clone(),
        Rule::FileName(name) => format!("files named {name}"),
        Rule::FileType(extension) => format!(".{extension} files"),
        Rule::Category(category) => match category {
            Category::Video => "videos",
            Category::Audio => "music and audio",
            Category::Archive => "archives",
            Category::Document => "documents",
            Category::Program => "programs",
            Category::Image => "images",
            Category::Other => "other files",
        }
        .to_owned(),
        Rule::Protocol(protocol) => format!(
            "{} links",
            match protocol {
                LinkProtocol::Http => "HTTP",
                LinkProtocol::Https => "HTTPS",
                LinkProtocol::Ftp => "FTP",
            }
        ),
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

/// A saved folder as Settings shows it: `{Downloads}/Video` reads `Downloads/Video`.
pub fn folder(path: &str) -> String {
    match path.strip_prefix('{').and_then(|rest| rest.split_once('}')) {
        Some((name, rest)) if name.chars().all(char::is_alphanumeric) => format!("{name}{rest}"),
        _ => path.to_owned(),
    }
}

/// A moment for the details: `Today, 2:05:31 PM`, `Yesterday, 9:12:44 AM` or
/// `3 Oct 2026, 9:12:44 AM`, in the chosen clock format.
pub fn date(at: UnixMillis) -> String {
    let Some(at) = Local.timestamp_millis_opt(at.0).single() else {
        return String::new();
    };
    let today = Local::now().date_naive();
    let day = at.date_naive();
    let time = format_time(at.time(), true, clock_format());
    if day == today {
        format!("Today, {time}")
    } else if today.pred_opt() == Some(day) {
        format!("Yesterday, {time}")
    } else {
        format!("{}, {time}", at.format("%-d %b %Y"))
    }
}

/// The local day of a moment.
pub fn local_day(at: UnixMillis) -> Option<NaiveDate> {
    Local
        .timestamp_millis_opt(at.0)
        .single()
        .map(|at| at.date_naive())
}

/// A day over the downloads that finished on it: `Today`, `Yesterday`, `Thursday, 29 October`,
/// or `Thursday, 29 October 2025` in another year.
pub fn day_heading(day: NaiveDate) -> String {
    day_name(day, Local::now().date_naive(), "%A, %-d %B")
}

/// A day under a time, where it fits a narrow column: `Today`, `Yesterday`, `29 Oct`.
pub fn day_brief(day: NaiveDate) -> String {
    day_name(day, Local::now().date_naive(), "%-d %b")
}

fn day_name(day: NaiveDate, today: NaiveDate, format: &str) -> String {
    if day == today {
        "Today".to_owned()
    } else if today.pred_opt() == Some(day) {
        "Yesterday".to_owned()
    } else if day.year() == today.year() {
        day.format(format).to_string()
    } else {
        format!("{} {}", day.format(format), day.year())
    }
}

/// A moment's clock time to the second, split for a row: `3:42` and `:07 PM`.
pub fn clock_parts(at: UnixMillis) -> (String, String) {
    let Some(at) = Local.timestamp_millis_opt(at.0).single() else {
        return (String::new(), String::new());
    };
    split_seconds(&format_time(at.time(), true, clock_format()))
}

fn split_seconds(time: &str) -> (String, String) {
    let at = time.rfind(':').unwrap_or(time.len());
    (time[..at].to_owned(), time[at..].to_owned())
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

/// When a queue runs: `01:00 to 07:00, weekdays`, `From 01:00 until empty`, or `No schedule`.
pub fn schedule(schedule: Option<&Schedule>) -> String {
    let Some(schedule) = schedule else {
        return "No schedule".to_owned();
    };
    let hours = match schedule.stop {
        Some(stop) => format!("{} to {}", time(schedule.start), time(stop)),
        None => format!("From {} until empty", time(schedule.start)),
    };
    let days = &schedule.days;
    let has = |day: &Weekday| days.contains(day);
    let weekdays = [
        Weekday::Mon,
        Weekday::Tue,
        Weekday::Wed,
        Weekday::Thu,
        Weekday::Fri,
    ];
    let weekend = [Weekday::Sat, Weekday::Sun];
    if days.is_empty() {
        hours
    } else if days.len() == 5 && weekdays.iter().all(has) {
        format!("{hours}, weekdays")
    } else if days.len() == 2 && weekend.iter().all(has) {
        format!("{hours}, weekends")
    } else {
        let names: Vec<&str> = days.iter().map(|day| day_short(*day)).collect();
        format!("{hours}, {}", names.join(", "))
    }
}

fn day_short(day: Weekday) -> &'static str {
    match day {
        Weekday::Mon => "Mon",
        Weekday::Tue => "Tue",
        Weekday::Wed => "Wed",
        Weekday::Thu => "Thu",
        Weekday::Fri => "Fri",
        Weekday::Sat => "Sat",
        Weekday::Sun => "Sun",
    }
}

/// A time of day in the chosen clock format: `1:00 AM` or `01:00`.
pub fn time(at: TimeOfDay) -> String {
    NaiveTime::from_hms_opt(u32::from(at.hour), u32::from(at.minute), 0)
        .map_or_else(String::new, |time| format_time(time, false, clock_format()))
}

/// The local clock time of a moment: `7:05 AM` or `07:05`.
pub fn clock(at: UnixMillis) -> String {
    Local
        .timestamp_millis_opt(at.0)
        .single()
        .map_or_else(String::new, |at| {
            format_time(at.time(), false, clock_format())
        })
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
    fn a_day_reads_by_its_name_near_today() {
        let today = NaiveDate::from_ymd_opt(2026, 10, 31).unwrap();
        let day = |m, d| NaiveDate::from_ymd_opt(2026, m, d).unwrap();
        assert_eq!(day_name(today, today, "%A, %-d %B"), "Today");
        assert_eq!(day_name(day(10, 30), today, "%A, %-d %B"), "Yesterday");
        assert_eq!(
            day_name(day(10, 29), today, "%A, %-d %B"),
            "Thursday, 29 October"
        );
        let last_year = NaiveDate::from_ymd_opt(2025, 12, 31).unwrap();
        assert_eq!(day_name(last_year, today, "%-d %b"), "31 Dec 2025");
    }

    #[test]
    fn a_time_splits_before_its_seconds() {
        assert_eq!(
            split_seconds("3:42:07 PM"),
            ("3:42".to_owned(), ":07 PM".to_owned())
        );
        assert_eq!(
            split_seconds("15:42:07"),
            ("15:42".to_owned(), ":07".to_owned())
        );
    }

    #[test]
    fn a_time_reads_in_the_chosen_clock() {
        let time = NaiveTime::from_hms_opt(15, 42, 7).unwrap();
        let morning = NaiveTime::from_hms_opt(7, 5, 0).unwrap();
        assert_eq!(
            format_time(time, true, ClockFormat::TwelveHour),
            "3:42:07 PM"
        );
        assert_eq!(
            format_time(time, true, ClockFormat::TwentyFourHour),
            "15:42:07"
        );
        assert_eq!(
            format_time(morning, false, ClockFormat::TwelveHour),
            "7:05 AM"
        );
        assert_eq!(
            format_time(morning, false, ClockFormat::TwentyFourHour),
            "07:05"
        );
    }

    #[test]
    fn a_saved_folder_reads_without_its_braces() {
        assert_eq!(folder("{Downloads}/Video"), "Downloads/Video");
        assert_eq!(folder("D:/Media"), "D:/Media");
        assert_eq!(folder("{not a name}/x"), "{not a name}/x");
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
