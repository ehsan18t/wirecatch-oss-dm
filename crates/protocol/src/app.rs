//! Downloads, queues and settings as the desktop UI sees them.

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use ts_rs::TS;

use crate::extension::{ExtensionSettings, ExtensionSettingsPatch};
use crate::proxy::{ProxyServer, ProxyServerSettings, ProxySettings, ProxyUse, QueueExceptions};
use crate::rules::{AddDownloads, QueueReason};
use crate::secret::Link;
use crate::{DownloadId, ProxyServerId, QuestionId, QueueId};

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
    /// Why it is in that queue (D72.8).
    pub queue_reason: QueueReason,
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
    /// The proxy its last run started through, in words: a profile's name and address, the
    /// computer's proxy, or no proxy (D73.3). `None` before its first run.
    #[serde(default)]
    pub proxy: Option<String>,
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

/// Something the person should know about how Wirecatch started, such as a damaged download list that was set aside: shown as a system notification and in the main window, which may open long after the notification has gone (CORE-1).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct StartupNotice {
    /// A short title.
    pub title: String,
    /// What happened and what it means for the person, in whole sentences.
    pub message: String,
    /// A damaged download list was set aside: the window offers to show it in its folder (`show_set_aside_list`), so the person need not copy its path out of the message.
    pub set_aside: bool,
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
    /// An FTP link a proxy cannot carry: open the proxy settings (D71).
    ProxySettings,
    /// The proxy could not be reached or refused the login: move the download to a queue with
    /// another proxy (D73.4).
    ChangeQueue,
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
    /// Downloads go to this queue when none is named. There is always exactly one, and it
    /// cannot be deleted.
    pub is_default: bool,
    pub settings: QueueSettings,
    /// Changed only by the queue's own commands.
    pub status: QueueStatus,
}

/// A queue setting that uses the global value or is the queue's own (D72.1).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "source", content = "value", rename_all = "snake_case")]
#[ts(export)]
pub enum Setting<T> {
    #[default]
    Global,
    Own(T),
}

impl<T> Setting<T> {
    /// The queue's own value, `None` when it uses the global one.
    #[must_use]
    pub fn own(self) -> Option<T> {
        match self {
            Self::Global => None,
            Self::Own(value) => Some(value),
        }
    }

