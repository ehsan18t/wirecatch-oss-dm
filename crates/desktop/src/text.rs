//! A download's numbers and states in short words, as its row says them.

use std::sync::atomic::{AtomicBool, Ordering};

use chrono::{Datelike as _, Local, NaiveDate, NaiveTime, TimeZone as _};
use gpui_kit::component::time_field::HourCycle;
use wirecatch_protocol::UnixMillis;
use wirecatch_protocol::app::{
    Category, ClockFormat, DownloadStatus, DownloadSummary, FileExistsQuestion, FileTypeQuestion,
    FixAction, MediaSummary, OutputFormat, PER_MILLE_DONE, ProcessingStep, QueueSettings, Schedule,
    SuffixListInfo, TimeOfDay, WaitReason, Weekday,
};
use wirecatch_protocol::extension::MediaKind;
use wirecatch_protocol::rules::{AddDownloads, LinkProtocol, Rule};
use wirecatch_shell::activity::format_bytes;
use wirecatch_shell::host;

use crate::ui::Sentence;

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

/// How a time field counts the hours, in the chosen clock format.
pub fn hour_cycle() -> HourCycle {
    hour_cycle_of(clock_format())
}

fn hour_cycle_of(format: ClockFormat) -> HourCycle {
    match format {
        ClockFormat::TwelveHour => HourCycle::H12,
        ClockFormat::TwentyFourHour => HourCycle::H23,
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
    if row.status == DownloadStatus::Completed {
        return Some(format!("{PERCENT_MAX}%"));
    }
    Some(format!(
        "{}%",
        (row.done * PERCENT_MAX / total).min(PERCENT_MAX - 1)
    ))
}

/// The lines of a typed list that hold something.
pub fn filled_lines(typed: &str) -> Vec<String> {
    typed
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_owned)
        .collect()
}

/// A typed list of file types as they are kept: without the dot, in lower case.
pub fn typed_extensions(typed: &str) -> Vec<String> {
    typed
        .split([',', ';', ' ', '\n'])
        .map(|typed| typed.trim().trim_start_matches('.').to_lowercase())
        .filter(|extension| !extension.is_empty())
        .collect()
}

/// A queue's own limits in words: how many at once, its speed, its connections.
pub fn own_limits(settings: &QueueSettings) -> [Option<String>; 3] {
    [
        settings
            .max_concurrent
            .own()
            .map(|n| format!("{n} at once")),
        settings
            .speed_limit
            .own()
            .map(|bytes| format!("Up to {}", speed(bytes))),
        settings.connections.own().map(|n| match n {
            1 => "1 connection each".to_owned(),
            n => format!("{n} connections each"),
        }),
    ]
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
        Some(total) => bytes_of(row.done, total),
    }
}

/// How much of `total` is done: `1.2 GB of 4.7 GB`.
pub fn bytes_of(done: u64, total: u64) -> String {
    format!("{} of {}", format_bytes(done), format_bytes(total))
}

/// A speed: `4.2 MB/s`.
pub fn speed(bytes_per_second: u64) -> String {
    format!("{}/s", format_bytes(bytes_per_second))
}

pub use wirecatch_shell::tray::{PAUSE_ALL, RESUME_ALL};

/// Pauses or resumes one queue: its header and its menu.
pub const PAUSE_QUEUE: &str = "Pause queue";
pub const RESUME_QUEUE: &str = "Resume queue";

/// Adds a download without starting it: the New download dialog and the download windows.
pub const DOWNLOAD_LATER: &str = "Download later";

pub fn status_label(status: DownloadStatus) -> &'static str {
    match status {
        DownloadStatus::Queued => "Waiting",
        DownloadStatus::Connecting => "Connecting",
        DownloadStatus::Downloading => "Downloading",
        DownloadStatus::Paused => "Paused",
        DownloadStatus::Completed => "Finished",
        DownloadStatus::Failed => "Needs attention",
        DownloadStatus::Cancelled => "Cancelled",
        DownloadStatus::Processing => "Processing",
    }
}

