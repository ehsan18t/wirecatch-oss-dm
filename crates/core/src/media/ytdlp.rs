//! Running the media add-on's yt-dlp: reading a page's formats (`-J`) and, for
//! formats Wirecatch cannot download itself, letting yt-dlp download them.
//!
//! The browser's cookies reach yt-dlp through a Netscape cookies file bound to
//! the page's host, readable only by the current user (it is created inside the
//! app's private data folder and restricted again), and deleted as soon as
//! yt-dlp exits. yt-dlp gets the add-on's Deno as its only JavaScript runtime
//! and ignores any configuration files on the machine.

use std::collections::HashMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use serde::Deserialize;
use tracing::{debug, warn};
use wirecatch_engine::numeric::f64_to_u64;
use wirecatch_engine::{CancellationToken, Url};
use wirecatch_protocol::app::{FixAction, UserError};

use super::MediaProgress;
use super::process::{self, Output, Ran, RunError};
use crate::addons::MediaTools;

/// What yt-dlp says about a page (the fields Wirecatch reads).
#[derive(Debug, Clone, Default, Deserialize)]
pub(crate) struct Info {
    #[serde(default)]
    pub title: Option<String>,
    /// The video's id on its site, so two links to one video are known to be the same.
    #[serde(default)]
    pub id: Option<String>,
    /// The extractor that read the page, for example `Youtube`; ids are unique per extractor.
    #[serde(default)]
    pub extractor_key: Option<String>,
    #[serde(default)]
    pub is_live: Option<bool>,
    #[serde(default)]
    pub live_status: Option<String>,
    #[serde(default)]
    pub formats: Vec<Format>,
    /// For a page with one format, yt-dlp may describe it at the top level.
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub ext: Option<String>,
    #[serde(default)]
    pub format_id: Option<String>,
    /// Length in seconds.
    #[serde(default, deserialize_with = "lenient")]
    pub duration: Option<f64>,
    /// The picture yt-dlp names as the video's thumbnail.
    #[serde(default, deserialize_with = "lenient")]
    pub thumbnail: Option<String>,
    #[serde(default, deserialize_with = "lenient")]
    pub uploader: Option<String>,
    #[serde(default, deserialize_with = "lenient")]
    pub channel: Option<String>,
    /// For music, the performer.
    #[serde(default, deserialize_with = "lenient")]
    pub artist: Option<String>,
    /// For music, the track's title.
    #[serde(default, deserialize_with = "lenient")]
    pub track: Option<String>,
    /// Subtitles written by people, by language.
    #[serde(default, deserialize_with = "lenient")]
    pub subtitles: HashMap<String, Vec<SubtitleFile>>,
    /// Subtitles made by speech recognition (and machine translations of them), by language.
    #[serde(default, deserialize_with = "lenient")]
    pub automatic_captions: HashMap<String, Vec<SubtitleFile>>,
    #[serde(default, deserialize_with = "lenient")]
    pub chapters: Vec<Chapter>,
}

/// Reads a field that yt-dlp may leave out, set to `null` or fill with something
/// unexpected: anything that does not read as `T` becomes `T`'s default, so one odd
/// field never makes a whole page unreadable.
fn lenient<'de, D, T>(deserializer: D) -> Result<T, D::Error>
where
    D: serde::Deserializer<'de>,
    T: serde::de::DeserializeOwned + Default,
{
    let value = serde_json::Value::deserialize(deserializer)?;
    Ok(serde_json::from_value(value).unwrap_or_default())
}

/// One file of a subtitle language, in one of the formats the site offers.
#[derive(Debug, Clone, Default, Deserialize)]
pub(crate) struct SubtitleFile {
    #[serde(default, deserialize_with = "lenient")]
    pub ext: Option<String>,
    #[serde(default, deserialize_with = "lenient")]
    pub url: Option<String>,
    /// The language in words, for example "English" or "English (Original)".
    #[serde(default, deserialize_with = "lenient")]
    pub name: Option<String>,
    #[serde(default, deserialize_with = "lenient")]
    pub http_headers: HashMap<String, String>,
}

/// One chapter of a video.
#[derive(Debug, Clone, Default, Deserialize)]
pub(crate) struct Chapter {
    #[serde(default, deserialize_with = "lenient")]
    pub start_time: Option<f64>,
    #[serde(default, deserialize_with = "lenient")]
    pub end_time: Option<f64>,
    #[serde(default, deserialize_with = "lenient")]
    pub title: Option<String>,
}