    /// The queue's own value by reference, `None` when it uses the global one.
    #[must_use]
    pub fn own_ref(&self) -> Option<&T> {
        match self {
            Self::Global => None,
            Self::Own(value) => Some(value),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct QueueSettings {
    /// The name the person gave it, unique among the queues.
    pub name: String,
    /// What the queue is for, in the person's words; may be empty.
    pub description: String,
    /// Downloads from this queue that may run at the same time. Following the app sets no limit of the queue's own; [`AppSettings::max_concurrent_downloads`] caps every queue.
    pub max_concurrent: Setting<u32>,
    /// Bytes per second shared by the whole queue. Following the app sets no limit of the queue's own; [`AppSettings::global_speed_limit`] caps every queue.
    #[ts(type = "Setting<number>")]
    pub speed_limit: Setting<u64>,
    /// Connections per download; following the app uses [`AppSettings::connections_per_download`].
    pub connections: Setting<u8>,
    /// The proxy its downloads go through (D73); following the app uses [`ProxySettings::proxy`].
    pub proxy: Setting<ProxyUse>,
    /// The hosts its downloads reach directly; following the app uses its exceptions (D73.7).
    pub exceptions: Setting<QueueExceptions>,
    /// When set, the queue only runs inside this window.
    pub schedule: Option<Schedule>,
    /// Manual, or auto with the new downloads it rules (D72.2).
    pub add_downloads: AddDownloads,
    /// When the person is told its downloads finished (D52).
    #[serde(default, with = "known_or_default")]
    #[ts(as = "QueueNotify")]
    pub notify: QueueNotify,
    /// Where its downloads are saved (D94); following the app picks a folder by file type.
    #[serde(default)]
    pub folder: Setting<PortablePath>,
}

impl QueueSettings {
    #[must_use]
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            description: String::new(),
            max_concurrent: Setting::Global,
            speed_limit: Setting::Global,
            connections: Setting::Global,
            proxy: Setting::Global,
            exceptions: Setting::Global,
            add_downloads: AddDownloads::Manual,
            schedule: None,
            notify: QueueNotify::EachDownload,
            folder: Setting::Global,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct QueueStatus {
    /// A paused queue starts nothing new, including downloads added later, and
    /// its running downloads are paused. Resuming it does not resume downloads
    /// the user paused one by one.
    pub paused: bool,
    /// Set by "start now": the queue runs outside its schedule until it is empty
    /// or paused, then the schedule applies again.
    pub running_outside_schedule: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct SettingsFileQueue {
    /// Its id where the profile was written; the profile's settings refer to it.
    pub id: QueueId,
    pub is_default: bool,
    pub settings: QueueSettings,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct SettingsFileProxy {
    /// Its id where the profile was written; the profile's settings and queues refer to it.
    pub id: ProxyServerId,
    pub settings: ProxyServerSettings,
}

/// A queue that is done: nothing in it downloads or waits to start (D52). It covers the
/// downloads that finished or failed since the queue last started working, including those
/// before a restart; downloads the person paused, cancelled or removed are not in it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct QueueReport {
    /// The queue.
    pub queue: QueueId,
    /// Its name when it finished.
    pub name: String,
    /// What finished, in the order it finished.
    pub completed: Vec<DownloadSummary>,
    /// What failed, in the order it failed.
    pub failed: Vec<DownloadSummary>,
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
    /// The saved resolution the video did not offer, when it was downloaded at another one
    /// without asking, for example "1080p" (D50).
    #[serde(default)]
    pub instead_of: Option<String>,
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
    /// The folder for its file type, where a queue that follows the app saves it (D94.4); none
    /// when the person picked the folder as they added the link.
    pub type_folder: Option<String>,
    /// The queue it goes to if the user just confirms.
    pub queue: QueueId,
    /// Why it goes to that queue (D72.8).
    pub queue_reason: QueueReason,
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
    /// Where the window starts (D50): the quality the saved resolution picks, or its fallback
    /// when the video does not offer it, saved as the saved choices say, as far as this video
    /// offers them ([`MediaDefaults::start`]).
    pub start: MediaSelection,
    /// The saved resolution, when this video does not offer it, for example "1080p". A pick
    /// from such a video leaves the saved resolution as it is.
    #[serde(default)]
    pub missing_quality: Option<String>,
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
    /// With subtitles inside the video, what happens to them when the format cannot hold any
    /// (Original, or audio only) (D93).
    #[serde(default, with = "known_or_default")]
    #[ts(as = "UnembeddedSubtitles")]
    pub unembedded_subtitles: UnembeddedSubtitles,
    /// Add the video's chapters to the file, when the format can hold them.
    #[serde(default)]
    pub chapters: bool,
    /// "Use these choices for every video" (D50): true, from now on a video downloads with the
    /// choices made here without asking; false asks for each video again; `None` leaves that
    /// as it is.
    #[serde(default)]
    pub every_video: Option<bool>,
}

/// What the quality window starts from (D24) and whether it opens at all (D50): the last
/// choices, kept separately for videos and for audio only, the resolution, and what to do when
/// a video does not offer it. Also in Settings, Media, and in the settings profile.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct MediaDefaults {
    /// Where the quality window starts for a video.
    #[serde(default)]
    pub video: OutputDefaults,
    /// Where the quality window starts for audio only.
    #[serde(default)]
    pub audio: OutputDefaults,
    /// Open the quality window for every video. Off, a video downloads with these choices
    /// without asking, unless it does not offer [`quality`](Self::quality) and the fallback
    /// is to ask, or the media add-on must be installed first (D50).
    #[serde(default = "default_true")]
    pub ask_each_time: bool,
    /// The resolution picked first; every pick from the quality window becomes it.
    #[serde(default, with = "known_or_default")]
    #[ts(as = "VideoQuality")]
    pub quality: VideoQuality,
    /// What is picked when a video does not offer [`quality`](Self::quality).
    #[serde(default, with = "known_or_default")]
    #[ts(as = "QualityFallback")]
    pub fallback: QualityFallback,
}

impl Default for MediaDefaults {
    fn default() -> Self {
        Self {
            video: OutputDefaults::default(),
            audio: OutputDefaults::default(),
            ask_each_time: true,
            quality: VideoQuality::default(),
            fallback: QualityFallback::default(),
        }
    }
}

/// The resolution a video is downloaded at (D50).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum VideoQuality {
    /// The best the video has.
    #[default]
    Best,
    /// Sound only, saved as the choices for audio only say.
    AudioOnly,
    /// This height in pixels, as a quality's label says it ("1080p").
    Height(u32),
}

/// What is picked when a video does not offer the saved resolution (D50).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum QualityFallback {
    /// The closest lower resolution, or the lowest the video has when none is lower.
    #[default]
    Lower,
    /// The closest higher resolution, or the best the video has when none is higher.
    Higher,
    /// The quality window opens, on the closest lower resolution.
    Ask,
}

/// The starting choices for one kind of media download.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
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
    /// Take a language's auto-generated subtitles when it has none written by people.
    #[serde(default = "default_true")]
    pub auto_subtitles: bool,
    /// Save subtitles as `.srt` files next to a video instead of inside it.
    #[serde(default)]
    pub subtitle_files: bool,
    /// With subtitles inside the video, what happens to them when the format cannot hold any (D93).
    #[serde(default, with = "known_or_default")]
    #[ts(as = "UnembeddedSubtitles")]
    pub unembedded_subtitles: UnembeddedSubtitles,
    /// Add chapters to the file.
    #[serde(default)]
    pub chapters: bool,
}

impl Default for OutputDefaults {
    fn default() -> Self {
        Self {
            format: OutputFormat::default(),
            audio_kbps: None,
            subtitle_languages: Vec::new(),
            auto_subtitles: true,
            subtitle_files: false,
            unembedded_subtitles: UnembeddedSubtitles::Files,
            chapters: false,
        }
    }
}

/// What happens to subtitles that were to go inside a file whose format cannot hold them: the
/// Original format, which keeps the site's file as it is, or audio only (D93).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum UnembeddedSubtitles {
    /// Saved as `.srt` files next to it.
    #[default]
    Files,
    /// Left out.
    Skip,
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

/// The Public Suffix List the app decides which sites are one owner's with (D44), as the
/// settings show it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct SuffixListInfo {
    /// When publicsuffix.org published the list in use, in milliseconds since the Unix epoch.
    #[ts(type = "number")]
    pub published: i64,
    /// When the app last looked for a newer list, in milliseconds since the Unix epoch;
    /// `None` until it first does.
    #[ts(type = "number | null")]
    pub checked: Option<i64>,
    /// Whether that last look failed (no answer, a list refused or not saved), so a newer
    /// list may exist; it is tried again a day later.
    pub check_failed: bool,
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
        /// The queue's [`QueueSettings::max_concurrent`].
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

/// Whether the app is light or dark.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum Theme {
    /// Follow the operating system's light or dark setting, and the default.
    #[default]
    System,
    /// Always light.
    Light,
    /// Always dark.
    Dark,
}

