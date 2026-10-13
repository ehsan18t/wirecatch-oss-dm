//! What the browser extension hands over, and the settings it can change (D5).

use std::collections::HashSet;
use std::fmt;

use serde::{Deserialize, Serialize};
use thiserror::Error;
use ts_rs::TS;

use crate::secrets::HIDDEN;
use crate::{DEFAULT_QUEUE_ID, DownloadId, QueueId};

crate::exported::exported_consts! {
    /// Most sites the list of sites to ignore holds. A patch naming more than this in one of its lists is refused before any site in it is read (D37.1).
    pub const MAX_IGNORED_SITES: usize = 1000;
    /// Longest site in the list of sites to ignore, in characters: the longest host name DNS allows.
    pub const MAX_SITE_CHARS: usize = 253;
}

/// A download captured in the browser. Its `Debug` hides the cookies, and the values of the headers that may carry a sign-in (see [`Header`]).
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct NewDownload {
    /// Chosen by the extension, unique per handover. The app treats a repeated
    /// `add_download` with the same id as the same download (idempotent), and
    /// [`Call::WithdrawDownload`](crate::wire::Call::WithdrawDownload) refers to it.
    pub handover_id: String,
    /// Address of the file (or, for [`MediaKind::Page`], of the page holding the media).
    pub url: String,
    /// Value for the `Referer` header of the app's requests.
    pub referrer: Option<String>,
    /// The page the download started from.
    pub page_url: Option<String>,
    /// Name the browser or page suggested; the server's name wins if this is absent.
    pub suggested_name: Option<String>,
    /// The MIME type the browser saw, for example `video/mp4`; tells audio from video for
    /// a direct media file.
    pub mime: Option<String>,
    /// Size in bytes the browser saw, when it knew. Shown when the app asks to confirm the
    /// download ([`PendingCapture::size`](crate::app::PendingCapture::size)), and checked
    /// against the server's answer: a different size means the app did not get the same
    /// file, and it does not take the download over.
    #[ts(type = "number | null")]
    pub size_hint: Option<u64>,
    /// Value for the `Cookie` header, so downloads behind a login keep working.
    pub cookies: Option<String>,
    /// The browser's `User-Agent`; the app sends it instead of its own.
    pub user_agent: Option<String>,
    /// Further request headers the download needs. The app drops those that belong to the
    /// HTTP layer or to its own downloader (`Host`, `Range`, the conditional headers,
    /// `Accept-Encoding`, `User-Agent`, `Proxy-*` and other hop-by-hop headers), keeps
    /// `Cookie`, `Referer` and `Authorization` once each, and when the link moves to another
    /// site sends only `Accept` and `Accept-Language` there.
    pub extra_headers: Vec<Header>,
    /// What in the browser started the capture.
    pub source: CaptureSource,
    /// Queue to add to; the default queue if absent.
    pub queue: Option<QueueId>,
    /// Set when the extension knows this is media rather than a plain file.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub media: Option<MediaHint>,
}

impl fmt::Debug for NewDownload {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Taken apart, so a field added later does not compile until it is written here too.
        let Self {
            handover_id,
            url,
            referrer,
            page_url,
            suggested_name,
            mime,
            size_hint,
            cookies,
            user_agent,
            extra_headers,
            source,
            queue,
            media,
        } = self;
        f.debug_struct("NewDownload")
            .field("handover_id", handover_id)
            .field("url", url)
            .field("referrer", referrer)
            .field("page_url", page_url)
            .field("suggested_name", suggested_name)
            .field("mime", mime)
            .field("size_hint", size_hint)
            .field("cookies", &cookies.as_ref().map(|_| HIDDEN))
            .field("user_agent", user_agent)
            .field("extra_headers", extra_headers)
            .field("source", source)
            .field("queue", queue)
            .field("media", media)
            .finish()
    }
}