/// One format of a page.
#[derive(Debug, Clone, Default, Deserialize)]
pub(crate) struct Format {
    /// yt-dlp's `format_id`, which names the format in a `-f` selection.
    #[serde(rename = "format_id")]
    pub id: String,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub protocol: Option<String>,
    #[serde(default)]
    pub ext: Option<String>,
    #[serde(default)]
    pub vcodec: Option<String>,
    #[serde(default)]
    pub acodec: Option<String>,
    #[serde(default)]
    pub height: Option<u32>,
    #[serde(default)]
    pub fps: Option<f64>,
    #[serde(default)]
    pub tbr: Option<f64>,
    #[serde(default)]
    pub abr: Option<f64>,
    /// Video bit rate in kbps.
    #[serde(default, deserialize_with = "lenient")]
    pub vbr: Option<f64>,
    #[serde(default)]
    pub filesize: Option<u64>,
    #[serde(default)]
    pub filesize_approx: Option<f64>,
    #[serde(default)]
    pub http_headers: HashMap<String, String>,
    /// `true`, `false` or `"maybe"`.
    #[serde(default)]
    pub has_drm: Option<serde_json::Value>,
    /// How yt-dlp itself would download the format; see
    /// [`request_limit`](Self::request_limit).
    #[serde(default)]
    pub downloader_options: Option<DownloaderOptions>,
}

/// The part of yt-dlp's `downloader_options` Wirecatch reads.
#[derive(Debug, Clone, Default, Deserialize)]
pub(crate) struct DownloaderOptions {
    /// Kept as any JSON value, so an odd one is ignored instead of making the
    /// whole page unreadable.
    #[serde(default)]
    pub http_chunk_size: Option<serde_json::Value>,
}

/// The smallest `http_chunk_size` taken from yt-dlp. Anything smaller would
/// mean many thousands of requests for one video, so it is not a real limit.
const MIN_REQUEST_LIMIT: u64 = 64 * 1024;

/// The largest request limit used. A larger `http_chunk_size` is cut to this:
/// asking for less than a server allows is always fine.
const MAX_REQUEST_LIMIT: u64 = 1024 * 1024 * 1024;

impl Format {
    pub fn has_video(&self) -> bool {
        self.vcodec.as_deref().is_none_or(|c| c != "none")
            && !matches!(self.ext.as_deref(), Some("mhtml"))
    }

    pub fn has_audio(&self) -> bool {
        self.acodec.as_deref().is_none_or(|c| c != "none")
    }

    pub fn drm(&self) -> bool {
        matches!(self.has_drm, Some(serde_json::Value::Bool(true)))
    }

    /// The size in bytes, exact or estimated, when known.
    pub fn size(&self) -> Option<u64> {
        self.filesize.or_else(|| {
            self.filesize_approx
                .filter(|s| *s > 0.0)
                .and_then(f64_to_u64)
        })
    }

    /// The most bytes one request for this format should ask for, from yt-dlp's
    /// `http_chunk_size`: some servers (`YouTube`'s among them) slow down any
    /// request for more, which is why yt-dlp itself downloads them in pieces.
    /// `None` when yt-dlp gave none, or a value that is not a whole number of
    /// at least 64 KiB; values above 1 GiB are cut to 1 GiB.
    pub fn request_limit(&self) -> Option<u64> {
        let value = self.downloader_options.as_ref()?.http_chunk_size.as_ref()?;
        let bytes = value.as_u64().or_else(|| {
            value
                .as_f64()
                .filter(|f| f.is_finite() && f.fract() == 0.0 && *f >= 0.0)
                // Beyond `u64`: as large as can be, which the cut below brings down.
                .map(|f| f64_to_u64(f).unwrap_or(u64::MAX))
        })?;
        (bytes >= MIN_REQUEST_LIMIT).then(|| bytes.min(MAX_REQUEST_LIMIT))
    }

    /// Whether Wirecatch downloads this format itself: plain web addresses with
    /// the engine, HLS playlists with `wirecatch-streams`.
    pub fn transport(&self) -> Transport {
        match self.protocol.as_deref().unwrap_or("https") {
            "http" | "https" => Transport::File,
            "m3u8" | "m3u8_native" => Transport::Hls,
            _ => Transport::YtDlp,
        }
    }
}

/// Who downloads a format's bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Transport {
    File,
    Hls,
    YtDlp,
}

impl Info {
    pub fn is_live(&self) -> bool {
        self.is_live == Some(true)
            || matches!(self.live_status.as_deref(), Some("is_live" | "is_upcoming"))
    }