/// The colour that marks what is selected, focused or running. The design system defines one
/// palette for each, in light and dark (`data-accent` on the page's root element).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum Accent {
    /// Wirecatch's own colour, and the default.
    #[default]
    Cyan,
    /// Purple.
    Purple,
    /// Blue.
    Blue,
    /// Green.
    Green,
    /// Orange.
    Orange,
    /// Pink.
    Pink,
    /// Red.
    Red,
    /// Teal.
    Teal,
    /// Indigo.
    Indigo,
    /// Amber.
    Amber,
    /// The accent colour chosen in Windows; [`Accent::Cyan`] where the system has none.
    System,
}

/// How the person is told that downloads finished or failed (D52). Each queue decides when: for
/// each download, or once the queue is done ([`QueueNotify`]). A failure is told at once either
/// way.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum NotifyOnFinish {
    /// A system notification with buttons to open the file or show it in its folder. The default
    /// (D90): a download usually finishes while its window or another program is in front.
    #[default]
    Notification,
    /// A notice in the browser tab the person is looking at, with the same buttons (D65); a
    /// system notification when no browser can show it.
    Browser,
    /// The download complete window, which collects what finished until the person closes it.
    Window,
    /// Nothing: the download list shows how each download ended.
    Nothing,
}

/// What a download does when the file it would save is already in the folder: same name, and the
/// same size and source where those are known (D88). A file that only shares the name gets a new
/// one whatever this says.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum WhenFileExists {
    /// Ask with a [`FileExistsQuestion`].
    #[default]
    Ask,
    /// Download it again over the file there.
    Replace,
    /// Download nothing: the file there is the download, completed.
    KeepExisting,
    /// Download it under a new name, `file (1).zip`.
    KeepBoth,
}

/// How the person is told that a browser download started without a window of its own (D65).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum NotifyOnStart {
    /// A notice in the browser tab the person is looking at; a system notification when no
    /// browser can show it.
    #[default]
    Browser,
    /// Nothing: the download list shows it.
    Nothing,
}

/// How long a browser notice about a download that started stays before it goes by itself (D65):
/// short, since the person just started it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum StartNoticeTime {
    /// Three seconds.
    #[default]
    ThreeSeconds,
    /// Five seconds.
    FiveSeconds,
    /// Eight seconds.
    EightSeconds,
    /// Ten seconds.
    TenSeconds,
}

impl StartNoticeTime {
    /// How long it stays, in seconds.
    #[must_use]
    pub const fn seconds(self) -> u64 {
        match self {
            StartNoticeTime::ThreeSeconds => 3,
            StartNoticeTime::FiveSeconds => 5,
            StartNoticeTime::EightSeconds => 8,
            StartNoticeTime::TenSeconds => 10,
        }
    }

    /// How long it stays, in milliseconds.
    #[must_use]
    pub const fn millis(self) -> u64 {
        self.seconds() * 1000
    }
}

/// How long a browser notice about finished downloads stays before it goes by itself (D65). A notice
/// about a failure stays until it is closed, whatever this says.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum FinishNoticeTime {
    /// Five seconds.
    FiveSeconds,
    /// Eight seconds.
    #[default]
    EightSeconds,
    /// Fifteen seconds.
    FifteenSeconds,
    /// Until the person closes it.
    UntilClosed,
}

impl FinishNoticeTime {
    /// How long it stays in seconds, or `None` until the person closes it.
    #[must_use]
    pub const fn seconds(self) -> Option<u64> {
        match self {
            FinishNoticeTime::FiveSeconds => Some(5),
            FinishNoticeTime::EightSeconds => Some(8),
            FinishNoticeTime::FifteenSeconds => Some(15),
            FinishNoticeTime::UntilClosed => None,
        }
    }

    /// How long it stays in milliseconds, or `None` until the person closes it.
    #[must_use]
    pub fn millis(self) -> Option<u64> {
        self.seconds().map(|seconds| seconds * 1000)
    }
}

/// When a queue tells the person its downloads finished (D52). How is [`NotifyOnFinish`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum QueueNotify {
    /// Each time one of its downloads finishes.
    #[default]
    EachDownload,
    /// Once, when nothing in the queue downloads or waits to start ([`QueueReport`]).
    WhenDone,
}

