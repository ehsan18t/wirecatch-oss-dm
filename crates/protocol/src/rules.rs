//! How a queue adds new downloads: manually, or automatically by its rules (D72.2, D72.6, D72.10). The core alone matches rules (`wirecatch-queues`); the windows send rules and show the answers.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::QueueId;
use crate::app::Category;

crate::exported::exported_consts! {
    /// Most rules of one kind in one queue.
    pub const MAX_RULES_PER_KIND: usize = 200;
    /// Most characters in one rule as the person writes it.
    pub const MAX_RULE_CHARS: usize = 256;
}

/// Whether a queue holds only the downloads the person adds to it, or also new downloads its rules match.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[ts(export)]
pub enum AddDownloads {
    #[default]
    Manual,
    Auto(QueueRules),
}

/// An auto queue's rules, by kind; a new download goes to the first rule that applies, in the order of the fields (D72.3, D72.6).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct QueueRules {
    /// Off, the queue rules nothing and keeps its rules for when it is on again (D72.4).
    pub enabled: bool,
    /// `example.com` (and the names under it), `*.example.com` (only those under it), `10.*`, `10.0.0.0/8`, `fd00::/8` or `<local>` (D72.10).
    pub sites: Vec<String>,
    /// Written as [`QueueRules::file_name_syntax`] says.
    pub file_names: Vec<String>,
    pub file_name_syntax: NameSyntax,
    /// Extensions without the dot, in lower case (`iso`, `tar.gz`).
    pub file_types: Vec<String>,
    /// The categories Settings sorts files into by their extensions.
    pub categories: Vec<Category>,
    pub protocols: Vec<LinkProtocol>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum NameSyntax {
    /// `*` for any characters and `?` for one, matching the whole name, ignoring case.
    #[default]
    Wildcards,
    /// A regular expression found anywhere in the name, ignoring case unless it says otherwise. The engine never backtracks, so no pattern can take long.
    Regex,
}

/// A link's protocol, as a rule names it; FTP covers FTPS.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum LinkProtocol {
    Http,
    Https,
    Ftp,
}

/// Why a download is in its queue (D72.3, D72.8).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "kind", content = "rule", rename_all = "snake_case")]
#[ts(export)]
pub enum QueueReason {
    /// The person picked the queue, when adding the download or by moving it.
    Picked,
    Matched(Rule),
    /// No queue matched it, and it came from the browser, whose downloads go to this queue.
    BrowserQueue,
    /// No queue matched it, it came from no browser, or its queue was deleted.
    #[default]
    DefaultQueue,
}

/// The one rule that sent a download to its queue, as the person wrote it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
#[ts(export)]
pub enum Rule {
    Site(String),
    FileName(String),
    FileType(String),
    Category(Category),
    Protocol(LinkProtocol),
}

/// A link to try against the rules, for Try a link (D72.5, D72.7).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct LinkTrial {
    pub url: String,
    /// One queue's rules as its editor has them, tried in its place before they are saved; `None` tries the saved rules.
    pub draft: Option<DraftRules>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct DraftRules {
    /// `None` for a queue not made yet, tried after every saved queue.
    pub queue: Option<QueueId>,
    pub add_downloads: AddDownloads,
}

/// What the rules make of a link: the rule that matches it, and a switched-off queue that would have matched it instead. A queue of `None` is the queue being made.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct LinkMatch {
    pub matched: Option<RuleMatch>,
    pub switched_off: Option<RuleMatch>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct RuleMatch {
    pub queue: Option<QueueId>,
    pub rule: Rule,
}
