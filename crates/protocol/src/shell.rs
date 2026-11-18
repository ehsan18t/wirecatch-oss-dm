//! What the browser extension draws and acts on, as the app describes it (D279): the options its
//! popup shows, the values its own checks use, and the app's answer on each capture.

use std::fmt;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use ts_rs::TS;

use crate::extension::{CaptureSource, CookieRequest};
use crate::secret::{Address, OptionalAddress};

crate::exported::exported_consts! {
    /// Most groups an option list holds; the extension leaves out any past it.
    pub const MAX_OPTION_GROUPS: usize = 32;
    /// Most options one group holds; the extension leaves out any past it.
    pub const MAX_OPTIONS_PER_GROUP: usize = 64;
    /// Most choices one option offers; the extension leaves out any past it.
    pub const MAX_OPTION_CHOICES: usize = 500;
    /// Longest text of an option the extension shows, in characters; it cuts anything longer.
    pub const MAX_OPTION_TEXT_CHARS: usize = 500;
    /// How long a capture may wait for the app, in milliseconds, from the browser's event to the
    /// app's answer to `check_download`, starting the app included. Past it the download goes on
    /// in the browser, and a later acceptance starts nothing.
    pub const CAPTURE_DEADLINE_MS: u64 = 12_000;
}

/// Orders what the app sends: a new app instance always wins, and within one instance a higher
/// counter does.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Revision {
    /// New at every start of the app.
    pub instance: String,
    /// Raised by every change within the instance.
    #[ts(type = "number")]
    pub counter: u64,
}

/// The values the extension's own checks use (D279): it gates captures, private windows and
/// media watching with them before anything reaches the app.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ShellConfig {
    /// Orders this copy against others.
    pub revision: Revision,
    /// Take browser downloads at all.
    pub capture_enabled: bool,
    /// Take downloads and media from private windows (D82).
    pub private_windows: bool,
    /// Show the download button over video and audio on pages.
    pub show_media_button: bool,
    /// Hosts whose downloads stay in the browser; each covers the names under it.
    pub ignored_sites: Vec<String>,
    /// File types to capture, lower case without the dot; empty takes every file.
    pub capture_extensions: Vec<String>,
    /// Files known to be smaller than this, in bytes, stay in the browser.
    #[ts(type = "number")]
    pub min_capture_size: u64,
    /// Sites with video pages the person added (D51).
    pub video_sites: Vec<String>,
    /// How the extension tells media from other requests.
    pub media: MediaTables,
}

/// What the extension matches requests against to find video and audio. How they are applied
/// stays in its code: HLS before DASH before direct files, segments left out.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct MediaTables {
    /// Extensions of HLS manifests.
    pub hls_extensions: Vec<String>,
    /// Media types of HLS manifests.
    pub hls_types: Vec<String>,
    /// Extensions of DASH manifests.
    pub dash_extensions: Vec<String>,
    /// Media types of DASH manifests.
    pub dash_types: Vec<String>,
    /// Extensions of whole video files.
    pub video_extensions: Vec<String>,
    /// Extensions of whole audio files.
    pub audio_extensions: Vec<String>,
    /// Extensions of stream segments, never listed.
    pub segment_extensions: Vec<String>,
    /// Media types of stream segments, never listed.
    pub segment_types: Vec<String>,
    /// Whole files known to be smaller than this, in bytes, are not listed: pages play short
    /// interface sounds as media.
    #[ts(type = "number")]
    pub min_direct_bytes: u64,
}

fn strings(list: &[&str]) -> Vec<String> {
    list.iter().map(|&s| s.to_owned()).collect()
}

/// The tables the extension ships with, for the time before the app first answers.
impl Default for MediaTables {
    fn default() -> Self {
        Self {
            hls_extensions: strings(&["m3u8"]),
            hls_types: strings(&[
                "application/vnd.apple.mpegurl",
                "application/x-mpegurl",
                "audio/mpegurl",
                "audio/x-mpegurl",
            ]),
            dash_extensions: strings(&["mpd"]),
            dash_types: strings(&["application/dash+xml", "video/vnd.mpeg.dash.mpd"]),
            video_extensions: strings(&[
                "mp4", "m4v", "webm", "mkv", "mov", "ogv", "avi", "flv", "3gp",
            ]),
            audio_extensions: strings(&[
                "mp3", "m4a", "ogg", "oga", "opus", "wav", "flac", "aac", "weba",
            ]),
            segment_extensions: strings(&[
                "ts", "m4s", "m4f", "m4v-seg", "cmfv", "cmfa", "aac-seg",
            ]),
            segment_types: strings(&["video/mp2t", "video/iso.segment", "audio/iso.segment"]),
            min_direct_bytes: 100 * 1024,
        }
    }
}

/// The options the popup shows, in groups (D279). Each option travels as raw JSON so an
/// extension skips one it cannot read and still shows the rest; [`OptionSpec`] is its shape.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct OptionList {
    /// Orders this list against others.
    pub revision: Revision,
    /// The groups, in the order the popup shows them.
    pub groups: Vec<OptionGroup>,
}