    /// The formats, including a single top-level one.
    pub fn all_formats(&self) -> Vec<Format> {
        if !self.formats.is_empty() {
            return self.formats.clone();
        }
        match &self.url {
            Some(url) => vec![Format {
                id: self.format_id.clone().unwrap_or_else(|| "0".into()),
                url: Some(url.clone()),
                ext: self.ext.clone(),
                ..Format::default()
            }],
            None => Vec::new(),
        }
    }
}

/// Where and how yt-dlp runs.
#[derive(Clone, Copy)]
pub(crate) struct Context<'a> {
    pub tools: &'a MediaTools,
    pub page: &'a Url,
    /// The browser's `Cookie` header for the page, if any.
    pub cookies: Option<&'a str>,
    pub user_agent: Option<&'a str>,
    /// A private folder for the cookies file.
    pub temp_dir: &'a Path,
    pub timeout: Duration,
}

/// Whether the browser's cookies go to yt-dlp from the first try for `page`.
///
/// Not for `YouTube`: with a signed-in browser's cookies yt-dlp reads `YouTube` as a
/// web player whose video addresses `YouTube` often refuses (403) for minutes at
/// a time, while without them it reads it as another player whose addresses
/// work. Measured on 2026-10-11: with cookies, 3 of 5 fresh reads gave refused
/// addresses; without, 5 of 5 worked. So `YouTube` is read without the cookies
/// first, and with them only when that fails (age-restricted, members-only or
/// private videos need the sign-in).
pub(crate) fn cookies_first(page: &Url) -> bool {
    let Some(host) = page.host_str() else {
        return true;
    };
    let host = host.trim_end_matches('.').to_ascii_lowercase();
    let youtube = ["youtube.com", "youtu.be", "youtube-nocookie.com"]
        .iter()
        .any(|site| host == *site || host.ends_with(&format!(".{site}")));
    !youtube
}

/// The same context without the browser's cookies, when the page is read
/// without them first (see [`cookies_first`]); `None` otherwise.
fn without_cookies<'a>(ctx: &Context<'a>) -> Option<Context<'a>> {
    (ctx.cookies.is_some() && !cookies_first(ctx.page)).then_some(Context {
        cookies: None,
        ..*ctx
    })
}

/// Reads a page's formats with `yt-dlp -J`: on `YouTube` without the browser's
/// cookies first, then with them if yt-dlp could not read the page that way.
pub(crate) async fn extract(
    ctx: &Context<'_>,
    cancel: &CancellationToken,
) -> Result<Info, UserError> {
    if let Some(anonymous) = without_cookies(ctx) {
        match extract_once(&anonymous, cancel).await {
            Ok(info) => return Ok(info),
            Err(Attempt::Refused(error)) => {
                debug!(error = %error.message, "reading the page without the browser's sign-in failed; trying with it");
            }
            Err(Attempt::Other(error)) => return Err(error),
        }
    }
    extract_once(ctx, cancel).await.map_err(Attempt::into_error)
}

/// Why one run of yt-dlp gave no result.
enum Attempt {
    /// yt-dlp ran and said it could not do it; another try with the browser's
    /// sign-in may work.
    Refused(UserError),
    /// Stopped, timed out, could not start, or printed nonsense.
    Other(UserError),
}

impl Attempt {
    fn into_error(self) -> UserError {
        match self {
            Attempt::Refused(e) | Attempt::Other(e) => e,
        }
    }
}

impl From<UserError> for Attempt {
    fn from(error: UserError) -> Self {
        Attempt::Other(error)
    }
}

async fn extract_once(ctx: &Context<'_>, cancel: &CancellationToken) -> Result<Info, Attempt> {
    let cookies = CookiesFile::create(ctx)?;
    let mut args = common_args(ctx, cookies.as_ref());
    args.push("-J".into());
    args.push("--".into());
    args.push(ctx.page.as_str().into());
    if let Err(e) = std::fs::create_dir_all(ctx.temp_dir) {
        return Err(Attempt::Other(fix(format!(
            "Wirecatch could not prepare a folder for the media add-on ({e}). Check that the disk has free space, then try again."
        ))));
    }
    let result = process::run(
        &ctx.tools.ytdlp,
        &args,
        ctx.temp_dir,
        true,
        Some(ctx.timeout),
        cancel,
        |_, _| {},
    )
    .await;
    drop(cookies);
    let ran = match result {
        Ok(ran) => ran,
        Err(RunError::Cancelled) => return Err(stopped().into()),
        Err(RunError::TimedOut) => {
            return Err(Attempt::Other(fix(format!(
                "The media add-on took more than {} seconds to read this page. Check your connection, then try again.",
                ctx.timeout.as_secs()
            ))));
        }
        Err(RunError::Start(e)) => {
            ctx.tools.mark_broken();
            return Err(cannot_start(&e).into());
        }
    };
    if !ran.success {
        return Err(Attempt::Refused(failed(&ran)));
    }
    serde_json::from_slice::<Info>(&ran.stdout).map_err(|e| {
        warn!(error = %e, "yt-dlp printed something that is not a page description");
        Attempt::Other(fix("The media add-on could not read this page. Update the media add-on in Settings, Add-ons, then try again.".into()))
    })
}