/// What a row is doing, with the step of a download being processed.
pub fn status_name(row: &DownloadSummary) -> &'static str {
    match (row.status, &row.processing) {
        (DownloadStatus::Processing, Some(processing)) => processing_label(processing.step),
        _ => status_label(row.status),
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
        | (DownloadStatus::Connecting, WaitReason::ServerRefused { .. })
        | (DownloadStatus::Downloading, WaitReason::Reconnecting { .. }) => Some(wait),
        _ => None,
    }
}

/// Where "Remove and delete" sends a finished file (D231).
pub const TRASH: &str = if cfg!(target_os = "linux") {
    "the trash"
} else {
    "the Recycle Bin"
};

/// What to say when `left` finished files could not go to [`TRASH`]; nothing when none.
pub fn left_in_place(left: usize) -> Option<String> {
    match left {
        0 => None,
        1 => Some(format!(
            "1 file could not go to {TRASH} and is still in its folder."
        )),
        n => Some(format!(
            "{n} files could not go to {TRASH} and are still in their folder."
        )),
    }
}

/// The quality window's switch and its setting (D200).
pub const REMEMBER_CHOICES: &str = "Remember these choices";

/// The quality window's switch and its setting (D50).
pub const ASK_EACH_VIDEO: &str = "Ask before each video";

/// The formats a video can be saved as, in the order a menu lists them (`OutputFormat::saves_video`).
pub const VIDEO_FORMATS: &[(OutputFormat, &str)] = &[
    (OutputFormat::Original, "Original"),
    (OutputFormat::Mp4, "MP4"),
    (OutputFormat::Mkv, "MKV"),
    (OutputFormat::Webm, "WebM"),
];
/// The formats audio can be saved as, in the order a menu lists them (`OutputFormat::saves_audio`).
pub const AUDIO_FORMATS: &[(OutputFormat, &str)] = &[
    (OutputFormat::Original, "Original"),
    (OutputFormat::Mp3, "MP3"),
    (OutputFormat::M4a, "M4A"),
    (OutputFormat::Opus, "Opus"),
    (OutputFormat::Flac, "FLAC"),
    (OutputFormat::Wav, "WAV"),
];

/// A format's name.
pub fn format_name(format: OutputFormat) -> &'static str {
    VIDEO_FORMATS
        .iter()
        .chain(AUDIO_FORMATS)
        .find(|(each, _)| *each == format)
        .map_or("Original", |(_, name)| name)
}

/// Why a download waits, in a couple of words, naming the queue it waits in (D47.2):
/// `Next in Main`, `3rd to start in Main`. `queue` is `None` while the queue is not known yet.
pub fn wait_short(wait: &WaitReason, queue: Option<&str>) -> Sentence {
    let said = Sentence::new();
    match wait {
        WaitReason::QueuePaused => named(said, queue, true).text(" is paused"),
        WaitReason::QueueSchedule {
            starts_at: Some(at),
        } => named(said, queue, true).text(&format!(" starts at {}", time(*at))),
        WaitReason::QueueSchedule { starts_at: None } => {
            named(said, queue, true).text(" is outside its schedule")
        }
        WaitReason::QueueFull { .. } | WaitReason::GlobalLimit { .. } => {
            named(said.text("Next in "), queue, false).text(", waiting for a free slot")
        }
        WaitReason::InLine { position } => named(said.text(&place(*position)), queue, false),
        WaitReason::ServerRefused { retry_at } => {
            said.text(&format!("Retrying at {}", clock(*retry_at)))
        }
        WaitReason::Reconnecting { attempt, of } => {
            said.text(&format!("Reconnecting (try {attempt} of {of})"))
        }
    }
}

/// Where a download stands in its queue's line, up to the queue's name: `Next in `,
/// `3rd to start in `.
fn place(position: u32) -> String {
    match position {
        0 | 1 => "Next in ".to_owned(),
        n => format!("{} to start in ", ordinal(n)),
    }
}