/// What the extension knows about captured media.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct MediaHint {
    /// How the media is delivered, which decides how the app downloads it.
    pub kind: MediaKind,
    /// The page or video title, used for the file name.
    pub title: Option<String>,
}

/// How the media is delivered.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum MediaKind {
    /// `url` is an HLS playlist (`.m3u8`).
    Hls,
    /// `url` is a DASH manifest (`.mpd`).
    Dash,
    /// `url` is a plain video or audio file.
    Direct,
    /// `url` is the page itself; finding the media needs the media add-on.
    Page,
}

/// Headers that say nothing about the person, so they may follow a download to another site (compared in lowercase). Every other header (cookies, credentials, tokens in custom headers, the referring page's address) stays with the site the browser sent it to.
pub const HEADERS_SAFE_FOR_ANY_SITE: &[&str] = &["accept", "accept-language"];

/// One HTTP request header. Its `Debug` writes the value of a header that may carry a sign-in as `***` (see [`Header::may_carry_sign_in`]), so a log line that prints one does not show it.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Header {
    /// The header's name, for example `Authorization`.
    pub name: String,
    /// The header's value.
    pub value: String,
}

impl Header {
    /// Whether the header is one of [`HEADERS_SAFE_FOR_ANY_SITE`], whatever its case and surrounding space.
    #[must_use]
    pub fn is_safe_for_any_site(&self) -> bool {
        let name = self.name.trim();
        HEADERS_SAFE_FOR_ANY_SITE
            .iter()
            .any(|safe| name.eq_ignore_ascii_case(safe))
    }

    /// Whether the header may carry the person's sign-in: any header but `Referer` and those of [`HEADERS_SAFE_FOR_ANY_SITE`], since a site may put a token in a header of its own.
    #[must_use]
    pub fn may_carry_sign_in(&self) -> bool {
        !(self.name.trim().eq_ignore_ascii_case("referer") || self.is_safe_for_any_site())
    }
}

impl fmt::Debug for Header {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let value: &dyn fmt::Debug = if self.may_carry_sign_in() {
            &HIDDEN
        } else {
            &self.value
        };
        f.debug_struct("Header")
            .field("name", &self.name)
            .field("value", value)
            .finish()
    }
}

/// What in the browser handed a download to the app.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum CaptureSource {
    /// The browser started a download and the extension took it over.
    BrowserDownload,
    /// The person chose to download a link from the extension's item in the right-click menu.
    ContextMenu,
    /// The download button shown over a video or audio element.
    MediaButton,
}

/// The app's answer to a [`NewDownload`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct AddedDownload {
    /// Absent when the app is waiting for the user to confirm (`ask_before_download`).
    pub id: Option<DownloadId>,
    /// What the app did with it, which tells the extension whether it may erase the
    /// browser's copy yet.
    pub action: AddAction,
}

/// What the app did with a captured download.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum AddAction {
    /// The server answered the app with the file (a 2xx response) and data is
    /// flowing. Only now may the extension erase the browser's copy.
    Started,
    /// Accepted into a queue that is not running right now; the link has not been
    /// tried yet.
    Queued,
    /// The app is showing the confirmation dialog; the link has not been tried yet.
    AwaitingConfirmation,
}

/// Asks the app to forget a handover the extension gave up waiting for.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct WithdrawRequest {
    /// The [`NewDownload::handover_id`] of the capture to forget.
    pub handover_id: String,
}

/// The app's answer to a [`WithdrawRequest`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum WithdrawOutcome {
    /// The app dropped it; the browser should keep its copy.
    Withdrawn,
    /// The app never received it; the browser should keep its copy.
    NotFound,
    /// The app already reached the server; the browser should drop its copy.
    AlreadyStarted,
}

