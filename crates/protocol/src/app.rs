//! Downloads, queues and settings as the desktop UI sees them.

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use ts_rs::TS;

use crate::extension::{ExtensionSettings, ExtensionSettingsPatch};
use crate::{DownloadId, QuestionId, QueueId};

/// One row of the download list.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct DownloadSummary {
    /// The download this row shows.
    pub id: DownloadId,
    /// Where it is downloaded from, with any password in the address removed.
    pub url: String,
    /// Name of the saved file, without the folder.
    pub file_name: String,
    /// Full path of the final file.
    pub path: String,
    /// The queue it belongs to.
    pub queue: QueueId,
    /// The kind of file, which picked its folder.
    pub category: Category,
    /// Where it is in its life.
    pub status: DownloadStatus,
    /// Size in bytes, when the server said.
    #[ts(type = "number | null")]
    pub total: Option<u64>,
    /// Bytes downloaded so far.
    #[ts(type = "number")]
    pub done: u64,
    /// Bytes per second, smoothed over the last few seconds.
    #[ts(type = "number")]
    pub speed: u64,
    /// Connections open to the server right now.
    pub connections: u32,
    /// Seconds left at the current speed, when the size is known and data is flowing.
    #[ts(type = "number | null")]
    pub eta_secs: Option<u64>,
    /// Why it failed, when it did.
    pub error: Option<UserError>,
    /// Why a `Queued` download is not running yet; `None` for every other status.
    pub waiting: Option<WaitReason>,
    /// Set for video and audio downloaded from a stream or a media page.
    pub media: Option<MediaSummary>,
    /// When it was added, in Unix time in milliseconds.
    #[ts(type = "number")]
    pub added_at: i64,
    /// When it completed, in Unix time in milliseconds; `None` until then.
    #[ts(type = "number | null")]
    pub completed_at: Option<i64>,
    /// What ffmpeg is doing while the status is `Processing`, and how far it is (D24);
    /// `None` at any other time.
    #[serde(default)]
    pub processing: Option<ProcessingProgress>,
}

/// What ffmpeg is doing to a downloaded video or audio, and how far it is (D24).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ProcessingProgress {
    /// What ffmpeg is doing.
    pub step: ProcessingStep,
    /// How much of the step is done, in tenths of a percent (0 to [`PER_MILLE_DONE`]), when
    /// known.
    pub per_mille: Option<u16>,
}

/// One step after a media download's bytes have arrived.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum ProcessingStep {
    /// Separate video and audio are copied into one file.
    Joining,
    /// The file is copied into its final format with its subtitles, chapters or tags; nothing
    /// is re-encoded, so it takes seconds.
    Finishing,
    /// The video or audio is re-encoded for the chosen format.
    Converting,
}

/// Where a download is in its life.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum DownloadStatus {
    /// Waiting for its queue to have room or for the queue's schedule.
    Queued,
    /// Asking the server for the file; no data has arrived yet.
    Connecting,
    /// Data is arriving.
    Downloading,
    /// Stopped by the person (this download or "pause all"), or added without starting; the
    /// downloaded part is kept and resuming continues from it. Pausing a queue does not
    /// pause its downloads one by one: they go back to `Queued` with
    /// [`WaitReason::QueuePaused`].
    Paused,
    /// The file is in its folder, finished.
    Completed,
    /// Stopped by an error, described in [`DownloadSummary::error`].
    Failed,
    /// Stopped by the user; the partial file is gone. Retry starts over.
    Cancelled,
    /// Downloaded; ffmpeg is joining the video and audio into one file, or converting it
    /// (see [`DownloadSummary::processing`]).
    Processing,
}

/// An error worded for a person, with the fix offered as a button (D3, rule 4).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct UserError {
    /// What happened and what to do about it, in plain language.
    pub message: String,
    /// The fix offered as a button, when there is one.
    pub fix: Option<FixAction>,
}

/// A fix the UI can offer for a [`UserError`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum FixAction {
    /// Try the download again.
    Retry,
    /// The link expired: get a fresh one from the page, keep the downloaded bytes.
    RefreshLink,
    /// The folder cannot be used or written to: pick another one.
    ChooseFolder,
    /// The disk is full: make room, then try again.
    FreeDiskSpace,
    /// Converting a downloaded video failed: save it as it was downloaded instead (D24).
    KeepOriginal,
}

/// The kind of file a download is; each has its own folder ([`CategoryRule`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum Category {
    /// Video files.
    Video,
    /// Sound files.
    Audio,
    /// Compressed files and disc images.
    Archive,
    /// Text, office documents and e-books.
    Document,
    /// Installers and programs.
    Program,
    /// Pictures.
    Image,
    /// Everything no rule matches; saved in the default folder.
    Other,
}

/// A line of downloads with its own limits and schedule.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Queue {
    /// Identifies the queue.
    pub id: QueueId,
    /// The name the person gave it, unique among the queues.
    pub name: String,
    /// Downloads from this queue that may run at the same time.
    pub max_concurrent: u32,
    /// Bytes per second shared by the whole queue; `None` is unlimited.
    #[ts(type = "number | null")]
    pub speed_limit: Option<u64>,
    /// When set, the queue only runs inside this window.
    pub schedule: Option<Schedule>,
    /// Downloads go to this queue when none is named. There is always exactly one, and it
    /// cannot be deleted.
    pub is_default: bool,
    /// A paused queue starts nothing new, including downloads added later, and
    /// its running downloads are paused. Resuming it does not resume downloads
    /// the user paused one by one.
    pub paused: bool,
    /// Set by "start now": the queue runs outside its schedule until it is empty
    /// or paused, then the schedule applies again.
    pub running_outside_schedule: bool,
}