/// One fold of the popup's options.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct OptionGroup {
    /// Stable across lists, so an open fold stays open.
    pub id: String,
    /// The fold's title; `None` for a card that does not fold.
    pub title: Option<String>,
    /// What the options are set to, shown while folded.
    pub summary: Option<String>,
    /// Shown folded when the popup opens; only a group with a title folds.
    pub folded: bool,
    /// Each an [`OptionSpec`] as JSON.
    #[ts(type = "Array<unknown>")]
    pub options: Vec<Value>,
}

/// The shape of one option on the wire. `kind` is open: an extension draws a kind it does not
/// know as its label and an "Open in Wirecatch" button.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct OptionSpec {
    /// Stable and opaque: the extension only sends it back.
    pub id: String,
    /// Which control the extension draws.
    pub kind: &'static str,
    /// The control's name.
    pub label: String,
    /// One line under the name, or nothing.
    pub help: Option<String>,
    /// Why it cannot be changed now; `None` when it can.
    pub disabled: Option<String>,
    /// The current value: a boolean, a number, a string, a list of them, or `null`.
    pub value: Value,
    /// The kind's own fields.
    #[serde(flatten)]
    pub extra: serde_json::Map<String, Value>,
}

/// One choice of a `choice` or `choices` option.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OptionChoice {
    /// What `set_option` sends back.
    pub value: Value,
    /// What the person reads.
    pub label: String,
    /// Other words that find it in a search.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub keywords: Option<String>,
}

/// The control kinds this build of the app sends. An extension older than a kind shows its
/// fallback for it.
pub mod kinds {
    /// On or off; the value is a boolean.
    pub const TOGGLE: &str = "toggle";
    /// One of `choices`.
    pub const CHOICE: &str = "choice";
    /// Several of `choices`; the value is a list.
    pub const CHOICES: &str = "choices";
    /// A number shown in `unit`: the value divided by `scale`, with `decimals`, between `min`
    /// and `max`, stepping by `step`.
    pub const NUMBER: &str = "number";
    /// A switch with a [`NUMBER`] under it; `null` is off, meaning no limit.
    pub const LIMIT: &str = "limit";
    /// A button that runs the app's action named by the option's id.
    pub const ACTION: &str = "action";
    /// A line of text with no control.
    pub const NOTE: &str = "note";
    /// A row the extension fills itself, named by `browser` (see [`super::BROWSER_ROWS`]).
    pub const BROWSER: &str = "browser";
}

/// The rows a `browser` option may name, each filled by the extension: the keyboard shortcut,
/// and the switch that leaves the tab's site in the browser ([`PageOptions`]).
pub const BROWSER_ROWS: &[&str] = &["shortcut", "page_site"];

/// A change to one option ([`SetOption`]).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum OptionChange {
    /// The new value.
    Set(#[ts(type = "unknown")] Value),
    /// For a `choices` option: values to add and to remove, so an edit made at the same time in
    /// another browser is kept.
    Edit {
        /// Values to add.
        #[ts(type = "Array<unknown>")]
        add: Vec<Value>,
        /// Values to remove, applied after `add`.
        #[ts(type = "Array<unknown>")]
        remove: Vec<Value>,
    },
}

/// Changes one option ([`crate::wire::Call::SetOption`]). Always applied: the last change wins.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct SetOption {
    /// The option's id.
    pub id: String,
    /// What to change.
    pub change: OptionChange,
}

/// Runs an action ([`crate::wire::Call::InvokeAction`]).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct InvokeAction {
    /// The action's id: an `action` option's id, or one of [`actions`].
    pub id: String,
    /// The host of the tab's page, for an action about the page.
    pub site: Option<String>,
}

/// The actions the extension's own design runs, besides the `action` options the app sends.
pub mod actions {
    /// Leave the downloads of `site` in the browser.
    pub const IGNORE_SITE: &str = "ignore_site";
    /// Take the downloads of `site` again, removing the entry that ignores it.
    pub const TAKE_SITE: &str = "take_site";
}

/// Asks about the tab's page ([`crate::wire::Call::PageOptions`]).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct PageQuery {
    /// The page's host name, never its address.
    pub host: String,
}

/// What the popup shows about the tab's page.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct PageOptions {
    /// The site to name: the entry that ignores the page, or the page's own site.
    pub site: String,
    /// Whether the page's downloads stay in the browser.
    pub ignored: bool,
}