/// A setting a newer Wirecatch may have saved with a value this build does not know (a theme
/// or accent added later, say): it reads as the setting's default, so the rest of the settings
/// still read. A missing field takes the default through `#[serde(default)]`.
pub(crate) mod known_or_default {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    pub fn serialize<S: Serializer, T: Serialize>(
        value: &T,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        value.serialize(serializer)
    }

    pub fn deserialize<'de, D, T>(deserializer: D) -> Result<T, D::Error>
    where
        D: Deserializer<'de>,
        T: Deserialize<'de> + Default,
    {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Stored<T> {
            Known(T),
            Unknown(serde::de::IgnoredAny),
        }
        Ok(match Stored::<T>::deserialize(deserializer)? {
            Stored::Known(value) => value,
            Stored::Unknown(_) => T::default(),
        })
    }
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
    /// The new [`AppSettings::accent`].
    pub accent: Option<Accent>,
    /// The new [`AppSettings::start_with_system`].
    pub start_with_system: Option<bool>,
    /// The new [`AppSettings::close_to_tray`].
    pub close_to_tray: Option<bool>,
    /// The new [`AppSettings::notify_on_finish`].
    pub notify_on_finish: Option<NotifyOnFinish>,
    /// The new [`AppSettings::finish_notice_time`].
    pub finish_notice_time: Option<FinishNoticeTime>,
    /// The new [`AppSettings::notify_on_start`].
    pub notify_on_start: Option<NotifyOnStart>,
    /// The new [`AppSettings::start_notice_time`].
    pub start_notice_time: Option<StartNoticeTime>,
    /// The new [`AppSettings::notify_during_do_not_disturb`].
    pub notify_during_do_not_disturb: Option<bool>,
    /// The new [`AppSettings::auto_update_addons`].
    pub auto_update_addons: Option<bool>,
    /// The new [`AppSettings::show_progress_windows`].
    pub show_progress_windows: Option<bool>,
    /// The new [`AppSettings::when_file_exists`].
    pub when_file_exists: Option<WhenFileExists>,
    /// Changes to [`AppSettings::media`].
    pub media: Option<MediaDefaultsPatch>,
    /// The new [`AppSettings::local_sites`], replacing the list.
    pub local_sites: Option<Vec<String>>,
    /// The new [`AppSettings::update_suffix_list`].
    pub update_suffix_list: Option<bool>,
    /// The new [`AppSettings::prerelease_updates`].
    pub prerelease_updates: Option<bool>,
    /// The new [`AppSettings::proxy`], replacing it whole.
    pub proxy: Option<ProxySettings>,
}

/// A change to some of [`MediaDefaults`].
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
#[ts(export, optional_fields)]
pub struct MediaDefaultsPatch {
    // A field left as it is is left out when written, as the TypeScript type has it.
    /// Changes to [`MediaDefaults::video`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub video: Option<OutputDefaultsPatch>,
    /// Changes to [`MediaDefaults::audio`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub audio: Option<OutputDefaultsPatch>,
    /// The new [`MediaDefaults::ask_each_time`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ask_each_time: Option<bool>,
    /// The new [`MediaDefaults::quality`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub quality: Option<VideoQuality>,
    /// The new [`MediaDefaults::fallback`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fallback: Option<QualityFallback>,
}