/// Media details shown on a download row.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct MediaSummary {
    /// For example "1080p" or "Audio only".
    pub quality: String,
    /// How the media was delivered.
    pub source: crate::extension::MediaKind,
    /// The site and id of a video read from its page, for example `Youtube:dQw4w9WgXcQ`, so
    /// another link to the same video is recognised. `None` for streams and media files.
    #[serde(default)]
    pub video_key: Option<String>,
}

/// The app found several qualities and asks the user to pick one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct MediaChoice {
    /// Identifies this choice; not a download id yet.
    pub id: QuestionId,
    /// The video's title, which names the file.
    pub title: String,
    /// The page the media was found on, with any password in the address removed.
    pub page_url: Option<String>,
    /// Best first. The UI preselects the first one.
    pub options: Vec<MediaOption>,
    /// Where it will be saved if the user just confirms.
    pub folder: String,
    /// The queue it goes to if the user just confirms.
    pub queue: QueueId,
    /// The subtitles the video has, human-written ones first, then auto-generated ones (D24).
    /// Empty for streams and for videos without subtitles.
    #[serde(default)]
    pub subtitles: Vec<MediaSubtitle>,
    /// The video has chapters, which can be added to the file (D24).
    #[serde(default)]
    pub chapters: bool,
    /// How long the video is, in milliseconds, when known.
    #[serde(default)]
    #[ts(type = "number | null")]
    pub duration_ms: Option<u64>,
}

/// One quality the media is offered in.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct MediaOption {
    /// Identifies it in [`MediaSelection::option_id`].
    pub id: String,
    /// Plain words, for example "1080p, MP4".
    pub label: String,
    /// The video's height in pixels; `None` for audio only or when the source does not say.
    pub height: Option<u32>,
    /// The option has sound and no picture.
    pub audio_only: bool,
    /// Estimated size when the source says.
    #[ts(type = "number | null")]
    pub size: Option<u64>,
    /// True when ffmpeg must join separate video and audio for this option.
    pub needs_ffmpeg: bool,
    /// The quality alone, without the format, for example "1080p, 60 fps" or "Audio only".
    #[serde(default)]
    pub quality: String,
    /// What each "Save as" choice means for this quality (D24). The first is always
    /// [`OutputFormat::Original`]; a stream offers only that one.
    #[serde(default)]
    pub outputs: Vec<OutputFit>,
}

/// What a media download is saved as (D24). `Original` keeps the format the site sends;
/// the others are for videos (`Mp4`, `Mkv`, `Webm`) or for audio only (the rest).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum OutputFormat {
    /// The file as the site sends it; nothing is converted.
    #[default]
    Original,
    /// MP4 video, which plays almost everywhere.
    Mp4,
    /// Matroska video, which holds any codec and subtitle tracks.
    Mkv,
    /// WebM video, the open format browsers play.
    Webm,
    /// MP3 audio.
    Mp3,
    /// AAC audio in an M4A file.
    M4a,
    /// Opus audio.
    Opus,
    /// FLAC: lossless, compressed audio.
    Flac,
    /// WAV: lossless, uncompressed audio.
    Wav,
}

impl OutputFormat {
    /// Whether a video can be saved as this format: the original and the video formats. The
    /// app refuses any other as the starting format for videos.
    #[must_use]
    pub const fn saves_video(self) -> bool {
        match self {
            OutputFormat::Original | OutputFormat::Mp4 | OutputFormat::Mkv | OutputFormat::Webm => {
                true
            }
            OutputFormat::Mp3
            | OutputFormat::M4a
            | OutputFormat::Opus
            | OutputFormat::Flac
            | OutputFormat::Wav => false,
        }
    }

    /// Whether audio alone can be saved as this format: the original and the audio formats.
    /// The app refuses any other as the starting format for audio downloads.
    #[must_use]
    pub const fn saves_audio(self) -> bool {
        matches!(self, OutputFormat::Original) || !self.saves_video()
    }

    /// Whether this format keeps audio without loss (FLAC and WAV), so a lossy source gains
    /// nothing from it.
    #[must_use]
    pub const fn is_lossless(self) -> bool {
        match self {
            OutputFormat::Flac | OutputFormat::Wav => true,
            OutputFormat::Original
            | OutputFormat::Mp4
            | OutputFormat::Mkv
            | OutputFormat::Webm
            | OutputFormat::Mp3
            | OutputFormat::M4a
            | OutputFormat::Opus => false,
        }
    }

    /// The audio bit rates, in kbps, a person can pick when saving as this format; for such a
    /// format the app refuses any other [`MediaSelection::audio_kbps`]. Empty when there is no
    /// choice, and then the app ignores `audio_kbps`: the original and the video formats keep
    /// the audio or re-encode it at the default bit rate of its codec, and FLAC and WAV are
    /// lossless.
    #[must_use]
    pub const fn kbps_choices(self) -> &'static [u32] {
        match self {
            OutputFormat::Mp3 => &[128, 192, 256, 320],
            OutputFormat::M4a => &[128, 192, 256],
            OutputFormat::Opus => &[96, 128, 160, 192],
            OutputFormat::Original
            | OutputFormat::Mp4
            | OutputFormat::Mkv
            | OutputFormat::Webm
            | OutputFormat::Flac
            | OutputFormat::Wav => &[],
        }
    }

    /// The audio bit rate, in kbps, used when the audio must be re-encoded for this format
    /// and none was picked; `None` when the format has no [`kbps_choices`](Self::kbps_choices).
    #[must_use]
    pub const fn default_kbps(self) -> Option<u32> {
        match self {
            OutputFormat::Mp3 | OutputFormat::M4a => Some(192),
            OutputFormat::Opus => Some(128),
            OutputFormat::Original
            | OutputFormat::Mp4
            | OutputFormat::Mkv
            | OutputFormat::Webm
            | OutputFormat::Flac
            | OutputFormat::Wav => None,
        }
    }
}