/// How a download by yt-dlp itself ended without failing.
#[derive(Debug)]
pub(crate) enum Fetched {
    Done(PathBuf),
    Cancelled,
}

/// Prefix of the progress lines asked of yt-dlp.
const PROGRESS_MARK: &str = "wcprogress";

/// Lets yt-dlp download format selection `format` into `work` (for streams
/// Wirecatch cannot download itself), reporting progress into `progress`.
///
/// On `YouTube` it runs without the browser's cookies first, like [`extract`],
/// and with them only if yt-dlp failed that way; the parts it saved are kept.
pub(crate) async fn download(
    ctx: &Context<'_>,
    format: &str,
    work: &Path,
    progress: &MediaProgress,
    cancel: &CancellationToken,
) -> Result<Fetched, UserError> {
    if let Some(anonymous) = without_cookies(ctx) {
        match download_once(&anonymous, format, work, progress, cancel).await {
            Ok(fetched) => return Ok(fetched),
            Err(Attempt::Refused(error)) => {
                debug!(error = %error.message, "downloading without the browser's sign-in failed; trying with it");
            }
            Err(Attempt::Other(error)) => return Err(error),
        }
    }
    download_once(ctx, format, work, progress, cancel)
        .await
        .map_err(Attempt::into_error)
}

async fn download_once(
    ctx: &Context<'_>,
    format: &str,
    work: &Path,
    progress: &MediaProgress,
    cancel: &CancellationToken,
) -> Result<Fetched, Attempt> {
    let cookies = CookiesFile::create(ctx)?;
    let args = download_args(ctx, cookies.as_ref(), format, work);
    let mut printed = None::<PathBuf>;
    let result = process::run(
        &ctx.tools.ytdlp,
        &args,
        work,
        false,
        None,
        cancel,
        |output, line| {
            if let Some(report) = line
                .trim()
                .strip_prefix(PROGRESS_MARK)
                .filter(|_| output == Output::Stdout)
            {
                if let Some((done, total)) = progress_report(report) {
                    progress.set_own(done, total);
                }
            } else if let Some(path) = printed_output(output, line, work) {
                printed = Some(path);
            }
        },
    )
    .await;
    drop(cookies);
    let ran = match result {
        Ok(ran) => ran,
        Err(RunError::Cancelled) => return Ok(Fetched::Cancelled),
        Err(RunError::TimedOut) => {
            return Err(fix("The media add-on stopped answering. Try again.".into()).into());
        }
        Err(RunError::Start(e)) => {
            ctx.tools.mark_broken();
            return Err(cannot_start(&e).into());
        }
    };
    if !ran.success {
        return Err(Attempt::Refused(failed(&ran)));
    }
    let work = work.to_path_buf();
    let found = tokio::task::spawn_blocking(move || {
        printed
            .and_then(|p| confirmed_output(&p, &work))
            .or_else(|| largest_result(&work))
    })
    .await
    .unwrap_or(None);
    match found {
        Some(path) => Ok(Fetched::Done(path)),
        None => {
            Err(fix("The media add-on finished without saving the video. Try again.".into()).into())
        }
    }
}

/// yt-dlp's arguments to download `format` of the page into `work` as
/// `media.<ext>`, reporting progress and printing the finished file's path.
fn download_args(
    ctx: &Context<'_>,
    cookies: Option<&CookiesFile>,
    format: &str,
    work: &Path,
) -> Vec<OsString> {
    let mut args = common_args(ctx, cookies);
    for arg in [
        "-f",
        format,
        "--no-simulate",
        "--newline",
        "--progress",
        "--progress-template",
        "download:wcprogress %(progress.downloaded_bytes)s %(progress.total_bytes)s %(progress.total_bytes_estimate)s",
        "--print",
        "after_move:filepath",
        "-o",
    ] {
        args.push(arg.into());
    }
    args.push(work.join("media.%(ext)s").into_os_string());
    args.push("--".into());
    args.push(ctx.page.as_str().into());
    args
}