/// A change to some of [`OutputDefaults`].
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
#[ts(export, optional_fields)]
pub struct OutputDefaultsPatch {
    // A field left as it is is left out when written, as the TypeScript type has it.
    /// The new [`OutputDefaults::format`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub format: Option<OutputFormat>,
    /// The new quality in kbps; `null` goes back to the format's usual quality.
    #[serde(default, skip_serializing_if = "Update::is_keep")]
    #[ts(optional, as = "Option<Option<u32>>")]
    pub audio_kbps: Update<Option<u32>>,
    /// The new [`OutputDefaults::subtitle_languages`], replacing the list.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subtitle_languages: Option<Vec<String>>,
    /// The new [`OutputDefaults::auto_subtitles`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auto_subtitles: Option<bool>,
    /// The new [`OutputDefaults::subtitle_files`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subtitle_files: Option<bool>,
    /// The new [`OutputDefaults::unembedded_subtitles`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unembedded_subtitles: Option<UnembeddedSubtitles>,
    /// The new [`OutputDefaults::chapters`].
    #[serde(skip_serializing_if = "Option::is_none")]
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
    /// [`AppSettings::max_concurrent_downloads`] and each [`QueueSettings::max_concurrent`].
    pub const MAX_CONCURRENT_DOWNLOADS: u32 = 100;
    /// [`AppSettings::max_concurrent_downloads`] on a fresh install.
    pub const DEFAULT_MAX_CONCURRENT_DOWNLOADS: u32 = 5;
    /// Most characters in [`QueueSettings::description`].
    pub const MAX_QUEUE_DESCRIPTION_CHARS: usize = 500;
    /// [`ProcessingProgress::per_mille`] of a finished step: progress is counted in tenths of
    /// a percent.
    pub const PER_MILLE_DONE: u16 = 1000;
    /// The only [`SettingsFile::format_version`] this build reads and writes (D75).
    pub const SETTINGS_FILE_FORMAT_VERSION: u32 = 3;
    /// Most download windows asking a question at once (D23); later questions wait until one
    /// of them closes.
    pub const MAX_QUESTION_WINDOWS: usize = 5;
    /// How far each download window sits down and right of the one before it, in logical
    /// pixels.
    pub const DOWNLOAD_WINDOW_CASCADE_STEP: u32 = 28;
    /// A download window's width, in logical pixels.
    pub const DOWNLOAD_WINDOW_WIDTH: u32 = 460;
    /// The least height a download window keeps, in logical pixels, however little it shows:
    /// below the shortest question, so every window fits what it asks.
    pub const DOWNLOAD_WINDOW_MIN_HEIGHT: u32 = 200;
    /// The most height a download window takes, in logical pixels, however much it shows: past
    /// it the window's body scrolls, so opening a folded group never makes a towering window.
    pub const DOWNLOAD_WINDOW_MAX_HEIGHT: u32 = 680;
    /// Every [`Theme`], in the order settings offer them.
    pub const THEMES: &[Theme] = &[Theme::System, Theme::Light, Theme::Dark];
    /// The fixes that trying the download again can carry out, so a notification and the
    /// download complete window offer Try again for them (D52). The others need the person's
    /// answer first: a fresh link, another folder, or keeping the original.
    pub const RETRY_FIXES: &[FixAction] = &[FixAction::Retry, FixAction::FreeDiskSpace];
    /// Every [`StartNoticeTime`] with its seconds, in the order settings offer them (D65).
    pub const START_NOTICE_TIMES: &[(StartNoticeTime, u64)] = &[
        (StartNoticeTime::ThreeSeconds, StartNoticeTime::ThreeSeconds.seconds()),
        (StartNoticeTime::FiveSeconds, StartNoticeTime::FiveSeconds.seconds()),
        (StartNoticeTime::EightSeconds, StartNoticeTime::EightSeconds.seconds()),
        (StartNoticeTime::TenSeconds, StartNoticeTime::TenSeconds.seconds()),
    ];
    /// Every [`FinishNoticeTime`] with its seconds (`null` until closed), in the order settings
    /// offer them (D65).
    pub const FINISH_NOTICE_TIMES: &[(FinishNoticeTime, Option<u64>)] = &[
        (FinishNoticeTime::FiveSeconds, FinishNoticeTime::FiveSeconds.seconds()),
        (FinishNoticeTime::EightSeconds, FinishNoticeTime::EightSeconds.seconds()),
        (FinishNoticeTime::FifteenSeconds, FinishNoticeTime::FifteenSeconds.seconds()),
        (FinishNoticeTime::UntilClosed, FinishNoticeTime::UntilClosed.seconds()),
    ];
    /// Every [`Accent`], in the order settings offer them. The design system in `packages/ui`
    /// reads this list and defines a palette for each of these but `system`; its tests fail on
    /// an accent without one.
    pub const ACCENTS: &[Accent] = &[
        Accent::Cyan,
        Accent::Purple,
        Accent::Blue,
        Accent::Green,
        Accent::Orange,
        Accent::Pink,
        Accent::Red,
        Accent::Teal,
        Accent::Indigo,
        Accent::Amber,
        Accent::System,
    ];
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
    /// Light, dark, or following the system.
    #[serde(default, with = "known_or_default")]
    #[ts(as = "Theme")]
    pub theme: Theme,
    /// The accent colour.
    #[serde(default, with = "known_or_default")]
    #[ts(as = "Accent")]
    pub accent: Accent,
    /// Start Wirecatch when the person signs in to the computer.
    pub start_with_system: bool,
    /// Closing the window keeps downloads running in the tray.
    pub close_to_tray: bool,
    /// How the person is told that downloads finished or failed (D52).
    #[serde(default, with = "known_or_default")]
    #[ts(as = "NotifyOnFinish")]
    pub notify_on_finish: NotifyOnFinish,
    /// How long a browser notice about finished downloads stays (D65).
    #[serde(default, with = "known_or_default")]
    #[ts(as = "FinishNoticeTime")]
    pub finish_notice_time: FinishNoticeTime,
    /// How the person is told that a browser download started without a window (D65).
    #[serde(default, with = "known_or_default")]
    #[ts(as = "NotifyOnStart")]
    pub notify_on_start: NotifyOnStart,
    /// How long a browser notice about a download that started stays (D65).
    #[serde(default, with = "known_or_default")]
    #[ts(as = "StartNoticeTime")]
    pub start_notice_time: StartNoticeTime,
    /// Notifications about downloads that finished or failed, and problems the person must know
    /// about, show even while the system's Do Not Disturb is on (D64). Off: Do Not Disturb holds
    /// them back, as it does any app's. The brief notice of a download that started is never
    /// one of them.
    #[serde(default = "default_true")]
    pub notify_during_do_not_disturb: bool,
    /// Keep installed add-ons up to date without asking.
    #[serde(default = "default_true")]
    pub auto_update_addons: bool,
    /// After Download now, a browser download's window stays open and shows its progress
    /// (D23). Off: the window closes when the download starts.
    #[serde(default = "default_true")]
    pub show_progress_windows: bool,
    /// What a download does when its file is already in the folder (D88).
    #[serde(default, with = "known_or_default")]
    #[ts(as = "WhenFileExists")]
    pub when_file_exists: WhenFileExists,
    /// What the quality window for a video starts from (D24).
    #[serde(default)]
    pub media: MediaDefaults,
    /// Sites on the person's network: host names, each also covering the names
    /// under it (`corp.example` covers `wiki.corp.example`). A link captured from
    /// a page on one of them may lead to devices on the local network, like a
    /// page whose own address is local (D39). Empty by default.
    #[serde(default)]
    pub local_sites: Vec<String>,
    /// Download a newer Public Suffix List from publicsuffix.org once a week while
    /// downloads are idle (D44). Off: the list in use stays until the person checks.
    #[serde(default = "default_true")]
    pub update_suffix_list: bool,
    /// Offer the app's pre-release versions as updates too, not only full releases (D111).
    #[serde(default)]
    pub prerelease_updates: bool,
    /// The proxy and exceptions of every queue that follows the app's (D73).
    #[serde(default)]
    pub proxy: ProxySettings,
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
            accent,
            start_with_system,
            close_to_tray,
            notify_on_finish,
            finish_notice_time,
            notify_on_start,
            start_notice_time,
            notify_during_do_not_disturb,
            auto_update_addons,
            show_progress_windows,
            when_file_exists,
            media,
            local_sites,
            update_suffix_list,
            prerelease_updates,
            proxy,
        } = self;
        let AppSettingsPatch {
            default_folder: new_default_folder,
            categories: new_categories,
            connections_per_download: new_connections_per_download,
            max_concurrent_downloads: new_max_concurrent_downloads,
            global_speed_limit: new_global_speed_limit,
            extension: extension_patch,
            theme: new_theme,
            accent: new_accent,
            start_with_system: new_start_with_system,
            close_to_tray: new_close_to_tray,
            notify_on_finish: new_notify_on_finish,
            finish_notice_time: new_finish_notice_time,
            notify_on_start: new_notify_on_start,
            start_notice_time: new_start_notice_time,
            notify_during_do_not_disturb: new_notify_during_do_not_disturb,
            auto_update_addons: new_auto_update_addons,
            show_progress_windows: new_show_progress_windows,
            when_file_exists: new_when_file_exists,
            media: media_patch,
            local_sites: new_local_sites,
            update_suffix_list: new_update_suffix_list,
            prerelease_updates: new_prerelease_updates,
            proxy: new_proxy,
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
        set(accent, new_accent);
        set(start_with_system, new_start_with_system);
        set(close_to_tray, new_close_to_tray);
        set(notify_on_finish, new_notify_on_finish);
        set(finish_notice_time, new_finish_notice_time);
        set(notify_on_start, new_notify_on_start);
        set(start_notice_time, new_start_notice_time);
        set(
            notify_during_do_not_disturb,
            new_notify_during_do_not_disturb,
        );
        set(auto_update_addons, new_auto_update_addons);
        set(show_progress_windows, new_show_progress_windows);
        set(when_file_exists, new_when_file_exists);
        if let Some(media_patch) = media_patch {
            media.apply(media_patch);
        }
        set(local_sites, new_local_sites);
        set(update_suffix_list, new_update_suffix_list);
        set(prerelease_updates, new_prerelease_updates);
        set(proxy, new_proxy);
    }
}