/// What happens to the video or to the audio of a quality saved as one [`OutputFormat`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum StreamAction {
    /// There is none: an audio-only quality has no video.
    None,
    /// Copied into the file as it is, which takes seconds.
    Copy,
    /// Re-encoded, which takes longer and keeps the processor or graphics chip busy.
    Encode,
}

/// How one quality is saved as one [`OutputFormat`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct OutputFit {
    /// The format this describes.
    pub format: OutputFormat,
    /// The file's extension, without the dot, for example `mp4`.
    pub ext: String,
    /// What happens to the video.
    pub video: StreamAction,
    /// What happens to the audio when no audio quality is picked; re-encoding then uses the
    /// format's default quality, which is quick. Picking an audio quality for an audio format
    /// always re-encodes it.
    pub audio: StreamAction,
    /// The video codec saved, in plain words ("H.264", "VP9", "AV1"), when there is video.
    pub video_codec: Option<String>,
    /// The downloaded audio's codec, in plain words ("AAC", "Opus").
    pub audio_codec: Option<String>,
    /// The downloaded audio's bit rate in kbps, when the site says.
    pub audio_kbps: Option<u32>,
    /// The downloaded audio is lossless (FLAC, ALAC, PCM), so a lossless format keeps all of it.
    pub lossless_source: bool,
    /// Estimated size of the saved file, when known.
    #[ts(type = "number | null")]
    pub size: Option<u64>,
    /// Subtitles go inside the file as tracks; otherwise they are saved as `.srt` files next
    /// to it.
    pub embeds_subtitles: bool,
    /// The format can hold chapters.
    pub chapters: bool,
}

/// One subtitle language a video has (D24).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct MediaSubtitle {
    /// Identifies it in [`MediaSelection::subtitles`].
    pub id: String,
    /// The language code the site gives, for example `en` or `pt-BR`.
    pub language: String,
    /// The language in words, for example "English".
    pub name: String,
    /// Made by speech recognition rather than written by a person.
    pub auto: bool,
}

/// The user's pick for a [`MediaChoice`]; absent fields keep the proposal.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct MediaSelection {
    /// The [`MediaOption::id`] picked.
    pub option_id: String,
    /// Absolute folder to save in, instead of [`MediaChoice::folder`].
    pub folder: Option<String>,
    /// File name to save as, instead of the one made from the title.
    pub file_name: Option<String>,
    /// Queue to add it to, instead of [`MediaChoice::queue`].
    pub queue: Option<QueueId>,
    /// Download later: add it to its queue paused instead of starting it (D23).
    #[serde(default)]
    pub later: bool,
    /// What to save it as; one of the chosen option's [`MediaOption::outputs`] (D24).
    #[serde(default)]
    pub output: OutputFormat,
    /// Audio quality in kbps for a lossy audio format (MP3, M4A, Opus); `None` keeps the
    /// downloaded audio when it fits, or uses the format's usual quality.
    #[serde(default)]
    pub audio_kbps: Option<u32>,
    /// Ids of the [`MediaChoice::subtitles`] to save.
    #[serde(default)]
    pub subtitles: Vec<String>,
    /// Save the subtitles as `.srt` files next to a video instead of inside it. Audio formats
    /// always save them as files.
    #[serde(default)]
    pub subtitle_files: bool,
    /// Add the video's chapters to the file, when the format can hold them.
    #[serde(default)]
    pub chapters: bool,
}

/// What the quality window starts from (D24): the last choices, kept separately for videos
/// and for audio only. Also in Settings, Media, and in the settings profile.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct MediaDefaults {
    /// Where the quality window starts for a video.
    #[serde(default)]
    pub video: OutputDefaults,
    /// Where the quality window starts for audio only.
    #[serde(default)]
    pub audio: OutputDefaults,
}

/// The starting choices for one kind of media download.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct OutputDefaults {
    /// The format picked first.
    #[serde(default)]
    pub format: OutputFormat,
    /// Audio quality in kbps for a lossy audio format; `None` is the format's usual quality.
    #[serde(default)]
    pub audio_kbps: Option<u32>,
    /// Subtitle languages to tick, for example `["en", "fr"]`.
    #[serde(default)]
    pub subtitle_languages: Vec<String>,
    /// Tick a language's auto-generated subtitles when it has no human-written ones.
    #[serde(default)]
    pub auto_subtitles: bool,
    /// Save subtitles as `.srt` files next to a video instead of inside it.
    #[serde(default)]
    pub subtitle_files: bool,
    /// Add chapters to the file.
    #[serde(default)]
    pub chapters: bool,
}

/// Identifies an optional component (D2). Since D20 there is one: the media add-on, which
/// installs yt-dlp, ffmpeg and Deno together.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum AddonId {
    /// The media add-on (D20).
    Media,
    /// An add-on this build does not know, named by a newer Wirecatch (in a settings profile,
    /// say), read so the rest of the message still works. The app never offers or lists one.
    Unknown,
}

impl AddonId {
    /// Every add-on this build knows, which reading an id looks for by name.
    pub const KNOWN: &[AddonId] = &[AddonId::Media];

    /// The name the add-on is written as.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            AddonId::Media => "media",
            AddonId::Unknown => "unknown",
        }
    }
}

/// Reads any name, so a profile or message naming an add-on from a newer Wirecatch still reads;
/// `#[serde(other)]` would do the same, but the TypeScript generator cannot read it.
impl<'de> Deserialize<'de> for AddonId {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let name = std::borrow::Cow::<str>::deserialize(deserializer)?;
        Ok(AddonId::KNOWN
            .iter()
            .copied()
            .find(|id| id.name() == name)
            .unwrap_or(AddonId::Unknown))
    }
}