/// `said` followed by the queue's name, standing out (D240), or by "its queue" while the queue is
/// not known.
fn named(said: Sentence, queue: Option<&str>, starts: bool) -> Sentence {
    match queue {
        Some(name) => said.queue(name),
        None if starts => said.text("Its queue"),
        None => said.text("its queue"),
    }
}

/// A download's status in a couple of words; while ffmpeg works on it, which step and how far.
pub fn status_short(row: &DownloadSummary, queue: Option<&str>) -> Sentence {
    if let Some(wait) = shown_wait(row) {
        return wait_short(wait, queue);
    }
    if let (DownloadStatus::Processing, Some(processing)) = (row.status, &row.processing) {
        let label = processing_label(processing.step);
        return match per_mille(row) {
            Some(done) => format!("{label}, {}%", (done / 10).min(99)).into(),
            None => label.into(),
        };
    }
    status_label(row.status).into()
}

/// What each processing step is, in a sentence.
pub fn processing_detail(step: ProcessingStep) -> String {
    let app = host::CHANNEL.name();
    match step {
        ProcessingStep::Joining => {
            format!("The video and audio are downloaded; {app} is merging them into one file.")
        }
        ProcessingStep::Finishing => format!(
            "The download is done; {app} is adding the extras you chose, such as subtitles or chapters. This takes a few seconds."
        ),
        ProcessingStep::Converting => format!(
            "The download is done; {app} is converting it to the format you chose. Pausing stops the conversion; the downloaded file is kept, and conversion starts over when you resume."
        ),
    }
}

/// Why a download waits, in a full sentence for its details.
pub fn wait_long(wait: &WaitReason, queue: Option<&str>) -> Sentence {
    let said = Sentence::new();
    match wait {
        WaitReason::QueuePaused => {
            named(said, queue, true).text(" is paused. Resume the queue to start it.")
        }
        WaitReason::QueueSchedule {
            starts_at: Some(at),
        } => named(said.text("Waiting for "), queue, false)
            .text(&format!(", which starts at {}.", time(*at))),
        WaitReason::QueueSchedule { starts_at: None } => {
            named(said, queue, true).text(" is outside its schedule.")
        }
        WaitReason::QueueFull { running, limit } => named(said, queue, true).text(&format!(
            " is running {running} of {limit}. This starts when one of them finishes."
        )),
        WaitReason::GlobalLimit { running, limit } => named(said.text("Next in "), queue, false)
            .text(&format!(
                ": {running} of {limit} downloads are running. This starts when one of them finishes."
            )),
        WaitReason::InLine { position } => {
            named(said.text(&place(*position)), queue, false).text(".")
        }
        WaitReason::ServerRefused { retry_at } => format!(
            "The video's server refused it for now, which sites sometimes do for a few minutes. {} tries again automatically at {} and keeps what was already downloaded.",
            host::CHANNEL.name(),
            clock(*retry_at)
        )
        .into(),
        WaitReason::Reconnecting { attempt, of } => format!(
            "The connection to the server was lost. {} is reconnecting (try {attempt} of {of}) and keeps what was already downloaded.",
            host::CHANNEL.name()
        )
        .into(),
    }
}