/// The subset of settings the extension popup shows. The app stores them; the
/// extension only edits them remotely and caches the last copy for display.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ExtensionSettings {
    /// Master switch: take over browser downloads at all.
    pub capture_enabled: bool,
    /// Show a window for every captured file before it starts (D23; on by default). Videos
    /// always ask for a quality, whatever this says.
    pub ask_before_download: bool,
    /// Show a download button over video and audio on web pages.
    pub show_media_button: bool,
    /// Host names whose downloads stay in the browser, for example `example.com`.
    pub ignored_sites: Vec<String>,
    /// File extensions to capture, lower case and without the dot. Empty means all.
    pub capture_extensions: Vec<String>,
    /// Files known to be smaller than this stay in the browser.
    #[ts(type = "number")]
    pub min_capture_size: u64,
    /// Queue captured downloads go to when the capture names none.
    pub default_queue: QueueId,
}

/// The settings of a fresh install, which the extension also judges by until it first hears
/// from the app.
impl Default for ExtensionSettings {
    fn default() -> Self {
        Self {
            capture_enabled: true,
            ask_before_download: true,
            show_media_button: true,
            ignored_sites: Vec::new(),
            capture_extensions: Vec::new(),
            min_capture_size: 0,
            default_queue: DEFAULT_QUEUE_ID,
        }
    }
}

/// A partial update: only fields that are `Some` change.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, optional_fields = nullable)]
pub struct ExtensionSettingsPatch {
    /// The new [`ExtensionSettings::capture_enabled`].
    pub capture_enabled: Option<bool>,
    /// The new [`ExtensionSettings::ask_before_download`].
    pub ask_before_download: Option<bool>,
    /// The new [`ExtensionSettings::show_media_button`].
    pub show_media_button: Option<bool>,
    /// Replaces the whole list. Prefer the add and remove fields, which do not
    /// overwrite an edit made at the same time from another browser.
    pub ignored_sites: Option<Vec<String>>,
    /// Hosts to add to `ignored_sites` (applied after `ignored_sites`).
    pub add_ignored_sites: Option<Vec<String>>,
    /// Hosts to remove from `ignored_sites` (applied last).
    pub remove_ignored_sites: Option<Vec<String>>,
    /// The new [`ExtensionSettings::capture_extensions`], replacing the whole list.
    pub capture_extensions: Option<Vec<String>>,
    /// The new [`ExtensionSettings::min_capture_size`], in bytes.
    #[ts(optional, type = "number | null")]
    pub min_capture_size: Option<u64>,
    /// The new [`ExtensionSettings::default_queue`].
    pub default_queue: Option<QueueId>,
}

impl ExtensionSettings {
    /// Applies a partial update; see [`ExtensionSettingsPatch`] for the order. Every site the patch names is taken the way [`normalize_site`] keeps it, so an edit matches the list whatever form it was typed in, and one that names no host is left out. Linear in the length of the lists, which [`ExtensionSettingsPatch::check_sites`] keeps short.
    pub fn apply(&mut self, patch: ExtensionSettingsPatch) {
        let ExtensionSettingsPatch {
            capture_enabled,
            ask_before_download,
            show_media_button,
            ignored_sites,
            add_ignored_sites,
            remove_ignored_sites,
            capture_extensions,
            min_capture_size,
            default_queue,
        } = patch;
        if let Some(v) = capture_enabled {
            self.capture_enabled = v;
        }
        if let Some(v) = ask_before_download {
            self.ask_before_download = v;
        }
        if let Some(v) = show_media_button {
            self.show_media_button = v;
        }
        if let Some(v) = ignored_sites {
            self.ignored_sites = normalize_sites(v);
        }
        if let Some(add) = add_ignored_sites {
            let mut kept: HashSet<String> = self.ignored_sites.iter().cloned().collect();
            for host in normalize_sites(add) {
                if kept.insert(host.clone()) {
                    self.ignored_sites.push(host);
                }
            }
        }
        if let Some(remove) = remove_ignored_sites {
            let remove: HashSet<String> = remove.iter().filter_map(|s| normalize_site(s)).collect();
            self.ignored_sites.retain(|h| !remove.contains(h));
        }
        if let Some(v) = capture_extensions {
            self.capture_extensions = v;
        }
        if let Some(v) = min_capture_size {
            self.min_capture_size = v;
        }
        if let Some(v) = default_queue {
            self.default_queue = v;
        }
    }
}

