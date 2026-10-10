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
    /// Unix time in milliseconds.
    #[ts(type = "number")]
    pub added_at: i64,
    #[ts(type = "number | null")]
    pub completed_at: Option<i64>,
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
}

/// What importing a settings profile changed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ImportSummary {
    /// Every folder setting in the profile and the folder it became on this machine.
    pub folders: Vec<ImportedFolder>,
    pub queues_added: u32,
    pub queues_updated: u32,
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
}