/// The bytes done and, when known, the total (exact, else estimated) from a
/// progress report yt-dlp printed after [`PROGRESS_MARK`]. Byte counts are
/// read as yt-dlp prints them; a report whose bytes done are not a count is
/// ignored rather than read as zero.
fn progress_report(report: &str) -> Option<(u64, Option<u64>)> {
    let mut numbers = report
        .split_whitespace()
        .map(|n| n.parse::<f64>().ok().and_then(f64_to_u64));
    let (done, total, estimate) = (
        numbers.next().flatten()?,
        numbers.next().flatten(),
        numbers.next().flatten(),
    );
    Some((done, total.or(estimate).filter(|t| *t > 0)))
}

/// The path of the finished file in a line yt-dlp printed (`--print
/// after_move:filepath`): from standard output only (a site's text in a
/// warning on standard error is never taken for it), a full path straight
/// inside `work` named `media.<ext>`, as the output template asks, with no
/// `..` in it. Anything else is not taken for the result.
fn printed_output(output: Output, line: &str, work: &Path) -> Option<PathBuf> {
    if output != Output::Stdout {
        return None;
    }
    let path = PathBuf::from(line.trim());
    let plain = path.is_absolute()
        && !path
            .components()
            .any(|c| matches!(c, std::path::Component::ParentDir));
    (plain && path.parent() == Some(work) && is_result_name(&path)).then_some(path)
}

/// Whether `path`'s file name is `media.<ext>`, the name the output template
/// gives the finished file (`ext` being letters and digits only, so no partial
/// or fragment file).
fn is_result_name(path: &Path) -> bool {
    path.file_name()
        .and_then(|n| n.to_str())
        .and_then(|n| n.strip_prefix("media."))
        .is_some_and(|ext| {
            (1..=10).contains(&ext.len()) && ext.bytes().all(|b| b.is_ascii_alphanumeric())
        })
}

/// `path` if it really is a file straight inside `work`, once links and the
/// like are resolved on both. Blocks on the disk.
fn confirmed_output(path: &Path, work: &Path) -> Option<PathBuf> {
    let real = std::fs::canonicalize(path).ok()?;
    let work = std::fs::canonicalize(work).ok()?;
    (real.parent() == Some(work.as_path()) && is_result_name(&real) && real.is_file())
        .then(|| path.to_path_buf())
}

/// The largest finished file yt-dlp left in `work` (not a partial one), checked
/// like a printed path. Blocks on the disk.
fn largest_result(work: &Path) -> Option<PathBuf> {
    std::fs::read_dir(work)
        .ok()?
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| is_result_name(p))
        .filter_map(|p| confirmed_output(&p, work))
        .filter_map(|p| Some((std::fs::metadata(&p).ok()?.len(), p)))
        .max_by_key(|(len, _)| *len)
        .map(|(_, p)| p)
}

/// The arguments every run gets.
fn common_args(ctx: &Context<'_>, cookies: Option<&CookiesFile>) -> Vec<OsString> {
    let mut args: Vec<OsString> = vec![
        "--ignore-config".into(),
        "--no-plugin-dirs".into(),
        "--no-playlist".into(),
        "--no-update".into(),
        "--no-js-runtimes".into(),
        "--js-runtimes".into(),
    ];
    let mut deno = OsString::from("deno:");
    deno.push(ctx.tools.deno.as_os_str());
    args.push(deno);
    args.push("--ffmpeg-location".into());
    args.push(ctx.tools.ffmpeg.clone().into_os_string());
    if let Some(cookies) = cookies {
        args.push("--cookies".into());
        args.push(cookies.path.clone().into_os_string());
    }
    if let Some(agent) = ctx
        .user_agent
        .filter(|a| !a.trim().is_empty() && !a.contains(['\r', '\n']))
    {
        args.push("--add-headers".into());
        args.push(format!("User-Agent:{agent}").into());
    }
    args
}

/// A Netscape cookies file holding the browser's cookies for the page, deleted
/// when dropped.
struct CookiesFile {
    path: PathBuf,
}

static COOKIE_FILES: AtomicU64 = AtomicU64::new(0);