/// An optional component (D2), as the add-ons settings show it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct AddonInfo {
    /// Which add-on it is.
    pub id: AddonId,
    /// The name shown, for example "Media add-on".
    pub name: String,
    /// One plain-language line on what it adds.
    pub description: String,
    /// Whether it is installed, being installed or out of date.
    pub state: AddonState,
    /// The version on this machine, when installed.
    pub installed_version: Option<String>,
    /// The newest version Wirecatch may install, when known.
    pub latest_version: Option<String>,
    /// Download size of the latest version, when known.
    #[ts(type = "number | null")]
    pub size: Option<u64>,
    /// A plain sentence about updates to show with the add-on, for example
    /// that a newer version waits until it is two days old (D21.1).
    #[serde(default)]
    pub note: Option<String>,
}

/// Where an add-on is.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "type", content = "data", rename_all = "snake_case")]
#[ts(export)]
pub enum AddonState {
    /// Not on this machine; what needs it is unavailable until it is installed.
    NotInstalled,
    /// Being downloaded and installed.
    Installing {
        /// Bytes downloaded so far.
        #[ts(type = "number")]
        done: u64,
        /// Size of the download in bytes, when known.
        #[ts(type = "number | null")]
        total: Option<u64>,
    },
    /// Installed and up to date.
    Installed,
    /// Installed, and a newer version can be installed.
    UpdateAvailable,
    /// The last install or update failed.
    Failed {
        /// What went wrong, in plain language.
        message: String,
    },
}

/// Why a queued download is waiting, shown in plain words so queues are never a
/// mystery (D3).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "type", content = "data", rename_all = "snake_case")]
#[ts(export)]
pub enum WaitReason {
    /// Its queue is paused.
    QueuePaused,
    /// Outside its queue's schedule.
    QueueSchedule {
        /// When the queue next starts, if it does.
        starts_at: Option<TimeOfDay>,
    },
    /// Its queue already runs as many downloads as it may.
    QueueFull {
        /// Downloads of the queue running now.
        running: u32,
        /// The queue's [`Queue::max_concurrent`].
        limit: u32,
    },
    /// The app already runs as many downloads as allowed overall.
    GlobalLimit {
        /// Downloads running now, across all queues.
        running: u32,
        /// [`AppSettings::max_concurrent_downloads`].
        limit: u32,
    },
    /// Ahead of it in the same queue are other waiting downloads.
    InLine {
        /// Its place among the queue's waiting downloads, counting from 1.
        position: u32,
    },
    /// A video's server refused it for now; Wirecatch tries again by itself at
    /// `retry_at` (Unix time in milliseconds), keeping what was downloaded. Only
    /// on a download that is connecting, never on a queued one.
    ServerRefused {
        /// When the next try starts, in Unix time in milliseconds.
        #[ts(type = "number")]
        retry_at: i64,
    },
}

/// When a queue runs, in the computer's local time.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Schedule {
    /// When the queue starts each day it runs.
    pub start: TimeOfDay,
    /// `None` runs until the queue is empty.
    pub stop: Option<TimeOfDay>,
    /// Empty means every day.
    pub days: Vec<Weekday>,
}

/// A time on the clock, in the computer's local time.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct TimeOfDay {
    /// 0 to 23.
    pub hour: u8,
    /// 0 to 59.
    pub minute: u8,
}

/// A day of the week.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum Weekday {
    /// Monday.
    Mon,
    /// Tuesday.
    Tue,
    /// Wednesday.
    Wed,
    /// Thursday.
    Thu,
    /// Friday.
    Fri,
    /// Saturday.
    Sat,
    /// Sunday.
    Sun,
}

/// A path that survives moving between machines and operating systems (D6).
/// It may start with one of `{Downloads}`, `{Home}`, `{Documents}`, `{Videos}`,
/// `{Music}` or `{Pictures}`, resolved on the machine that uses it. Separators are `/`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct PortablePath(pub String);

/// Which files are a [`Category`], and where they are saved.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct CategoryRule {
    /// The category the rule is for.
    pub category: Category,
    /// File extensions of the category: lower case, without the dot.
    pub extensions: Vec<String>,
    /// Where files of the category are saved.
    pub folder: PortablePath,
}

/// How the app looks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum Theme {
    /// Follow the operating system's light or dark setting.
    System,
    /// Always light.
    Light,
    /// Always dark.
    Dark,
}

/// A change to some of [`AppSettings`]: only the fields that are present change, and a group (`extension`, `media`) changes only in the fields its own patch holds. A window sends only what the person changed, applied to the app's current settings, so two windows changing different settings never undo each other.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
#[ts(export, optional_fields)]
pub struct AppSettingsPatch {
    /// The new [`AppSettings::default_folder`].
    pub default_folder: Option<PortablePath>,
    /// The new [`AppSettings::categories`], replacing every rule.
    pub categories: Option<Vec<CategoryRule>>,
    /// The new [`AppSettings::connections_per_download`].
    pub connections_per_download: Option<u8>,
    /// The new [`AppSettings::max_concurrent_downloads`].
    pub max_concurrent_downloads: Option<u32>,
    /// The new limit in bytes per second; `null` removes it.
    #[serde(default, skip_serializing_if = "Update::is_keep")]
    // f64: TypeScript sees a `number`, like every other u64 in the protocol.
    #[ts(optional, as = "Option<Option<f64>>")]
    pub global_speed_limit: Update<Option<u64>>,
    /// Changes to [`AppSettings::extension`].
    pub extension: Option<ExtensionSettingsPatch>,
    /// The new [`AppSettings::theme`].
    pub theme: Option<Theme>,
    /// The new [`AppSettings::start_with_system`].
    pub start_with_system: Option<bool>,
    /// The new [`AppSettings::close_to_tray`].
    pub close_to_tray: Option<bool>,
    /// The new [`AppSettings::notify_on_complete`].
    pub notify_on_complete: Option<bool>,
    /// The new [`AppSettings::auto_update_addons`].
    pub auto_update_addons: Option<bool>,
    /// The new [`AppSettings::show_progress_windows`].
    pub show_progress_windows: Option<bool>,
    /// Changes to [`AppSettings::media`].
    pub media: Option<MediaDefaultsPatch>,
    /// The new [`AppSettings::local_sites`], replacing the list.
    pub local_sites: Option<Vec<String>>,
}