/// What the app needs to decide on a capture, without its cookies
/// ([`crate::wire::Call::CheckDownload`]). Its `Debug` hides any sign-in in its addresses.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct DownloadCheck {
    /// The address of what would be downloaded.
    pub url: String,
    /// The address of the page it comes from, when known.
    pub page_url: Option<String>,
    /// The file name the browser chose, when known.
    pub file_name: Option<String>,
    /// The media type the server sent, when known.
    pub mime: Option<String>,
    /// The size in bytes, when known.
    #[ts(type = "number | null")]
    pub size: Option<u64>,
    /// How the capture started.
    pub source: CaptureSource,
    /// The private session it comes from, `None` for a normal window.
    pub private_session: Option<String>,
    /// The request's context, from which the app picks the cookies to read.
    pub cookie_request: CookieRequest,
}

impl fmt::Debug for DownloadCheck {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let Self {
            url,
            page_url,
            file_name,
            mime,
            size,
            source,
            private_session,
            cookie_request,
        } = self;
        f.debug_struct("DownloadCheck")
            .field("url", &Address(url))
            .field("page_url", &OptionalAddress(page_url.as_deref()))
            .field("file_name", file_name)
            .field("mime", mime)
            .field("size", size)
            .field("source", source)
            .field("private_session", &private_session.is_some())
            .field("cookie_request", cookie_request)
            .finish()
    }
}

/// The app's answer to [`DownloadCheck`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "decision", rename_all = "snake_case")]
#[ts(export)]
pub enum CheckAnswer {
    /// Hand it over, with these cookies.
    Take {
        /// Which cookies the extension reads for it.
        cookies: CookiePlan,
    },
    /// Leave it in the browser.
    Leave {
        /// Why, for the extension's log; never shown.
        reason: String,
    },
}

/// Which cookies the extension reads for a capture: values its fixed reading code uses, so it
/// needs no list of public suffixes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct CookiePlan {
    /// The site of the capture's address, with its scheme.
    pub site: Option<String>,
    /// The site of the top-level page, with its scheme: the partition to read, and Firefox's
    /// first-party domain.
    pub top_level_site: Option<String>,
    /// The browser would count the request as cross-site: media goes with the `Cookie` header its
    /// own request carried, when that was seen (D112.4).
    pub cross_site: bool,
}

/// Says a private session ended ([`crate::wire::Call::PrivateSessionEnded`]).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct PrivateSessionEnded {
    /// The session's id.
    pub id: String,
}

/// The control kinds, browser rows and actions, written into the generated TypeScript so the
/// extension names them from this one list.
#[cfg(test)]
pub(crate) fn name_tables() -> Vec<crate::exported::Constant> {
    use crate::exported::Constant;
    let kinds = serde_json::json!({
        "toggle": kinds::TOGGLE,
        "choice": kinds::CHOICE,
        "choices": kinds::CHOICES,
        "number": kinds::NUMBER,
        "limit": kinds::LIMIT,
        "action": kinds::ACTION,
        "note": kinds::NOTE,
        "browser": kinds::BROWSER,
    });
    let actions = serde_json::json!({
        "ignore_site": actions::IGNORE_SITE,
        "take_site": actions::TAKE_SITE,
    });
    vec![
        Constant::json(
            "OPTION_KINDS",
            &[" The control kinds an option may name (`shell::kinds`, D279)."],
            "Readonly<Record<string, string>>",
            &[],
            &kinds,
        ),
        Constant::json(
            "BROWSER_ROWS",
            &[" The rows a `browser` option may name (`shell::BROWSER_ROWS`)."],
            "readonly string[]",
            &[],
            &BROWSER_ROWS,
        ),
        Constant::json(
            "SHELL_ACTIONS",
            &[" The actions the extension's own design runs (`shell::actions`)."],
            "Readonly<Record<string, string>>",
            &[],
            &actions,
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_option_is_written_with_its_kind_fields_beside_the_common_ones() {
        let mut extra = serde_json::Map::new();
        extra.insert("unit".into(), "MB/s".into());
        let option = OptionSpec {
            id: "speed".into(),
            kind: kinds::LIMIT,
            label: "Global speed limit".into(),
            help: None,
            disabled: None,
            value: Value::Null,
            extra,
        };
        assert_eq!(
            serde_json::to_value(option).unwrap(),
            serde_json::json!({
                "id": "speed", "kind": "limit", "label": "Global speed limit",
                "help": null, "disabled": null, "value": null, "unit": "MB/s"
            })
        );
    }

    #[test]
    fn a_choices_edit_is_written_as_what_to_add_and_remove() {
        let change = OptionChange::Edit {
            add: vec!["de".into()],
            remove: vec![],
        };
        assert_eq!(
            serde_json::to_value(change).unwrap(),
            serde_json::json!({"edit": {"add": ["de"], "remove": []}})
        );
        assert_eq!(
            serde_json::to_value(OptionChange::Set(true.into())).unwrap(),
            serde_json::json!({"set": true})
        );
    }

    #[test]
    fn a_check_answer_names_its_decision() {
        let take = CheckAnswer::Take {
            cookies: CookiePlan {
                site: Some("https://example.com".into()),
                top_level_site: None,
                cross_site: false,
            },
        };
        assert_eq!(serde_json::to_value(take).unwrap()["decision"], "take");
    }
}
