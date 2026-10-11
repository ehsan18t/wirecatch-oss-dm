//! Downloads, queues and settings as the desktop UI sees them.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::extension::ExtensionSettings;
use crate::{DownloadId, QueueId};

/// One row of the download list.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct DownloadSummary {
    pub id: DownloadId,
    pub url: String,
    pub file_name: String,
    /// Full path of the final file.
    pub path: String,
    pub queue: QueueId,
    pub category: Category,
    pub status: DownloadStatus,
    #[ts(type = "number | null")]
    pub total: Option<u64>,
    #[ts(type = "number")]
    pub done: u64,
    /// Smoothed over the last few seconds.
    #[ts(type = "number")]
    pub speed: u64,
    pub connections: u32,
    #[ts(type = "number | null")]
    pub eta_secs: Option<u64>,
    pub error: Option<UserError>,
    /// Why a `Queued` download is not running yet; `None` for every other status.
    pub waiting: Option<WaitReason>,
    /// Set for video and audio downloaded from a stream or a media page.
    pub media: Option<MediaSummary>,
    /// Unix time in milliseconds.
    #[ts(type = "number")]
    pub added_at: i64,
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
    pub step: ProcessingStep,
    /// How much is done, in tenths of a percent (0 to 1000), when known.
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum DownloadStatus {
    /// Waiting for its queue to have room or for the queue's schedule.
    Queued,
    Connecting,
    Downloading,
    Paused,
    Completed,
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
    pub message: String,
    pub fix: Option<FixAction>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum FixAction {
    Retry,
    /// The link expired: get a fresh one from the page, keep the downloaded bytes.
    RefreshLink,
    ChooseFolder,
    FreeDiskSpace,
    /// Converting a downloaded video failed: save it as it was downloaded instead (D24).
    KeepOriginal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum Category {
    Video,
    Audio,
    Archive,
    Document,
    Program,
    Image,
    Other,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Queue {
    pub id: QueueId,
    pub name: String,
    /// Downloads from this queue that may run at the same time.
    pub max_concurrent: u32,
    /// Bytes per second shared by the whole queue; `None` is unlimited.
    #[ts(type = "number | null")]
    pub speed_limit: Option<u64>,
    /// When set, the queue only runs inside this window.
    pub schedule: Option<Schedule>,
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
    #[ts(type = "number")]
    pub id: u64,
    pub title: String,
    pub page_url: Option<String>,
    /// Best first. The UI preselects the first one.
    pub options: Vec<MediaOption>,
    /// Where it will be saved if the user just confirms.
    pub folder: String,
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct MediaOption {
    pub id: String,
    /// Plain words, for example "1080p, MP4".
    pub label: String,
    pub height: Option<u32>,
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
    #[default]
    Original,
    Mp4,
    Mkv,
    Webm,
    Mp3,
    M4a,
    Opus,
    Flac,
    Wav,
}

/// How one quality is saved as one [`OutputFormat`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct OutputFit {
    pub format: OutputFormat,
    /// The file's extension, without the dot, for example `mp4`.
    pub ext: String,
    /// The video must be re-encoded: it takes longer and keeps the processor or graphics
    /// chip busy. `false` when the video is copied as is (seconds) or there is none.
    pub converts_video: bool,
    /// The audio must be re-encoded with the default audio quality (quick). Picking an audio
    /// quality for an audio format always re-encodes it.
    pub converts_audio: bool,
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
    pub option_id: String,
    pub folder: Option<String>,
    pub file_name: Option<String>,
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
    #[serde(default)]
    pub video: OutputDefaults,
    #[serde(default)]
    pub audio: OutputDefaults,
}

/// The starting choices for one kind of media download.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct OutputDefaults {
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

/// An optional component (D2): ffmpeg, the yt-dlp media add-on.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct AddonInfo {
    /// Stable id, for example `ffmpeg` or `yt-dlp`.
    pub id: String,
    pub name: String,
    /// One plain-language line on what it adds.
    pub description: String,
    pub state: AddonState,
    pub installed_version: Option<String>,
    pub latest_version: Option<String>,
    /// Download size of the latest version, when known.
    #[ts(type = "number | null")]
    pub size: Option<u64>,
    /// A plain sentence about updates to show with the add-on, for example
    /// that a newer version waits until it is two days old (D21.1).
    #[serde(default)]
    pub note: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "type", content = "data", rename_all = "snake_case")]
#[ts(export)]
pub enum AddonState {
    NotInstalled,
    Installing {
        #[ts(type = "number")]
        done: u64,
        #[ts(type = "number | null")]
        total: Option<u64>,
    },
    Installed,
    UpdateAvailable,
    Failed {
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
    /// Outside its queue's schedule; `starts_at` is the next start, if any.
    QueueSchedule { starts_at: Option<TimeOfDay> },
    /// Its queue already runs as many downloads as it may.
    QueueFull { running: u32, limit: u32 },
    /// The app already runs as many downloads as allowed overall.
    GlobalLimit { running: u32, limit: u32 },
    /// Ahead of it in the same queue are other waiting downloads.
    InLine { position: u32 },
    /// A video's server refused it for now; Wirecatch tries again by itself at
    /// `retry_at` (Unix time in milliseconds), keeping what was downloaded. Only
    /// on a download that is connecting, never on a queued one.
    ServerRefused {
        #[ts(type = "number")]
        retry_at: i64,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Schedule {
    pub start: TimeOfDay,
    /// `None` runs until the queue is empty.
    pub stop: Option<TimeOfDay>,
    /// Empty means every day.
    pub days: Vec<Weekday>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct TimeOfDay {
    pub hour: u8,
    pub minute: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum Weekday {
    Mon,
    Tue,
    Wed,
    Thu,
    Fri,
    Sat,
    Sun,
}

/// A path that survives moving between machines and operating systems (D6).
/// It may start with one of `{Downloads}`, `{Home}`, `{Documents}`, `{Videos}`,
/// `{Music}` or `{Pictures}`, resolved on the machine that uses it. Separators are `/`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct PortablePath(pub String);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct CategoryRule {
    pub category: Category,
    /// Lower case, without the dot.
    pub extensions: Vec<String>,
    pub folder: PortablePath,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum Theme {
    System,
    Light,
    Dark,
}

/// One change to [`AppSettings`]: the setting at `path` (field names from the top, such as `["extension", "ask_before_download"]`) becomes `value`. A window sends only what the person changed, applied to the app's current settings, so two windows changing different settings never undo each other.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct SettingsChange {
    /// Field names from the top of the settings to the one that changes. Every name must exist.
    pub path: Vec<String>,
    /// The new value, in the settings' own JSON form.
    #[ts(type = "unknown")]
    pub value: serde_json::Value,
}

/// Every user setting. The app is the only store (D5).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct AppSettings {
    pub default_folder: PortablePath,
    pub categories: Vec<CategoryRule>,
    /// Most connections per download.
    pub connections_per_download: u8,
    /// Downloads running at once across all queues.
    pub max_concurrent_downloads: u32,
    /// Bytes per second for everything together; `None` is unlimited.
    #[ts(type = "number | null")]
    pub global_speed_limit: Option<u64>,
    pub extension: ExtensionSettings,
    pub theme: Theme,
    pub start_with_system: bool,
    /// Closing the window keeps downloads running in the tray.
    pub close_to_tray: bool,
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
}

fn default_true() -> bool {
    true
}

/// The file written by settings export and read by import (D6).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct SettingsProfile {
    pub format_version: u32,
    pub app_version: String,
    #[ts(type = "number")]
    pub exported_at: i64,
    pub settings: AppSettings,
    pub queues: Vec<Queue>,
    /// Ids of installed add-ons, reinstalled on import.
    pub addons: Vec<String>,
}

/// A download added from the app itself (paste, Add dialog, command palette).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct AddRequest {
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
    #[ts(type = "number")]
    pub id: u64,
    pub url: String,
    pub file_name: String,
    /// Where it will be saved if the user just confirms.
    pub folder: String,
    pub category: Category,
    #[ts(type = "number | null")]
    pub size: Option<u64>,
    pub queue: QueueId,
    /// The page it came from, shown so the user knows what they are confirming.
    pub page_url: Option<String>,
}

/// The user's answer to a [`PendingCapture`]; absent fields keep the proposal.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct CaptureConfirmation {
    pub folder: Option<String>,
    pub file_name: Option<String>,
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
    DownloadsRemoved(Vec<DownloadId>),
    QueuesChanged(Vec<Queue>),
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
        #[ts(type = "number")]
        id: u64,
        url: String,
    },
    AddonsChanged(Vec<AddonInfo>),
    /// A capture needs an add-on that is not installed; the UI offers to install it.
    AddonRequired {
        addon: AddonInfo,
        url: String,
        /// The media that waits: a page waiting for the add-on (dismiss it with
        /// `dismiss_media_choice`), or the [`MediaChoice`] whose picked quality
        /// needs it. Shares the id space of [`ConfirmationClosed`](Self::ConfirmationClosed).
        #[serde(default)]
        #[ts(type = "number")]
        id: u64,
    },
    /// A question the UI may be showing is no longer open, so its dialog should close:
    /// a [`PendingCapture`] or [`MediaChoice`] with this id was answered (perhaps in
    /// another window), withdrawn by the browser, or replaced by another question (for
    /// example a [`MediaChoice`] that follows an add-on install). Pending captures and
    /// media choices share one id space, so the id is never ambiguous.
    ConfirmationClosed {
        #[ts(type = "number")]
        id: u64,
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
    pub queues_added: u32,
    pub queues_updated: u32,
    /// Ids of add-ons the profile lists that are not installed here. Nothing is
    /// downloaded on import; the UI may offer to install them.
    #[serde(default)]
    pub addons_to_offer: Vec<String>,
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
    /// Ids of add-ons the profile lists that are not installed here, to offer
    /// after the import. Nothing is downloaded on import.
    pub addons_to_offer: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