/// A change to some of [`MediaDefaults`].
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
#[ts(export, optional_fields)]
pub struct MediaDefaultsPatch {
    /// Changes to [`MediaDefaults::video`].
    pub video: Option<OutputDefaultsPatch>,
    /// Changes to [`MediaDefaults::audio`].
    pub audio: Option<OutputDefaultsPatch>,
}

/// A change to some of [`OutputDefaults`].
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
#[ts(export, optional_fields)]
pub struct OutputDefaultsPatch {
    /// The new [`OutputDefaults::format`].
    pub format: Option<OutputFormat>,
    /// The new quality in kbps; `null` goes back to the format's usual quality.
    #[serde(default, skip_serializing_if = "Update::is_keep")]
    #[ts(optional, as = "Option<Option<u32>>")]
    pub audio_kbps: Update<Option<u32>>,
    /// The new [`OutputDefaults::subtitle_languages`], replacing the list.
    pub subtitle_languages: Option<Vec<String>>,
    /// The new [`OutputDefaults::auto_subtitles`].
    pub auto_subtitles: Option<bool>,
    /// The new [`OutputDefaults::subtitle_files`].
    pub subtitle_files: Option<bool>,
    /// The new [`OutputDefaults::chapters`].
    pub chapters: Option<bool>,
}

/// A patch field for a setting that can be empty. `Option` alone cannot say both "leave it" and "empty it", since JSON `null` reads as `None`: here a missing field is [`Update::Keep`] (through `#[serde(default)]`) and any value, `null` included, is [`Update::Set`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Update<T> {
    /// Leave the setting as it is.
    #[default]
    Keep,
    /// Give the setting this value.
    Set(T),
}

impl<T> Update<T> {
    /// Whether the setting is left as it is.
    pub fn is_keep(&self) -> bool {
        matches!(self, Self::Keep)
    }
}

impl<T: Serialize> Serialize for Update<T> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            // A patch skips the field instead (`skip_serializing_if`): `null` would read back as a value.
            Self::Keep => serializer.serialize_unit(),
            Self::Set(value) => value.serialize(serializer),
        }
    }
}

impl<'de, T: Deserialize<'de>> Deserialize<'de> for Update<T> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        T::deserialize(deserializer).map(Self::Set)
    }
}

/// Replaces `slot` when the patch has a value for it.
fn set<T>(slot: &mut T, value: Option<T>) {
    if let Some(value) = value {
        *slot = value;
    }
}

/// Replaces `slot` when the patch sets it.
fn update<T>(slot: &mut T, value: Update<T>) {
    if let Update::Set(value) = value {
        *slot = value;
    }
}

crate::exported::exported_consts! {
    /// Highest accepted [`AppSettings::connections_per_download`].
    pub const MAX_CONNECTIONS_PER_DOWNLOAD: u8 = 32;
    /// [`AppSettings::connections_per_download`] on a fresh install.
    pub const DEFAULT_CONNECTIONS_PER_DOWNLOAD: u8 = 8;
    /// Highest accepted value for any limit on downloads running at once: the app-wide
    /// [`AppSettings::max_concurrent_downloads`] and each [`Queue::max_concurrent`].
    pub const MAX_CONCURRENT_DOWNLOADS: u32 = 100;
    /// [`AppSettings::max_concurrent_downloads`] on a fresh install.
    pub const DEFAULT_MAX_CONCURRENT_DOWNLOADS: u32 = 5;
    /// [`Queue::max_concurrent`] of a newly created queue.
    pub const NEW_QUEUE_MAX_CONCURRENT: u32 = 2;
    /// [`Queue::max_concurrent`] of the default queue on a fresh install.
    pub const DEFAULT_QUEUE_MAX_CONCURRENT: u32 = 3;
    /// [`ProcessingProgress::per_mille`] of a finished step: progress is counted in tenths of
    /// a percent.
    pub const PER_MILLE_DONE: u16 = 1000;
    /// [`SettingsProfile::format_version`] of the settings files this build writes, and the
    /// newest it reads.
    pub const PROFILE_FORMAT_VERSION: u32 = 1;
    /// Most download windows asking a question at once (D23); later questions wait until one
    /// of them closes.
    pub const MAX_QUESTION_WINDOWS: usize = 5;
    /// How far each download window sits down and right of the one before it, in logical
    /// pixels.
    pub const DOWNLOAD_WINDOW_CASCADE_STEP: u32 = 28;
}

/// Every user setting. The app is the only store (D5).
#[expect(
    clippy::struct_excessive_bools,
    reason = "each bool is an independent setting the person switches on its own, persisted as is; they are not states of one machine, and grouping them would change the settings file for no gain"
)]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct AppSettings {
    /// Where downloads no [`CategoryRule`] matches are saved.
    pub default_folder: PortablePath,
    /// Which files go to which folder, one rule per category.
    pub categories: Vec<CategoryRule>,
    /// Most connections per download, 1 to [`MAX_CONNECTIONS_PER_DOWNLOAD`].
    pub connections_per_download: u8,
    /// Downloads running at once across all queues, 1 to [`MAX_CONCURRENT_DOWNLOADS`].
    pub max_concurrent_downloads: u32,
    /// Bytes per second for everything together; `None` is unlimited.
    #[ts(type = "number | null")]
    pub global_speed_limit: Option<u64>,
    /// What the browser extension does.
    pub extension: ExtensionSettings,
    /// How the app looks.
    pub theme: Theme,
    /// Start Wirecatch when the person signs in to the computer.
    pub start_with_system: bool,
    /// Closing the window keeps downloads running in the tray.
    pub close_to_tray: bool,
    /// Show a system notification when a download completes.
    pub notify_on_complete: bool,
    /// Keep installed add-ons up to date without asking.
    #[serde(default = "default_true")]
    pub auto_update_addons: bool,
    /// After Download now, a browser download's window stays open and shows its progress
    /// (D23). Off: the window closes when the download starts.
    #[serde(default = "default_true")]
    pub show_progress_windows: bool,
    /// What the quality window for a video starts from (D24).
    #[serde(default)]
    pub media: MediaDefaults,
    /// Sites on the person's network: host names, each also covering the names
    /// under it (`corp.example` covers `wiki.corp.example`). A link captured from
    /// a page on one of them may lead to devices on the local network, like a
    /// page whose own address is local (D39). Empty by default.
    #[serde(default)]
    pub local_sites: Vec<String>,
}