/// What a download is doing in a sentence, when there is more to say than its status.
pub fn status_detail(row: &DownloadSummary, queue: Option<&str>) -> Option<Sentence> {
    if let Some(wait) = shown_wait(row) {
        return Some(wait_long(wait, queue));
    }
    (row.status == DownloadStatus::Processing).then(|| {
        processing_detail(
            row.processing
                .as_ref()
                .map_or(ProcessingStep::Joining, |p| p.step),
        )
        .into()
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

/// An auto queue's rules in a few words: `Adds youtube.com and 2 more`.
pub fn rules_fact(add: &AddDownloads) -> Option<String> {
    let AddDownloads::Auto(rules) = add else {
        return None;
    };
    if !rules.enabled {
        return Some("Rules off".to_owned());
    }
    let all: Vec<Rule> = rules
        .sites
        .iter()
        .cloned()
        .map(Rule::Site)
        .chain(rules.file_names.iter().cloned().map(Rule::FileName))
        .chain(rules.file_types.iter().cloned().map(Rule::FileType))
        .chain(rules.categories.iter().copied().map(Rule::Category))
        .chain(rules.protocols.iter().copied().map(Rule::Protocol))
        .collect();
    let first = all.first()?;
    Some(match all.len() - 1 {
        0 => format!("Adds {}", rule_phrase(first)),
        more => format!("Adds {} and {more} more", rule_phrase(first)),
    })
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

/// What is known of the Public Suffix List in use: `List published 3 Nov 2026. Last checked 4 Nov 2026.`
pub fn suffix_list_said(list: &SuffixListInfo) -> String {
    let published = whole_day(list.published);
    match list.checked {
        Some(checked) => format!(
            "List published {published}. Last checked {}.",
            whole_day(checked)
        ),
        None => format!("List published {published}. Not checked yet."),
    }
}

/// A moment's local day, year included: `3 Nov 2026`.
fn whole_day(at: UnixMillis) -> String {
    Local
        .timestamp_millis_opt(at.0)
        .single()
        .map_or_else(String::new, |at| at.format("%-d %b %Y").to_string())
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

/// What a question's window says of the download already in the list for its link (D23):
/// `Already downloaded yesterday at 7:05 AM`, `Already in your list, paused`.
pub fn earlier_notice(row: &DownloadSummary) -> String {
    earlier_notice_on(row, Local::now().date_naive(), clock_format())
}

fn earlier_notice_on(row: &DownloadSummary, today: NaiveDate, clock: ClockFormat) -> String {
    let when = |at| when_said(at, today, clock);
    match row.status {
        DownloadStatus::Completed => format!(
            "Already downloaded {}",
            when(row.completed_at.unwrap_or(row.added_at))
        ),
        DownloadStatus::Connecting | DownloadStatus::Downloading | DownloadStatus::Processing => {
            "Already downloading".to_owned()
        }
        DownloadStatus::Queued => "Already in your list, queued".to_owned(),
        DownloadStatus::Paused => "Already in your list, paused".to_owned(),
        DownloadStatus::Failed => "Already in your list, failed".to_owned(),
        DownloadStatus::Cancelled => format!("Started {}, then cancelled", when(row.added_at)),
    }
}

/// Where a paused download got to, once it got anywhere; its row's time column says Paused.
pub fn paused_at(row: &DownloadSummary) -> Option<String> {
    (row.done > 0).then(|| size(row))
}

/// What the question about a file already in the folder says of that file (D96).
pub fn file_exists_said(question: &FileExistsQuestion) -> String {
    file_exists_said_on(question, Local::now().date_naive(), clock_format())
}

fn file_exists_said_on(
    question: &FileExistsQuestion,
    today: NaiveDate,
    clock: ClockFormat,
) -> String {
    let which = if question.same_file {
        "A file that looks like the same one"
    } else {
        "Another file with this name"
    };
    let bytes = format_bytes(question.size);
    let facts = match question.saved_at {
        Some(at) => format!("{bytes}, saved {}", when_said(at, today, clock)),
        None => bytes,
    };
    format!(
        "{which} is already in {} ({facts}).",
        folder(&question.folder)
    )
}

/// What the question about a server's name of another file type says (D166).
pub fn file_type_said(question: &FileTypeQuestion) -> String {
    format!(
        "The server named this file {}, which is a different type of file from {}. Which name do you want to save it under?",
        question.server_name, question.confirmed_name
    )
}

/// A moment in a sentence: `today at 6:38 PM`, `yesterday at 7:05 AM` or `on 3 Sep 2026`.
fn when_said(at: UnixMillis, today: NaiveDate, clock: ClockFormat) -> String {
    let Some(at) = Local.timestamp_millis_opt(at.0).single() else {
        return "earlier".to_owned();
    };
    let day = at.date_naive();
    let time = format_time(at.time(), false, clock);
    if day == today {
        format!("today at {time}")
    } else if today.pred_opt() == Some(day) {
        format!("yesterday at {time}")
    } else {
        at.format("on %-d %b %Y").to_string()
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

/// What a queue's tile says while it runs: `3 running · 2 waiting`, or with nothing waiting
/// `3 running · 4 min left`.
pub fn queue_running(running: usize, waiting: usize, left_secs: Option<u64>) -> String {
    match (waiting, left_secs) {
        (1.., _) => format!("{running} running · {waiting} waiting"),
        (0, Some(left)) => format!("{running} running · {} left", eta(left)),
        (0, None) => format!("{running} running"),
    }
}

/// The local clock time of a moment: `7:05 AM` or `07:05`.
pub fn clock(at: UnixMillis) -> String {
    clock_in(at, false, clock_format())
}

/// The local clock time of a moment to the second, as the list shows it: `7:05:31 AM` or `07:05:31`.
pub fn clock_to_second(at: UnixMillis) -> String {
    clock_in(at, true, clock_format())
}

fn clock_in(at: UnixMillis, seconds: bool, format: ClockFormat) -> String {
    Local
        .timestamp_millis_opt(at.0)
        .single()
        .map_or_else(String::new, |at| format_time(at.time(), seconds, format))
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
    fn an_auto_queue_names_its_first_rule_and_counts_the_rest() {
        use wirecatch_protocol::rules::QueueRules;
        let rules = |enabled| {
            AddDownloads::Auto(QueueRules {
                enabled,
                sites: vec!["youtube.com".into(), "vimeo.com".into()],
                file_types: vec!["iso".into()],
                ..QueueRules::default()
            })
        };
        assert_eq!(
            rules_fact(&rules(true)).as_deref(),
            Some("Adds youtube.com and 2 more")
        );
        assert_eq!(rules_fact(&rules(false)).as_deref(), Some("Rules off"));
        assert_eq!(rules_fact(&AddDownloads::Manual), None);
    }

    #[test]
    fn files_the_trash_refused_are_counted_once_there_are_any() {
        assert_eq!(left_in_place(0), None);
        assert_eq!(
            left_in_place(1),
            Some(format!(
                "1 file could not go to {TRASH} and is still in its folder."
            ))
        );
        assert_eq!(
            left_in_place(2),
            Some(format!(
                "2 files could not go to {TRASH} and are still in their folder."
            ))
        );
    }

    #[test]
    fn the_suffix_list_says_its_days_or_that_it_was_never_checked() {
        let noon = |day| {
            UnixMillis(
                Local
                    .with_ymd_and_hms(2026, 11, day, 12, 0, 0)
                    .single()
                    .expect("noon exists")
                    .timestamp_millis(),
            )
        };
        let list = |checked| SuffixListInfo {
            published: noon(3),
            checked,
            check_failed: false,
        };
        assert_eq!(
            suffix_list_said(&list(Some(noon(4)))),
            "List published 3 Nov 2026. Last checked 4 Nov 2026."
        );
        assert_eq!(
            suffix_list_said(&list(None)),
            "List published 3 Nov 2026. Not checked yet."
        );
    }

    #[test]
    fn an_unfinished_download_never_reads_100() {
        let row = |status, done| DownloadSummary {
            status,
            done,
            total: Some(100),
            ..wirecatch_test_support::downloads::download(1)
        };
        assert_eq!(
            percent(&row(DownloadStatus::Processing, 100)).as_deref(),
            Some("99%")
        );
        assert_eq!(
            percent(&row(DownloadStatus::Downloading, 150)).as_deref(),
            Some("99%")
        );
        assert_eq!(
            percent(&row(DownloadStatus::Downloading, 50)).as_deref(),
            Some("50%")
        );
        assert_eq!(
            percent(&row(DownloadStatus::Completed, 100)).as_deref(),
            Some("100%")
        );
    }

    #[test]
    fn the_status_names_the_processing_step() {
        let row = |step| DownloadSummary {
            status: DownloadStatus::Processing,
            processing: Some(wirecatch_protocol::app::ProcessingProgress {
                step,
                per_mille: None,
            }),
            ..wirecatch_test_support::downloads::download(1)
        };
        assert_eq!(
            status_name(&row(ProcessingStep::Joining)),
            "Merging video and audio"
        );
        assert_eq!(
            status_name(&row(ProcessingStep::Finishing)),
            "Finishing the file"
        );
        assert_eq!(status_name(&row(ProcessingStep::Converting)), "Converting");
    }

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
    fn a_time_field_counts_the_hours_of_the_chosen_clock() {
        assert_eq!(hour_cycle_of(ClockFormat::TwelveHour), HourCycle::H12);
        assert_eq!(hour_cycle_of(ClockFormat::TwentyFourHour), HourCycle::H23);
    }

    #[test]
    fn a_moment_to_the_second_keeps_its_seconds() {
        let at = UnixMillis(1_780_000_000_000);
        for format in [ClockFormat::TwelveHour, ClockFormat::TwentyFourHour] {
            let said = clock_in(at, true, format);
            assert_eq!(said.matches(':').count(), 2, "{said}");
        }
    }

    #[test]
    fn a_wait_names_the_queue_it_waits_in() {
        let short = |wait: WaitReason| wait_short(&wait, Some("Main"));
        let next = short(WaitReason::InLine { position: 1 });
        assert_eq!(next.said(), "Next in Main");
        assert_eq!(next.marked(), ["Main"]);
        assert_eq!(
            short(WaitReason::InLine { position: 3 }).said(),
            "3rd to start in Main"
        );
        let first = WaitReason::GlobalLimit {
            running: 5,
            limit: 5,
        };
        assert_eq!(short(first).said(), "Next in Main, waiting for a free slot");
        let full = WaitReason::QueueFull {
            running: 2,
            limit: 2,
        };
        assert_eq!(short(full).said(), "Next in Main, waiting for a free slot");
        assert_eq!(short(WaitReason::QueuePaused).said(), "Main is paused");
    }

    #[test]
    fn a_download_reconnecting_says_which_try() {
        let reconnecting = DownloadSummary {
            status: DownloadStatus::Downloading,
            waiting: Some(WaitReason::Reconnecting { attempt: 3, of: 6 }),
            ..wirecatch_test_support::downloads::download(1)
        };
        assert_eq!(
            status_short(&reconnecting, Some("Main")).said(),
            "Reconnecting (try 3 of 6)"
        );
        assert!(
            status_detail(&reconnecting, Some("Main"))
                .is_some_and(|said| said.said().contains("reconnecting (try 3 of 6)"))
        );
    }

    #[test]
    fn a_wait_in_a_queue_not_known_yet_says_its_queue() {
        let in_line = wait_short(&WaitReason::InLine { position: 2 }, None);
        assert_eq!(in_line.said(), "2nd to start in its queue");
        assert_eq!(in_line.marked(), Vec::<&str>::new());
        assert_eq!(
            wait_short(&WaitReason::QueuePaused, None).said(),
            "Its queue is paused"
        );
    }

    #[test]
    fn a_wait_in_full_names_its_queue_so_it_stands_out() {
        let said = wait_long(&WaitReason::InLine { position: 2 }, Some("Videos"));
        assert_eq!(said.said(), "2nd to start in Videos.");
        assert_eq!(said.marked(), ["Videos"]);
        let first = WaitReason::GlobalLimit {
            running: 5,
            limit: 5,
        };
        assert_eq!(
            wait_long(&first, Some("Main")).said(),
            "Next in Main: 5 of 5 downloads are running. This starts when one of them finishes."
        );
        assert_eq!(
            wait_long(&WaitReason::QueuePaused, Some("Main")).marked(),
            ["Main"]
        );
    }

    #[test]
    fn a_running_queue_counts_what_runs_and_what_waits() {
        assert_eq!(queue_running(3, 2, Some(240)), "3 running · 2 waiting");
        assert_eq!(queue_running(3, 0, Some(240)), "3 running · 4 min left");
        assert_eq!(queue_running(1, 0, None), "1 running");
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
    fn a_paused_row_says_where_it_got_to_only_once_it_got_anywhere() {
        let paused = |done, total| DownloadSummary {
            status: DownloadStatus::Paused,
            done,
            total,
            ..wirecatch_test_support::downloads::download(1)
        };
        assert_eq!(paused_at(&paused(0, None)), None);
        assert_eq!(paused_at(&paused(0, Some(2048))), None);
        assert_eq!(paused_at(&paused(1024, None)).as_deref(), Some("1.00 KB"));
        assert_eq!(
            paused_at(&paused(1024, Some(2048))).as_deref(),
            Some("1.00 KB of 2.00 KB")
        );
    }

    #[test]
    fn the_file_exists_question_says_which_case_it_is() {
        let today = NaiveDate::from_ymd_opt(2026, 10, 10).unwrap();
        let saved = Local
            .with_ymd_and_hms(2026, 10, 10, 18, 38, 0)
            .unwrap()
            .timestamp_millis();
        let question = |same_file, saved_at| FileExistsQuestion {
            id: wirecatch_protocol::QuestionId(1),
            download: wirecatch_protocol::DownloadId(1),
            url: "https://example.com/report.pdf".to_owned(),
            file_name: "report.pdf".to_owned(),
            folder: "Downloads".to_owned(),
            size: 2048,
            same_file,
            saved_at,
        };
        let said = |same_file, saved_at| {
            file_exists_said_on(
                &question(same_file, saved_at),
                today,
                ClockFormat::TwentyFourHour,
            )
        };
        let folder = folder("Downloads");
        assert_eq!(
            said(true, Some(UnixMillis(saved))),
            format!(
                "A file that looks like the same one is already in {folder} (2.00 KB, saved today at 18:38)."
            )
        );
        assert_eq!(
            said(false, None),
            format!("Another file with this name is already in {folder} (2.00 KB).")
        );
    }

    #[test]
    fn an_earlier_download_says_when_it_finished_or_what_it_is_doing() {
        let today = NaiveDate::from_ymd_opt(2026, 10, 10).unwrap();
        let moment = |month, day, hour, minute| {
            UnixMillis(
                Local
                    .with_ymd_and_hms(2026, month, day, hour, minute, 0)
                    .unwrap()
                    .timestamp_millis(),
            )
        };
        let said = |status, clock| {
            let mut row = wirecatch_test_support::downloads::download(1);
            row.status = status;
            row.added_at = moment(10, 10, 18, 30);
            row.completed_at = Some(moment(10, 10, 18, 38));
            earlier_notice_on(&row, today, clock)
        };
        let finished_on = |at| {
            let mut row = wirecatch_test_support::downloads::download(1);
            row.status = DownloadStatus::Completed;
            row.completed_at = Some(at);
            earlier_notice_on(&row, today, ClockFormat::TwelveHour)
        };
        let twelve = ClockFormat::TwelveHour;

        assert_eq!(
            said(DownloadStatus::Completed, twelve),
            "Already downloaded today at 6:38 PM"
        );
        assert_eq!(
            said(DownloadStatus::Completed, ClockFormat::TwentyFourHour),
            "Already downloaded today at 18:38"
        );
        assert_eq!(
            finished_on(moment(10, 9, 7, 5)),
            "Already downloaded yesterday at 7:05 AM"
        );
        assert_eq!(
            finished_on(moment(9, 3, 9, 0)),
            "Already downloaded on 3 Sep 2026"
        );
        for status in [
            DownloadStatus::Connecting,
            DownloadStatus::Downloading,
            DownloadStatus::Processing,
        ] {
            assert_eq!(said(status, twelve), "Already downloading");
        }
        assert_eq!(
            said(DownloadStatus::Queued, twelve),
            "Already in your list, queued"
        );
        assert_eq!(
            said(DownloadStatus::Paused, twelve),
            "Already in your list, paused"
        );
        assert_eq!(
            said(DownloadStatus::Failed, twelve),
            "Already in your list, failed"
        );
        assert_eq!(
            said(DownloadStatus::Cancelled, twelve),
            "Started today at 6:30 PM, then cancelled"
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
