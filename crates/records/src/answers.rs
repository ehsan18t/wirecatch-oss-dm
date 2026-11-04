//! What has been decided about a download and must hold after a restart.

use serde::{Deserialize, Serialize};

/// What a download does about the file already in its folder, once decided (D88).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OnExisting {
    /// Its run finishes over the file at its path.
    Replace,
    /// Its path is the next free name.
    KeepBoth,
}

/// What the browser saw of a captured download, so the app can tell when its own
/// request got something else (another size, or a web page instead of the file).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Expectation {
    /// The size the browser reported, if any.
    pub size: Option<u64>,
    /// The browser's name for the file, which says whether a web page is expected.
    pub name: Option<String>,
}

/// Decisions a download's next run acts on.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Answers {
    pub on_existing: Option<OnExisting>,
    pub expect: Option<Expectation>,
}