/// The host name an ignored site stands for, the way the list keeps it: the host as a browser reports it (lower case, ASCII, a Unicode name in its punycode form, an IP address in its usual form), without a scheme, user, port, path, query, leading `*.` or `.`, or trailing dot. So `https://Bücher.de/page`, `*.bücher.de`, `.bücher.de` and `xn--bcher-kva.de.` are all `xn--bcher-kva.de`, the host the browser gives for a download from that site. `None` when no host is left, or what is left is not a host name; that includes a `*` anywhere else, which no host a browser reports holds.
///
/// An entry covers its host and every subdomain of it. The extension and the desktop interface use a port of this function in `@wirecatch/protocol`, tested against the cases in this module, so a site typed anywhere is kept the same way.
#[must_use]
pub fn normalize_site(input: &str) -> Option<String> {
    read_site(input).ok().flatten()
}

/// The host [`normalize_site`] keeps for a site as a person typed it, or `None` for an entry holding nothing but white space, which names nothing.
///
/// # Errors
///
/// [`NotASite`] for an entry that names no host.
pub fn read_site(input: &str) -> Result<Option<String>, NotASite> {
    let site = input.trim();
    if site.is_empty() {
        return Ok(None);
    }
    host_of(site)
        .map(Some)
        .ok_or_else(|| NotASite(input.to_owned()))
}

/// The host in a trimmed site that is not empty (see [`normalize_site`]).
fn host_of(site: &str) -> Option<String> {
    let site = match site.split_once("://") {
        Some((scheme, rest)) if is_scheme(scheme) => rest,
        _ => site,
    };
    let site = site.split(['/', '\\', '?', '#']).next().unwrap_or_default();
    let mut site = site.rsplit_once('@').map_or(site, |(_, host)| host);
    while let Some(rest) = site.strip_prefix("*.").or_else(|| site.strip_prefix('.')) {
        site = rest;
    }
    let site = match site.rsplit_once(':') {
        Some((host, port)) if port.bytes().all(|b| b.is_ascii_digit()) => host,
        _ => site,
    };
    if site.is_empty() || site.contains(char::is_whitespace) || site.contains('*') {
        return None;
    }
    // The host parser of the URL standard, which browsers use: IDNA, IPv4 and IPv6 forms, and
    // the characters no host may hold.
    let host = url::Host::parse(site).ok()?.to_string();
    let host = host.trim_end_matches('.');
    (!host.is_empty()).then(|| host.to_owned())
}

/// Whether `text` is a URL scheme (an ASCII letter, then letters, digits, `+`, `-` or `.`), so that the `://` after it ends a scheme, not a path or query that holds another address.
fn is_scheme(text: &str) -> bool {
    let mut chars = text.chars();
    chars.next().is_some_and(|c| c.is_ascii_alphabetic())
        && chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
}

/// `list` with each entry kept the way [`normalize_site`] keeps it, in order, without the ones that name no host or name one already in the list. Linear in the length of the list.
#[must_use]
pub fn normalize_sites(list: impl IntoIterator<Item = String>) -> Vec<String> {
    let mut seen = HashSet::new();
    list.into_iter()
        .filter_map(|s| normalize_site(&s))
        .filter(|site| seen.insert(site.clone()))
        .collect()
}

/// A file type the way the lists of file types keep it (the extension's capture filter and the category rules): lower case, without white space around it or the leading `*.` or `.` a person may type, so `*.ZIP`, `.zip` and `zip` are all `zip`. `None` when nothing is left. The extension and the desktop interface use a port of this function in `@wirecatch/protocol`, tested against the cases in this module.
#[must_use]
pub fn normalize_file_type(input: &str) -> Option<String> {
    let mut kind = input.trim();
    while let Some(rest) = kind.strip_prefix("*.").or_else(|| kind.strip_prefix('.')) {
        kind = rest;
    }
    (!kind.is_empty()).then(|| kind.to_lowercase())
}