impl MediaDefaults {
    /// Applies a patch; see [`AppSettings::apply`].
    pub fn apply(&mut self, patch: MediaDefaultsPatch) {
        let Self {
            video,
            audio,
            ask_each_time,
            quality,
            fallback,
        } = self;
        let MediaDefaultsPatch {
            video: video_patch,
            audio: audio_patch,
            ask_each_time: new_ask_each_time,
            quality: new_quality,
            fallback: new_fallback,
        } = patch;
        if let Some(video_patch) = video_patch {
            video.apply(video_patch);
        }
        if let Some(audio_patch) = audio_patch {
            audio.apply(audio_patch);
        }
        set(ask_each_time, new_ask_each_time);
        set(quality, new_quality);
        set(fallback, new_fallback);
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
            unembedded_subtitles,
            chapters,
        } = self;
        let OutputDefaultsPatch {
            format: new_format,
            audio_kbps: new_audio_kbps,
            subtitle_languages: new_subtitle_languages,
            auto_subtitles: new_auto_subtitles,
            subtitle_files: new_subtitle_files,
            unembedded_subtitles: new_unembedded_subtitles,
            chapters: new_chapters,
        } = patch;
        set(format, new_format);
        update(audio_kbps, new_audio_kbps);
        set(subtitle_languages, new_subtitle_languages);
        set(auto_subtitles, new_auto_subtitles);
        set(subtitle_files, new_subtitle_files);
        set(unembedded_subtitles, new_unembedded_subtitles);
        set(chapters, new_chapters);
    }
}

/// The file written by settings export and read by import (D6).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct SettingsFile {
    /// Version of this file's layout; an app refuses a profile newer than it understands.
    pub format_version: u32,
    /// Version of the app that wrote it.
    pub app_version: String,
    /// When it was written, in Unix time in milliseconds.
    #[ts(type = "number")]
    pub exported_at: i64,
    /// Every setting.
    pub settings: AppSettings,
    /// Every queue, with its limits and schedule, in sidebar order.
    pub queues: Vec<SettingsFileQueue>,
    /// Every proxy server, without its password or last test.
    pub proxy_servers: Vec<SettingsFileProxy>,
    /// The installed add-ons, offered for installing on import.
    pub addons: Vec<AddonId>,
}