fn default_true() -> bool {
    true
}

impl AppSettings {
    /// Applies a patch. Every field is named on both sides, so a setting added to one and not the other does not compile.
    pub fn apply(&mut self, patch: AppSettingsPatch) {
        let Self {
            default_folder,
            categories,
            connections_per_download,
            max_concurrent_downloads,
            global_speed_limit,
            extension,
            theme,
            start_with_system,
            close_to_tray,
            notify_on_complete,
            auto_update_addons,
            show_progress_windows,
            media,
            local_sites,
        } = self;
        let AppSettingsPatch {
            default_folder: new_default_folder,
            categories: new_categories,
            connections_per_download: new_connections_per_download,
            max_concurrent_downloads: new_max_concurrent_downloads,
            global_speed_limit: new_global_speed_limit,
            extension: extension_patch,
            theme: new_theme,
            start_with_system: new_start_with_system,
            close_to_tray: new_close_to_tray,
            notify_on_complete: new_notify_on_complete,
            auto_update_addons: new_auto_update_addons,
            show_progress_windows: new_show_progress_windows,
            media: media_patch,
            local_sites: new_local_sites,
        } = patch;
        set(default_folder, new_default_folder);
        set(categories, new_categories);
        set(connections_per_download, new_connections_per_download);
        set(max_concurrent_downloads, new_max_concurrent_downloads);
        update(global_speed_limit, new_global_speed_limit);
        if let Some(extension_patch) = extension_patch {
            extension.apply(extension_patch);
        }
        set(theme, new_theme);
        set(start_with_system, new_start_with_system);
        set(close_to_tray, new_close_to_tray);
        set(notify_on_complete, new_notify_on_complete);
        set(auto_update_addons, new_auto_update_addons);
        set(show_progress_windows, new_show_progress_windows);
        if let Some(media_patch) = media_patch {
            media.apply(media_patch);
        }
        set(local_sites, new_local_sites);
    }
}

impl MediaDefaults {
    /// Applies a patch; see [`AppSettings::apply`].
    pub fn apply(&mut self, patch: MediaDefaultsPatch) {
        let Self { video, audio } = self;
        let MediaDefaultsPatch {
            video: video_patch,
            audio: audio_patch,
        } = patch;
        if let Some(video_patch) = video_patch {
            video.apply(video_patch);
        }
        if let Some(audio_patch) = audio_patch {
            audio.apply(audio_patch);
        }
    }
}

impl OutputDefaults {
    /// Applies a patch; see [`AppSettings::apply`].
    pub fn apply(&mut self, patch: OutputDefaultsPatch) {
        let Self {
            format,
            audio_kbps,
            subtitle_languages,
            auto_subtitles,
            subtitle_files,
            chapters,
        } = self;
        let OutputDefaultsPatch {
            format: new_format,
            audio_kbps: new_audio_kbps,
            subtitle_languages: new_subtitle_languages,
            auto_subtitles: new_auto_subtitles,
            subtitle_files: new_subtitle_files,
            chapters: new_chapters,
        } = patch;
        set(format, new_format);
        update(audio_kbps, new_audio_kbps);
        set(subtitle_languages, new_subtitle_languages);
        set(auto_subtitles, new_auto_subtitles);
        set(subtitle_files, new_subtitle_files);
        set(chapters, new_chapters);
    }
}

/// The file written by settings export and read by import (D6).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct SettingsProfile {
    /// Version of this file's layout; an app refuses a profile newer than it understands.
    pub format_version: u32,
    /// Version of the app that wrote it.
    pub app_version: String,
    /// When it was written, in Unix time in milliseconds.
    #[ts(type = "number")]
    pub exported_at: i64,
    /// Every setting.
    pub settings: AppSettings,
    /// Every queue, with its limits and schedule.
    pub queues: Vec<Queue>,
    /// The installed add-ons, offered for installing on import.
    pub addons: Vec<AddonId>,
}

/// A download added from the app itself (paste, Add dialog, command palette).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct AddRequest {
    /// The address to download.
    pub url: String,
    /// Default queue if absent.
    pub queue: Option<QueueId>,
    /// Absolute folder; the category rule decides if absent.
    pub folder: Option<String>,
    /// The server's name is used if absent.
    pub file_name: Option<String>,
    /// `false` adds it paused.
    pub start: bool,
}

/// A captured download waiting for the user because `ask_before_download` is on.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct PendingCapture {
    /// Identifies this confirmation; not a download id yet.
    pub id: QuestionId,
    /// The address to download, with any password removed.
    pub url: String,
    /// The name it will be saved as if the user just confirms.
    pub file_name: String,
    /// Where it will be saved if the user just confirms.
    pub folder: String,
    /// The kind of file, which picked the folder.
    pub category: Category,
    /// Size in bytes, when known.
    #[ts(type = "number | null")]
    pub size: Option<u64>,
    /// The queue it goes to if the user just confirms.
    pub queue: QueueId,
    /// The page it came from, shown so the user knows what they are confirming.
    pub page_url: Option<String>,
}