/// What a person may type as a file type, and the type each one stands for; `pnpm protocol:generate` writes them for the TypeScript port of [`normalize_file_type`] to be tested against.
#[cfg(test)]
pub(crate) fn file_type_cases() -> Vec<(&'static str, Option<&'static str>)> {
    vec![
        ("zip", Some("zip")),
        (" ZIP ", Some("zip")),
        (".zip", Some("zip")),
        ("..Zip", Some("zip")),
        ("*.MP4", Some("mp4")),
        ("*..mp4", Some("mp4")),
        (".tar.GZ", Some("tar.gz")),
        ("\u{85}7z\u{3000}", Some("7z")),
        ("", None),
        ("  ", None),
        ("*.", None),
        (".", None),
        ("ÄPK", Some("äpk")),
    ]
}

/// A site that names no host, as it was typed.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("{0:?} names no site")]
pub struct NotASite(pub String);

/// Why the sites an [`ExtensionSettingsPatch`] names are refused.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum SitesRefused {
    /// One of its lists names more sites than the list may hold.
    #[error("a list of sites names more than {MAX_IGNORED_SITES}")]
    TooMany,
    /// A site to keep names no host.
    #[error(transparent)]
    NotASite(#[from] NotASite),
}

impl ExtensionSettingsPatch {
    /// Checks the sites the patch names before it is applied, and reads none of them when a list is longer than [`MAX_IGNORED_SITES`], so the work [`ExtensionSettings::apply`] does stays small whatever a client sends. A site to keep must name a host or be blank (a blank one is left out); a site to remove that names no host removes nothing.
    ///
    /// # Errors
    ///
    /// [`SitesRefused::TooMany`] for a list longer than [`MAX_IGNORED_SITES`], and [`SitesRefused::NotASite`] for the first site to keep that names no host.
    pub fn check_sites(&self) -> Result<(), SitesRefused> {
        let lists = [
            &self.ignored_sites,
            &self.add_ignored_sites,
            &self.remove_ignored_sites,
        ];
        if lists
            .into_iter()
            .flatten()
            .any(|list| list.len() > MAX_IGNORED_SITES)
        {
            return Err(SitesRefused::TooMany);
        }
        for site in [&self.ignored_sites, &self.add_ignored_sites]
            .into_iter()
            .flatten()
            .flatten()
        {
            read_site(site)?;
        }
        Ok(())
    }
}