/// A download added from the app itself (paste, Add dialog, command palette).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct AddRequest {
    /// The address to download.
    pub url: Link,
    /// The queue picked; absent, the rules place it, else it joins the default queue (D72.3).
    pub queue: Option<QueueId>,
    /// Absolute folder; absent, the queue's folder or the one for the file type decides (D94.2).
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
    /// The folder for its file type, where a queue that follows the app saves it (D94.4).
    pub type_folder: String,
    /// The kind of file, which picked the folder.
    pub category: Category,
    /// Size in bytes, when known.
    #[ts(type = "number | null")]
    pub size: Option<u64>,
    /// The queue it goes to if the user just confirms.
    pub queue: QueueId,
    /// Why it goes to that queue (D72.8).
    pub queue_reason: QueueReason,
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
    /// Every proxy server, after any changed or was tested.
    ProxyServersChanged(Vec<ProxyServer>),
    /// Every setting, after any changed. Boxed: the settings are far larger than any other event.
    SettingsChanged(Box<AppSettings>),
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
    /// A download found its file already in the folder and asks what to do (D88).
    FileExistsRequested(FileExistsQuestion),
    /// The browser sent a link that is already a question or a download in progress, which it
    /// joined instead of starting again: its window comes to the front (D89).
    CaptureRepeated(Repeated),
    /// A video or stream from the browser was handed over and is read for its qualities
    /// without a window (D50): the desktop tells the person it was added at once, before the
    /// read ends (D67). Its download is not told again when it starts.
    MediaCaptured {
        /// The id the read has, as a question it may yet ask.
        id: QuestionId,
        /// Its title, or its address with any password removed.
        title: String,
    },
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
    /// The Public Suffix List after each look for a newer one, the weekly one or Check now,
    /// whether it found one, none or failed (D44).
    SuffixListChecked(SuffixListInfo),
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
    /// A second launch of the app handed over links (D103): the desktop shows its main window with the Add dialog holding them.
    LinksRequested(Vec<String>),
    /// A client asked for the browser settings (the popup's settings button, D61): the desktop shows its main window on them.
    BrowserSettingsRequested,
    /// A browser showed the notice with this id (D65): the desktop tells no one else.
    NoticeShown(crate::extension::NoticeId),
    /// The person pressed one of a browser notice's buttons (D65): the desktop does what it stands
    /// for.
    NoticePressed(crate::extension::NoticePressed),
    /// A queue is done; the desktop tells the person when the queue is set to
    /// [`QueueNotify::WhenDone`] (D52).
    QueueFinished(QueueReport),
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

/// A download found the file it would save already in the folder and asks what to do (D88). The
/// download is paused until the answer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct FileExistsQuestion {
    /// Identifies this question.
    pub id: QuestionId,
    /// The download that asks.
    pub download: DownloadId,
    /// Where it is downloaded from, with any password removed.
    pub url: String,
    /// The name of the file in the folder, which the download would save as.
    pub file_name: String,
    /// The folder.
    pub folder: String,
    /// Size of the file in the folder, in bytes.
    #[ts(type = "number")]
    pub size: u64,
    /// The file in the folder looks like the one the download would save; otherwise it is a
    /// different file with the same name (D96).
    #[serde(default)]
    pub same_file: bool,
    /// When the file in the folder was last changed, in Unix time in milliseconds.
    #[ts(type = "number | null")]
    pub saved_at: Option<i64>,
}

/// The person's answer to a [`FileExistsQuestion`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum FileExistsChoice {
    /// Download it again over the file in the folder.
    Replace,
    /// Download it under a new name.
    KeepBoth,
    /// Cancel the download; the file in the folder stays.
    Cancel,
}

/// The answer to a [`FileExistsQuestion`], and whether it becomes [`AppSettings::when_file_exists`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct FileExistsAnswer {
    /// What to do.
    pub choice: FileExistsChoice,
    /// Do the same every time: saves [`FileExistsChoice::Replace`] or [`FileExistsChoice::KeepBoth`]
    /// as the setting; ignored with [`FileExistsChoice::Cancel`], which is never remembered.
    pub remember: bool,
}

/// What a link the browser sent again joined (D89).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "kind", content = "data", rename_all = "snake_case")]
#[ts(export)]
pub enum Repeated {
    /// A question still open: a file waiting for confirmation, or a video being read or
    /// waiting for its quality.
    Question(QuestionId),
    /// A download not finished yet: queued, running or paused.
    Download(DownloadId),
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
    /// A download whose file is already in the folder (D88).
    FileExists(FileExistsQuestion),
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
            Question::FileExists(question) => question.id,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // A link pasted into the app may carry a sign-in (`ftp://me:pw@host`), and printing the
    // request printed it (W4I review L2).
    #[test]
    fn a_pasted_link_prints_without_its_sign_in() {
        let request = AddRequest {
            url: "ftp://me:password-that-must-not-print@files.example/a.zip".into(),
            queue: None,
            folder: None,
            file_name: None,
            start: true,
        };
        let printed = format!("{request:?}");
        assert!(!printed.contains("must-not-print"), "{printed}");
        assert!(printed.contains("files.example/a.zip"), "{printed}");
        // The wire keeps the whole link.
        let json = serde_json::to_value(&request).unwrap();
        assert_eq!(json["url"], request.url.as_str());
        assert_eq!(serde_json::from_value::<AddRequest>(json).unwrap(), request);
    }

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