/// The user's answer to a [`PendingCapture`]; absent fields keep the proposal.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct CaptureConfirmation {
    /// Absolute folder to save in, instead of [`PendingCapture::folder`].
    pub folder: Option<String>,
    /// File name to save as, instead of [`PendingCapture::file_name`].
    pub file_name: Option<String>,
    /// Queue to add it to, instead of [`PendingCapture::queue`].
    pub queue: Option<QueueId>,
    /// Download later: add it to its queue paused instead of starting it (D23).
    #[serde(default)]
    pub later: bool,
}

/// Everything the core pushes to the desktop UI. The desktop app forwards these
/// to the webview as one event stream.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(tag = "type", content = "data", rename_all = "snake_case")]
#[ts(export)]
pub enum UiEvent {
    /// Changed rows only, batched (at most about ten batches per second).
    DownloadsChanged(Vec<DownloadSummary>),
    /// Downloads that are gone from the list.
    DownloadsRemoved(Vec<DownloadId>),
    /// Every queue, in sidebar order, after any queue changed.
    QueuesChanged(Vec<Queue>),
    /// Every setting, after any changed.
    SettingsChanged(AppSettings),
    /// A browser capture started; the UI shows the toast with Change and Open folder (D4).
    Captured(DownloadSummary),
    /// A capture needs the user's confirmation (`ask_before_download`).
    ConfirmationRequested(PendingCapture),
    /// Saving to the download list failed even after retrying, so recent changes
    /// may be lost when the app closes. The UI shows the message.
    StorageFailed(UserError),
    /// A settings profile was imported; the UI shows what changed.
    SettingsImported(ImportSummary),
    /// Media with several qualities: the UI shows the picker.
    MediaChoiceRequested(MediaChoice),
    /// Wirecatch started reading a captured page or stream to find its qualities, which can
    /// take a few seconds. It ends with a [`MediaChoiceRequested`](Self::MediaChoiceRequested)
    /// with the same id, or a [`ConfirmationClosed`](Self::ConfirmationClosed) with it
    /// (dismissed, withdrawn by the browser, or it could not be read, in which case a failed
    /// download says why).
    MediaAnalysing {
        /// The id the question will have.
        id: QuestionId,
        /// The page or stream being read, with any password removed.
        url: String,
    },
    /// Every add-on, after any changed (an install's progress included).
    AddonsChanged(Vec<AddonInfo>),
    /// A capture needs an add-on that is not installed; the UI offers to install it.
    AddonRequired {
        /// The add-on needed.
        addon: AddonInfo,
        /// The page or stream that needs it, with any password removed.
        url: String,
        /// The media that waits: a page waiting for the add-on (dismiss it with
        /// `dismiss_media_choice`), or the [`MediaChoice`] whose picked quality
        /// needs it.
        id: QuestionId,
    },
    /// A question the UI may be showing is no longer open, so its dialog should close:
    /// a [`PendingCapture`] or [`MediaChoice`] with this id was answered (perhaps in
    /// another window), withdrawn by the browser, or replaced by another question (for
    /// example a [`MediaChoice`] that follows an add-on install). Questions share one id space,
    /// so the id is never ambiguous.
    ConfirmationClosed {
        /// The id of the question that closed.
        id: QuestionId,
    },
    /// A client asked to see Wirecatch (the extension's "Open Wirecatch"): the desktop shows its main window.
    WindowRequested,
}

/// What importing a settings profile changed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ImportSummary {
    /// Every folder setting in the profile and the folder it became on this machine.
    pub folders: Vec<ImportedFolder>,
    /// Queues the profile added.
    pub queues_added: u32,
    /// Existing queues that took the profile's name, limits and schedule: the default queue,
    /// and every queue whose name matches one in the profile, ignoring case.
    pub queues_updated: u32,
    /// Add-ons the profile lists that are not installed here. Nothing is
    /// downloaded on import; the UI may offer to install them.
    #[serde(default)]
    pub addons_to_offer: Vec<AddonId>,
}

/// One folder setting from an imported profile.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ImportedFolder {
    /// The setting in plain words, for example "Default folder" or "Video folder".
    pub setting: String,
    /// The folder as written in the profile.
    pub requested: PortablePath,
    /// The folder now used. Differs from `requested` when that does not exist on
    /// this machine (an absolute path from another system).
    pub used: PortablePath,
    /// Where `used` is on this machine.
    pub resolved: String,
    /// Why `requested` was not used, in plain words, when it was not: for
    /// example it is not on this computer, or it is a system or startup folder
    /// where a downloaded file could run by itself.
    #[serde(default)]
    pub reason: Option<String>,
    /// `resolved` is outside the home folder and the usual folders (Downloads,
    /// Documents, Videos, Music, Pictures), so the person should look at it
    /// before importing.
    #[serde(default)]
    pub outside_user_folders: bool,
}

/// What importing a settings profile would change, shown before anything is
/// applied, so the person can decide.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ImportPreview {
    /// Every folder setting in the profile and the folder it would become on
    /// this machine.
    pub folders: Vec<ImportedFolder>,
    /// Names of the queues the import would add.
    pub queues_added: Vec<String>,
    /// Names of the queues here the import would change (the default queue is
    /// always matched with the profile's).
    pub queues_updated: Vec<String>,
    /// Add-ons the profile lists that are not installed here, to offer
    /// after the import. Nothing is downloaded on import.
    pub addons_to_offer: Vec<AddonId>,
}

/// A captured stream or page still being read for its qualities.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct MediaReading {
    /// The id its quality choice will have; dismissing the question with it stops the reading.
    pub id: QuestionId,
    /// The stream or page, without any password in it.
    pub url: String,
}

/// A video page waiting for an add-on that is not installed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct AddonRequestItem {
    /// The question; dismissing it drops the page, and the quality choice that follows the
    /// install keeps it.
    pub id: QuestionId,
    /// The add-on it needs.
    pub addon: AddonInfo,
    /// The page, without any password in it.
    pub url: String,
}