impl CookiesFile {
    /// `None` when there are no cookies to pass.
    fn create(ctx: &Context<'_>) -> Result<Option<Self>, UserError> {
        let Some(text) = ctx.cookies.and_then(|c| netscape(ctx.page, c)) else {
            return Ok(None);
        };
        let write = || -> std::io::Result<PathBuf> {
            std::fs::create_dir_all(ctx.temp_dir)?;
            let n = COOKIE_FILES.fetch_add(1, Ordering::Relaxed);
            let path = ctx
                .temp_dir
                .join(format!("cookies-{}-{n}.txt", std::process::id()));
            let mut options = std::fs::File::options();
            options.write(true).create_new(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            let mut file = options.open(&path)?;
            crate::owner_only::restrict(&path)?;
            std::io::Write::write_all(&mut file, text.as_bytes())?;
            Ok(path)
        };
        match write() {
            Ok(path) => Ok(Some(Self { path })),
            Err(e) => Err(fix(format!(
                "Wirecatch could not prepare the page's sign-in for the media add-on ({e}). Check that the disk has free space, then try again."
            ))),
        }
    }
}

impl Drop for CookiesFile {
    fn drop(&mut self) {
        if let Err(e) = std::fs::remove_file(&self.path) {
            warn!(path = %self.path.display(), error = %e, "could not delete a temporary cookies file");
        }
    }
}

/// The `Cookie` header `header` as a Netscape cookies file for the page's host
/// only (not its other subdomains), or `None` when it holds no cookies.
pub(crate) fn netscape(page: &Url, header: &str) -> Option<String> {
    let host = page.host_str()?;
    let secure = if page.scheme() == "https" {
        "TRUE"
    } else {
        "FALSE"
    };
    // A day from now: long enough for any download, and yt-dlp skips cookies
    // without an expiry time when it reads the file.
    let expires = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
        + 24 * 60 * 60;
    // Line by line, joined at the end.
    let mut lines = vec!["# Netscape HTTP Cookie File".to_string()];
    for pair in header.split(';') {
        let Some((name, value)) = pair.split_once('=') else {
            continue;
        };
        let (name, value) = (name.trim(), value.trim());
        let clean = |s: &str| !s.is_empty() && !s.contains(['\t', '\r', '\n']);
        if !clean(name) || value.contains(['\t', '\r', '\n']) {
            continue;
        }
        lines.push(format!(
            "{host}\tFALSE\t/\t{secure}\t{expires}\t{name}\t{value}"
        ));
    }
    // The header line alone holds no cookies.
    (lines.len() > 1).then(|| {
        let mut text = lines.join("\n");
        text.push('\n');
        text
    })
}

fn fix(message: String) -> UserError {
    UserError {
        message,
        fix: Some(FixAction::Retry),
    }
}

fn stopped() -> UserError {
    fix("Reading the page was stopped. Try again.".into())
}

fn cannot_start(error: &std::io::Error) -> UserError {
    fix(format!(
        "The media add-on could not be started ({error}). Reinstall it in Settings, Add-ons, then try again."
    ))
}

/// yt-dlp's own reason, in a sentence that says what to do.
fn failed(ran: &Ran) -> UserError {
    let reason = ran
        .error_line()
        .map(|l| l.trim_start_matches("ERROR:").trim().trim_end_matches('.'))
        .filter(|l| !l.is_empty())
        .unwrap_or("it gave no reason");
    fix(format!(
        "The media add-on could not get the video from this page ({reason}). Check that the video plays in your browser; if it does, update the media add-on in Settings, Add-ons, then try again."
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn youtube_is_read_without_the_browser_sign_in_first() {
        for page in [
            "https://www.youtube.com/watch?v=Y_nLA7iE2Uk",
            "https://youtube.com/shorts/abc",
            "https://m.youtube.com/watch?v=1",
            "https://music.youtube.com/watch?v=1",
            "https://youtu.be/Y_nLA7iE2Uk",
            "https://www.youtube-nocookie.com/embed/1",
            "https://WWW.YOUTUBE.COM./watch?v=1",
        ] {
            assert!(!cookies_first(&Url::parse(page).unwrap()), "{page}");
        }
        for page in [
            "https://vimeo.com/1",
            "https://notyoutube.com/watch?v=1",
            "https://youtube.com.example.org/watch?v=1",
            "http://127.0.0.1:8080/page",
        ] {
            assert!(cookies_first(&Url::parse(page).unwrap()), "{page}");
        }
    }

    #[test]
    fn the_result_path_is_taken_only_from_stdout_and_only_inside_the_work_folder() {
        let dir = tempfile::tempdir().unwrap();
        let work = dir.path().join("clip.mp4.wcmedia");
        std::fs::create_dir_all(&work).unwrap();
        let good = work.join("media.mp4");
        let line = good.to_string_lossy().into_owned();
        assert_eq!(
            printed_output(Output::Stdout, &line, &work),
            Some(good.clone())
        );
        // Standard error never names the result.
        assert_eq!(printed_output(Output::Stderr, &line, &work), None);
        let outside = dir.path().join("media.mp4");
        let climbing = work.join("..").join("media.mp4");
        let nested = work.join("sub").join("media.mp4");
        for bad in [
            outside.to_string_lossy().into_owned(),
            climbing.to_string_lossy().into_owned(),
            nested.to_string_lossy().into_owned(),
            work.join("media.mp4.part").to_string_lossy().into_owned(),
            work.join("media.f137.mp4").to_string_lossy().into_owned(),
            work.join("secret.txt").to_string_lossy().into_owned(),
            "media.mp4".to_string(),
            "WARNING: see https://example.com".to_string(),
        ] {
            assert_eq!(printed_output(Output::Stdout, &bad, &work), None, "{bad}");
        }
    }

    #[test]
    fn a_result_must_really_be_a_file_in_the_work_folder() {
        let dir = tempfile::tempdir().unwrap();
        let work = dir.path().join("work");
        std::fs::create_dir_all(&work).unwrap();
        // Not there yet.
        assert_eq!(confirmed_output(&work.join("media.mp4"), &work), None);
        std::fs::write(work.join("media.mp4"), b"video").unwrap();
        std::fs::write(work.join("media.mp4.part"), b"partial, but larger").unwrap();
        assert_eq!(
            confirmed_output(&work.join("media.mp4"), &work),
            Some(work.join("media.mp4"))
        );
        assert_eq!(largest_result(&work), Some(work.join("media.mp4")));
        // A folder by the right name is not a file.
        std::fs::create_dir(work.join("media.mkv")).unwrap();
        assert_eq!(confirmed_output(&work.join("media.mkv"), &work), None);
    }

    /// A link named like the result that points elsewhere is refused, whether
    /// printed or found by looking in the folder.
    #[cfg(unix)]
    #[test]
    fn a_link_out_of_the_work_folder_is_not_the_result() {
        let dir = tempfile::tempdir().unwrap();
        let work = dir.path().join("work");
        std::fs::create_dir_all(&work).unwrap();
        let secret = dir.path().join("secret.mp4");
        std::fs::write(&secret, b"someone else's file").unwrap();
        std::os::unix::fs::symlink(&secret, work.join("media.mp4")).unwrap();
        assert_eq!(confirmed_output(&work.join("media.mp4"), &work), None);
        assert_eq!(largest_result(&work), None);
    }

    #[test]
    fn cookies_are_bound_to_the_page_host() {
        let page = Url::parse("https://www.example.com/watch?v=1").unwrap();
        let text = netscape(&page, "a=1; b = two words;broken; c=x\ty").unwrap();
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines[0], "# Netscape HTTP Cookie File");
        assert_eq!(lines.len(), 3);
        let fields: Vec<&str> = lines[1].split('\t').collect();
        assert_eq!(fields[..4], ["www.example.com", "FALSE", "/", "TRUE"]);
        assert_eq!(fields[5..], ["a", "1"]);
        assert!(lines[2].ends_with("\tb\ttwo words"));
        assert!(netscape(&page, "junk").is_none());
    }

    #[test]
    fn formats_are_read_from_yt_dlp_json() {
        let json = r#"{"title":"T","formats":[
            {"format_id":"140","ext":"m4a","vcodec":"none","acodec":"mp4a.40.2","protocol":"https","url":"https://a/140","filesize":10},
            {"format_id":"137","ext":"mp4","vcodec":"avc1","acodec":"none","height":1080,"protocol":"https","url":"https://a/137","has_drm":false},
            {"format_id":"hls-1","ext":"mp4","protocol":"m3u8_native","url":"https://a/x.m3u8","has_drm":"maybe"},
            {"format_id":"sb0","ext":"mhtml","vcodec":"none","acodec":"none","protocol":"mhtml"}
        ],"is_live":false}"#;
        let info: Info = serde_json::from_str(json).unwrap();
        assert!(!info.is_live());
        let f = &info.formats;
        assert!(f[0].has_audio() && !f[0].has_video());
        assert!(f[1].has_video() && !f[1].has_audio());
        assert_eq!(f[1].transport(), Transport::File);
        assert_eq!(f[2].transport(), Transport::Hls);
        assert!(!f[2].drm());
        assert_eq!(f[3].transport(), Transport::YtDlp);
        assert!(!f[3].has_video() && !f[3].has_audio());
        assert!(f.iter().all(|f| f.request_limit().is_none()));
    }