    fn stored_settings() -> serde_json::Value {
        serde_json::json!({
            "default_folder": "{Downloads}",
            "categories": [],
            "connections_per_download": 8,
            "max_concurrent_downloads": 5,
            "global_speed_limit": null,
            "extension": ExtensionSettings::default(),
            "theme": "dark",
            "accent": "green",
            "start_with_system": false,
            "close_to_tray": true,
            "notify_on_finish": "window",
        })
    }

    fn read_settings(value: serde_json::Value) -> AppSettings {
        serde_json::from_value(value).unwrap()
    }

    /// The popup shows a change with a port of `BrowserControls::apply` before the app confirms
    /// it, while the app saves it with `AppSettings::apply`: both give the same settings (D61).
    #[test]
    fn browser_controls_apply_as_the_app_settings_do() {
        for (before, patch) in crate::extension::browser_controls_cases() {
            let mut settings = read_settings(stored_settings());
            settings.global_speed_limit = before.global_speed_limit;
            settings.max_concurrent_downloads = before.max_concurrent_downloads;
            settings.media = before.media.clone();
            settings.apply(patch.clone().into());
            let mut controls = before;
            controls.apply(patch);
            assert_eq!(settings.browser_controls(), controls);
        }
    }

    #[test]
    fn appearance_settings_round_trip() {
        let settings = read_settings(stored_settings());
        assert_eq!(
            (settings.theme, settings.accent),
            (Theme::Dark, Accent::Green)
        );
        let written = serde_json::to_value(&settings).unwrap();
        assert_eq!(
            (&written["theme"], &written["accent"]),
            (&"dark".into(), &"green".into())
        );
        assert_eq!(read_settings(written), settings);
    }

    /// D64: settings saved before the switch existed show notifications during Do Not Disturb,
    /// as a fresh profile does.
    #[test]
    fn settings_saved_before_the_do_not_disturb_switch_notify_through_it() {
        let settings = read_settings(stored_settings());
        assert!(settings.notify_during_do_not_disturb);
        let mut off = settings.clone();
        off.apply(AppSettingsPatch {
            notify_during_do_not_disturb: Some(false),
            ..AppSettingsPatch::default()
        });
        assert!(!off.notify_during_do_not_disturb);
    }

    /// Settings saved before the accent existed have none, and a newer build may have saved a
    /// theme or accent this one does not know: each reads as its default, and the rest of the
    /// settings are kept rather than lost with the one value.
    #[test]
    fn appearance_settings_read_their_default_when_missing_or_unknown() {
        let mut older = stored_settings();
        let fields = older.as_object_mut().unwrap();
        fields.remove("theme");
        fields.remove("accent");
        let settings = read_settings(older);
        assert_eq!(
            (settings.theme, settings.accent),
            (Theme::System, Accent::Cyan)
        );
        assert_eq!(settings.connections_per_download, 8);

        let mut newer = stored_settings();
        newer["theme"] = "high_contrast".into();
        newer["accent"] = serde_json::json!({"custom": "#123456"});
        let settings = read_settings(newer);
        assert_eq!(
            (settings.theme, settings.accent),
            (Theme::System, Accent::Cyan)
        );
        assert_eq!(settings.max_concurrent_downloads, 5);
    }

    /// The design system's stylesheet names its accents with these words (`data-accent`), and
    /// the desktop shell's first-paint script writes them as they are.
    #[test]
    fn accents_and_themes_are_snake_case_words() {
        let accents = [
            (Accent::Cyan, "cyan"),
            (Accent::Purple, "purple"),
            (Accent::Blue, "blue"),
            (Accent::Green, "green"),
            (Accent::Orange, "orange"),
            (Accent::Pink, "pink"),
            (Accent::Red, "red"),
            (Accent::Teal, "teal"),
            (Accent::Indigo, "indigo"),
            (Accent::Amber, "amber"),
            (Accent::System, "system"),
        ];
        for (accent, word) in accents {
            assert_eq!(serde_json::to_value(accent).unwrap(), word);
        }
        assert_eq!(ACCENTS, accents.map(|(accent, _)| accent));
        let themes = [
            (Theme::System, "system"),
            (Theme::Light, "light"),
            (Theme::Dark, "dark"),
        ];
        assert_eq!(THEMES, themes.map(|(theme, _)| theme));
        for (theme, word) in themes {
            assert_eq!(serde_json::to_value(theme).unwrap(), word);
        }
    }

    #[test]
    fn a_patch_changes_the_accent_alone() {
        let mut settings = read_settings(stored_settings());
        let before = settings.clone();
        settings.apply(serde_json::from_str(r#"{"accent": "system"}"#).unwrap());
        assert_eq!(settings.accent, Accent::System);
        assert_eq!(
            AppSettings {
                accent: before.accent,
                ..settings
            },
            before
        );
        assert!(serde_json::from_str::<AppSettingsPatch>(r#"{"accent": "beige"}"#).is_err());
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