/// One open question, as a download window shows it (D23): a browser file to confirm, media
/// being read, a quality to pick, or a video page waiting for the media add-on.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "kind", content = "data", rename_all = "snake_case")]
#[ts(export)]
pub enum Question {
    /// A browser file waiting for Download now, Download later or Cancel.
    Capture(PendingCapture),
    /// A stream or video page being read for its qualities.
    Reading(MediaReading),
    /// A video waiting for the person to pick a quality.
    Choice(MediaChoice),
    /// A video page waiting for the media add-on.
    Addon(AddonRequestItem),
}

impl Question {
    /// The question's id, which stays the same from one step to the next.
    #[must_use]
    pub fn id(&self) -> QuestionId {
        match self {
            Question::Capture(capture) => capture.id,
            Question::Reading(reading) => reading.id,
            Question::Choice(choice) => choice.id,
            Question::Addon(page) => page.id,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A profile from a newer Wirecatch may name an add-on this build does not know; the rest
    /// of it still reads.
    #[test]
    fn an_unknown_add_on_id_still_reads() {
        let ids: Vec<AddonId> = serde_json::from_str(r#"["media", "from_the_future"]"#).unwrap();
        assert_eq!(ids, [AddonId::Media, AddonId::Unknown]);
        assert_eq!(
            serde_json::to_string(&AddonId::Media).unwrap(),
            r#""media""#
        );
    }

    /// The desktop interface reads this shape (`Question` in `@wirecatch/protocol`).
    #[test]
    fn a_question_is_sent_with_its_kind() {
        let reading = Question::Reading(MediaReading {
            id: QuestionId(7),
            url: "https://videos.example/a".into(),
        });
        assert_eq!(
            serde_json::to_value(&reading).unwrap(),
            serde_json::json!({"kind": "reading", "data": {"id": 7, "url": "https://videos.example/a"}})
        );
        assert_eq!(reading.id(), QuestionId(7));
    }

    // A patch tells "unchanged" (missing) from "set to nothing" (null), and refuses a name it
    // does not know instead of ignoring it, which a misspelt setting would otherwise be.
    #[test]
    fn a_patch_reads_missing_null_and_unknown_fields_apart() {
        let read = |json: &str| serde_json::from_str::<AppSettingsPatch>(json);
        assert_eq!(read("{}").unwrap(), AppSettingsPatch::default());
        assert_eq!(
            read(r#"{"global_speed_limit": null}"#)
                .unwrap()
                .global_speed_limit,
            Update::Set(None)
        );
        assert_eq!(
            read(r#"{"global_speed_limit": 5}"#)
                .unwrap()
                .global_speed_limit,
            Update::Set(Some(5))
        );
        let kbps = read(r#"{"media": {"audio": {"audio_kbps": null}}}"#).unwrap();
        assert_eq!(
            kbps.media.and_then(|m| m.audio).map(|a| a.audio_kbps),
            Some(Update::Set(None))
        );
        assert_eq!(read("{}").unwrap().global_speed_limit, Update::Keep);
        // Plain fields write null, which reads back as unchanged; an emptiable one must not.
        let written = serde_json::to_string(&AppSettingsPatch::default()).unwrap();
        assert!(!written.contains("global_speed_limit"), "{written}");
        assert!(read(r#"{"no_such_setting": 1}"#).is_err());
        assert!(read(r#"{"media": {"video": {"no_such_setting": 1}}}"#).is_err());
        assert!(read(r#"{"theme": 42}"#).is_err());
    }

    /// A selection from a client that knows nothing of output formats keeps today's
    /// behaviour: the original format, no subtitles, no chapters.
    #[test]
    fn an_older_selection_keeps_the_original_format() {
        let selection: MediaSelection = serde_json::from_str(
            r#"{"option_id":"137+140","folder":null,"file_name":null,"queue":null}"#,
        )
        .unwrap();
        assert_eq!(selection.output, OutputFormat::Original);
        assert_eq!(selection.audio_kbps, None);
        assert!(selection.subtitles.is_empty() && !selection.subtitle_files && !selection.chapters);
    }

    #[test]
    fn output_formats_are_snake_case_words() {
        let all = [
            (OutputFormat::Original, "original"),
            (OutputFormat::Mp4, "mp4"),
            (OutputFormat::Mkv, "mkv"),
            (OutputFormat::Webm, "webm"),
            (OutputFormat::Mp3, "mp3"),
            (OutputFormat::M4a, "m4a"),
            (OutputFormat::Opus, "opus"),
            (OutputFormat::Flac, "flac"),
            (OutputFormat::Wav, "wav"),
        ];
        for (format, word) in all {
            assert_eq!(
                serde_json::to_string(&format).unwrap(),
                format!("\"{word}\"")
            );
        }
        assert_eq!(
            MediaDefaults::default().video.format,
            OutputFormat::Original
        );
    }

    #[test]
    fn each_format_is_for_videos_or_for_audio_and_only_the_original_for_both() {
        use OutputFormat as F;
        let video = [F::Original, F::Mp4, F::Mkv, F::Webm];
        let audio = [F::Original, F::Mp3, F::M4a, F::Opus, F::Flac, F::Wav];
        for format in video.into_iter().chain(audio).filter(|f| *f != F::Original) {
            assert_eq!(format.saves_video(), video.contains(&format), "{format:?}");
            assert_eq!(format.saves_audio(), audio.contains(&format), "{format:?}");
        }
        assert!(F::Original.saves_video() && F::Original.saves_audio());
        let lossless: Vec<F> = audio.into_iter().filter(|f| f.is_lossless()).collect();
        assert_eq!(lossless, [F::Flac, F::Wav]);
        assert!(video.iter().all(|f| !f.is_lossless()));
        // A lossless format has no bit rate to pick.
        assert!(lossless.iter().all(|f| f.kbps_choices().is_empty()));
    }
}