    #[test]
    fn the_request_limit_comes_from_http_chunk_size() {
        let limit = |value: serde_json::Value| {
            let format: Format = serde_json::from_value(serde_json::json!({
                "format_id": "137",
                "downloader_options": {"http_chunk_size": value},
            }))
            .unwrap();
            format.request_limit()
        };
        // What yt-dlp reports for every YouTube format.
        assert_eq!(limit(10_485_760.into()), Some(10_485_760));
        assert_eq!(limit(serde_json::json!(10_485_760.0)), Some(10_485_760));
        assert_eq!(limit((64 * 1024).into()), Some(64 * 1024));
        // Too large: cut to 1 GiB.
        assert_eq!(limit((5u64 << 30).into()), Some(1 << 30));
        assert_eq!(limit(serde_json::json!(1e300)), Some(1 << 30));
        // Zero, negative, tiny, fractional or not a number: no limit, and the
        // page is still readable.
        for odd in [
            serde_json::json!(0),
            serde_json::json!(-10_485_760),
            serde_json::json!(4096),
            serde_json::json!(10_485_760.5),
            serde_json::json!("10485760"),
            serde_json::json!(null),
            serde_json::json!({"x": 1}),
        ] {
            assert_eq!(limit(odd.clone()), None, "{odd}");
        }
        // No `downloader_options` at all, or no chunk size in it.
        let plain: Format = serde_json::from_str(r#"{"format_id":"1"}"#).unwrap();
        assert_eq!(plain.request_limit(), None);
        let other: Format =
            serde_json::from_str(r#"{"format_id":"1","downloader_options":{"x":1}}"#).unwrap();
        assert_eq!(other.request_limit(), None);
    }

    /// An estimated size is used only when it is a byte count: one too large
    /// for any file is unknown, not the largest number there is.
    #[test]
    fn an_estimated_size_that_is_not_a_count_is_unknown() {
        let size = |approx: serde_json::Value| {
            let format: Format = serde_json::from_value(serde_json::json!({
                "format_id": "18",
                "filesize_approx": approx,
            }))
            .unwrap();
            format.size()
        };
        assert_eq!(size(serde_json::json!(1_048_576.7)), Some(1_048_576));
        assert_eq!(size(serde_json::json!(1e30)), None);
        assert_eq!(size(serde_json::json!(-5.0)), None);
        assert_eq!(size(serde_json::json!(0.0)), None);
    }

    /// Progress reports give the bytes done and the total, exact or else
    /// estimated; a report whose bytes done are not a count is ignored.
    #[test]
    fn progress_reports_are_read_as_counts() {
        assert_eq!(progress_report("10 20 NA"), Some((10, Some(20))));
        assert_eq!(progress_report("10 NA 30.0"), Some((10, Some(30))));
        assert_eq!(progress_report("10 NA NA"), Some((10, None)));
        assert_eq!(progress_report("10 0 0"), Some((10, None)));
        assert_eq!(progress_report("-5 20 NA"), None);
        assert_eq!(progress_report("NA 20 NA"), None);
        assert_eq!(progress_report(""), None);
    }

    /// A page description as yt-dlp prints it for `YouTube`: every format has
    /// `downloader_options.http_chunk_size`, and the whole page still reads
    /// when one format has an odd value there.
    #[test]
    fn a_youtube_page_gives_every_format_its_chunk_size() {
        let json = r#"{"title":"T","formats":[
            {"format_id":"140","ext":"m4a","vcodec":"none","acodec":"mp4a.40.2","protocol":"https",
             "url":"https://rr1.googlevideo.example/videoplayback?itag=140","downloader_options":{"http_chunk_size":10485760}},
            {"format_id":"137","ext":"mp4","vcodec":"avc1.640028","acodec":"none","protocol":"https",
             "url":"https://rr1.googlevideo.example/videoplayback?itag=137","downloader_options":{"http_chunk_size":10485760}},
            {"format_id":"18","ext":"mp4","protocol":"https","url":"https://a/18","downloader_options":{"http_chunk_size":"big"}}
        ]}"#;
        let info: Info = serde_json::from_str(json).unwrap();
        let limits: Vec<Option<u64>> = info.formats.iter().map(Format::request_limit).collect();
        assert_eq!(limits, [Some(10_485_760), Some(10_485_760), None]);
    }
}