/// What a person may type as a site to ignore, and the host each one stands for; `pnpm
/// protocol:generate` writes them for the TypeScript port of [`normalize_site`] to be tested
/// against.
#[cfg(test)]
pub(crate) fn site_cases() -> Vec<(&'static str, Option<&'static str>)> {
    vec![
        ("example.com", Some("example.com")),
        ("  Example.COM  ", Some("example.com")),
        ("https://Example.com/page?q=1#top", Some("example.com")),
        ("http://user@example.com:8080/", Some("example.com")),
        ("*.example.com", Some("example.com")),
        ("www.example.com.", Some("www.example.com")),
        ("example.com:443", Some("example.com")),
        ("example.com:", Some("example.com")),
        ("user@example.com", Some("example.com")),
        ("exa_mple.com", Some("exa_mple.com")),
        ("bücher.de", Some("xn--bcher-kva.de")),
        ("BÜCHER.de", Some("xn--bcher-kva.de")),
        ("xn--bcher-kva.de", Some("xn--bcher-kva.de")),
        ("XN--BCHER-KVA.DE.", Some("xn--bcher-kva.de")),
        ("https://bücher.de/p", Some("xn--bcher-kva.de")),
        ("*.Bücher.de:8080", Some("xn--bcher-kva.de")),
        ("http://user:pw@bücher.de:443/x", Some("xn--bcher-kva.de")),
        ("bücher。de", Some("xn--bcher-kva.de")),
        ("例え.テスト", Some("xn--r8jz45g.xn--zckzah")),
        ("[::1]", Some("[::1]")),
        ("http://[::1]:8080/x", Some("[::1]")),
        ("[0:0::1]", Some("[::1]")),
        ("[2001:DB8::1]:443", Some("[2001:db8::1]")),
        ("0x7f.1", Some("127.0.0.1")),
        ("１２７.０.０.１", Some("127.0.0.1")),
        ("a<b.com", None),
        ("xn--zz", None),
        (r"example.com\path", Some("example.com")),
        (r"x@a.com\@b.com", Some("a.com")),
        ("example.com/", Some("example.com")),
        ("example.com/path/", Some("example.com")),
        ("example.com?x=1", Some("example.com")),
        ("example.com#x", Some("example.com")),
        ("sub.example.co.uk", Some("sub.example.co.uk")),
        ("192.168.1.10:8000", Some("192.168.1.10")),
        ("", None),
        ("   ", None),
        ("/", None),
        ("https://", None),
        ("*.", None),
        ("exa mple.com", None),
        ("example.com/a://b.com", Some("example.com")),
        ("example.com/out?to=https://other.com", Some("example.com")),
        ("x.com#http://y.com", Some("x.com")),
        (
            "google.com/url?q=https://videos.example/a",
            Some("google.com"),
        ),
        ("user@example.com/x://y.com", Some("example.com")),
        ("git+ssh://example.com/repo", Some("example.com")),
        (".example.com", Some("example.com")),
        ("..example.com", Some("example.com")),
        ("*.*.example.com", Some("example.com")),
        ("*..example.com", Some("example.com")),
        ("*", None),
        ("*example.com", None),
        ("a.*.example.com", None),
        ("**.example.com", None),
        ("exa\u{FEFF}mple.com", Some("example.com")),
        ("\u{FEFF}example.com", Some("example.com")),
        ("\u{85}example.com", Some("example.com")),
        ("exa\u{85}mple.com", None),
        ("example.com\u{3000}", Some("example.com")),
        ("exa\u{A0}mple.com", None),
    ]
}

