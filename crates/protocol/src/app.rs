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