/// Settings and the patches applied to them; `pnpm protocol:generate` writes each with what
/// [`ExtensionSettings::apply`] makes of it, for the TypeScript port to be tested against.
#[cfg(test)]
pub(crate) fn apply_cases() -> Vec<(ExtensionSettings, ExtensionSettingsPatch)> {
    let sites = |list: &[&str]| Some(list.iter().map(|s| (*s).to_owned()).collect());
    let before = ExtensionSettings {
        ignored_sites: vec!["a.com".into(), "b.com".into()],
        capture_extensions: vec!["zip".into()],
        ..ExtensionSettings::default()
    };
    let doubled = ExtensionSettings {
        ignored_sites: vec![
            "a.com".into(),
            "b.com".into(),
            "a.com".into(),
            "b.com".into(),
        ],
        ..before.clone()
    };
    vec![
        (before.clone(), ExtensionSettingsPatch::default()),
        (
            before.clone(),
            ExtensionSettingsPatch {
                capture_enabled: Some(false),
                ask_before_download: Some(false),
                show_media_button: Some(false),
                capture_extensions: sites(&["iso", "exe"]),
                min_capture_size: Some(1024),
                default_queue: Some(QueueId(3)),
                ..ExtensionSettingsPatch::default()
            },
        ),
        (
            before.clone(),
            ExtensionSettingsPatch {
                add_ignored_sites: sites(&["https://WWW.C.com/page", "*.a.com", "c.com", ""]),
                remove_ignored_sites: sites(&["B.com."]),
                ..ExtensionSettingsPatch::default()
            },
        ),
        (
            before.clone(),
            ExtensionSettingsPatch {
                ignored_sites: sites(&["D.com/", "d.com", " ", "e.com:80"]),
                add_ignored_sites: sites(&["f.com"]),
                remove_ignored_sites: sites(&["e.com", "a.com"]),
                ..ExtensionSettingsPatch::default()
            },
        ),
        (
            before,
            ExtensionSettingsPatch {
                ignored_sites: sites(&[]),
                remove_ignored_sites: sites(&["a.com"]),
                ..ExtensionSettingsPatch::default()
            },
        ),
        // A list holding the same site twice is kept as it is unless the patch replaces it.
        (doubled.clone(), ExtensionSettingsPatch::default()),
        (
            doubled.clone(),
            ExtensionSettingsPatch {
                add_ignored_sites: sites(&["b.com", "c.com"]),
                ..ExtensionSettingsPatch::default()
            },
        ),
        (
            doubled,
            ExtensionSettingsPatch {
                remove_ignored_sites: sites(&["b.com"]),
                ..ExtensionSettingsPatch::default()
            },
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn header(name: &str, value: &str) -> Header {
        Header {
            name: name.into(),
            value: value.into(),
        }
    }

    #[test]
    fn debug_hides_what_may_carry_a_sign_in() {
        for name in ["Cookie", "Authorization", "X-Auth-Token", "X-Custom"] {
            let written = format!("{:?}", header(name, "SECRET1"));
            assert_eq!(
                written,
                format!("Header {{ name: {name:?}, value: \"***\" }}")
            );
        }
        for name in ["Referer", " accept ", "Accept-Language"] {
            assert!(format!("{:?}", header(name, "shown")).contains("\"shown\""));
        }
        let download = NewDownload {
            handover_id: "h".into(),
            url: "https://example.com/f".into(),
            referrer: None,
            page_url: None,
            suggested_name: None,
            mime: None,
            size_hint: None,
            cookies: Some("sid=SECRET2".into()),
            user_agent: None,
            extra_headers: vec![header("Authorization", "Bearer SECRET3")],
            source: CaptureSource::ContextMenu,
            queue: None,
            media: None,
        };
        let written = format!("{download:?} {download:#?}");
        assert!(!written.contains("SECRET"), "{written}");
        assert!(written.contains("cookies: Some(\"***\")"), "{written}");
    }

    #[test]
    fn patch_changes_only_given_fields() {
        let mut s = ExtensionSettings::default();
        assert!(
            s.ask_before_download,
            "a window shows first by default (D23)"
        );
        s.apply(ExtensionSettingsPatch {
            ask_before_download: Some(false),
            ..Default::default()
        });
        assert!(!s.ask_before_download);
        assert!(s.capture_enabled);
    }

    #[test]
    fn site_list_edits_merge_instead_of_overwriting() {
        let mut s = ExtensionSettings {
            ignored_sites: vec!["a.com".into(), "b.com".into()],
            ..Default::default()
        };
        s.apply(ExtensionSettingsPatch {
            add_ignored_sites: Some(vec!["c.com".into(), "a.com".into()]),
            remove_ignored_sites: Some(vec!["b.com".into()]),
            ..Default::default()
        });
        assert_eq!(s.ignored_sites, vec!["a.com", "c.com"]);
    }

    #[test]
    fn a_file_type_is_kept_lower_case_without_its_leading_dot() {
        for (typed, kind) in file_type_cases() {
            assert_eq!(normalize_file_type(typed).as_deref(), kind, "{typed:?}");
        }
    }

    #[test]
    fn a_site_is_kept_as_its_bare_lower_case_host() {
        let wrong: Vec<String> = site_cases()
            .into_iter()
            .filter_map(|(typed, site)| {
                let kept = normalize_site(typed);
                (kept.as_deref() != site)
                    .then(|| format!("{typed:?} kept as {kept:?}, not {site:?}"))
            })
            .collect();
        assert!(wrong.is_empty(), "{wrong:#?}");
    }

    #[test]
    fn site_edits_are_matched_the_way_the_list_keeps_them() {
        for (before, patch) in apply_cases() {
            let mut applied = before.clone();
            applied.apply(patch.clone());
            let kept: Vec<Option<String>> = applied
                .ignored_sites
                .iter()
                .map(|s| normalize_site(s))
                .collect();
            assert!(
                applied
                    .ignored_sites
                    .iter()
                    .map(|s| Some(s.clone()))
                    .eq(kept),
                "{before:?} with {patch:?} kept {:?}",
                applied.ignored_sites
            );
        }
        let mut s = ExtensionSettings {
            ignored_sites: vec!["a.com".into(), "b.com".into()],
            ..Default::default()
        };
        s.apply(ExtensionSettingsPatch {
            add_ignored_sites: Some(vec!["https://WWW.C.com/page".into(), "*.a.com".into()]),
            remove_ignored_sites: Some(vec!["B.com.".into()]),
            ..Default::default()
        });
        assert_eq!(s.ignored_sites, vec!["a.com", "www.c.com"]);
        s.apply(ExtensionSettingsPatch {
            ignored_sites: Some(vec!["D.com/".into(), "d.com".into(), " ".into()]),
            ..Default::default()
        });
        assert_eq!(s.ignored_sites, vec!["d.com"]);
    }

    fn sites(list: &[&str]) -> Vec<String> {
        list.iter().map(|&s| s.to_owned()).collect()
    }

    #[test]
    fn a_site_to_keep_must_name_a_host_and_a_blank_one_names_nothing() {
        assert_eq!(read_site("  "), Ok(None));
        assert_eq!(read_site("Example.com/"), Ok(Some("example.com".into())));
        assert_eq!(read_site("not a site"), Err(NotASite("not a site".into())));

        let keep = ExtensionSettingsPatch {
            ignored_sites: Some(sites(&["a.com", ""])),
            add_ignored_sites: Some(sites(&["b.com", "exa mple.com"])),
            ..ExtensionSettingsPatch::default()
        };
        assert_eq!(
            keep.check_sites(),
            Err(SitesRefused::NotASite(NotASite("exa mple.com".into())))
        );
        let remove = ExtensionSettingsPatch {
            ignored_sites: Some(sites(&["a.com", " "])),
            remove_ignored_sites: Some(sites(&["*", "/"])),
            ..ExtensionSettingsPatch::default()
        };
        assert_eq!(remove.check_sites(), Ok(()));
    }

    /// A frame's worth of sites, about 80,000, is refused for its length before any is read: its first entry names no host, and the refusal is the one for the length. Lists as long as they may be are applied.
    #[test]
    fn a_frame_full_of_sites_is_refused_unread_and_full_lists_apply() {
        let many = |n: usize| -> Vec<String> { (0..n).map(|i| format!("s{i}.example")).collect() };
        let mut flood = vec!["exa mple.com".to_owned()];
        let mut written = 0;
        while written < crate::frame::MAX_FRAME_LEN {
            let site = format!("s{}.example", flood.len());
            written += serde_json::to_string(&site).unwrap().len() + ",".len();
            flood.push(site);
        }
        assert!(flood.len() > 50 * MAX_IGNORED_SITES, "{}", flood.len());
        let patch = ExtensionSettingsPatch {
            add_ignored_sites: Some(flood),
            ..ExtensionSettingsPatch::default()
        };
        assert_eq!(patch.check_sites(), Err(SitesRefused::TooMany));

        let mut settings = ExtensionSettings {
            ignored_sites: many(MAX_IGNORED_SITES),
            ..ExtensionSettings::default()
        };
        let full = ExtensionSettingsPatch {
            ignored_sites: Some(many(MAX_IGNORED_SITES)),
            add_ignored_sites: Some(many(MAX_IGNORED_SITES)),
            remove_ignored_sites: Some(many(MAX_IGNORED_SITES)),
            ..ExtensionSettingsPatch::default()
        };
        assert_eq!(full.check_sites(), Ok(()));
        settings.apply(full);
        assert_eq!(settings.ignored_sites, [] as [String; 0]);
    }
}
