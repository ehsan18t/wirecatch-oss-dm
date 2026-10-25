//! Running the media add-on's yt-dlp: reading a page's formats (`-J`) and, for
//! formats Wirecatch cannot download itself, letting yt-dlp download them.
//!
//! The browser's cookies reach yt-dlp through a Netscape cookies file bound to the page's host, readable only by the current user (it is created inside the app's private data folder and restricted again), and deleted as soon as yt-dlp exits. yt-dlp gets the add-on's Deno as its only JavaScript runtime, ignores any configuration files and plugins on the machine, and keeps its cache in the add-on folder rather than the one other copies of yt-dlp share.

#![forbid(unsafe_code)]

mod cookies;
mod download;
mod info;

pub(crate) use download::{Fetched, download};
pub(crate) use info::{Chapter, Format, FormatId, Info, SubtitleFile, Transport, ids, selection};

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::time::Duration;

use tracing::debug;
use wirecatch_engine::{CancellationToken, Url};
use wirecatch_streams::process::{self, Ran, RunError, RunOptions};

use crate::addons::MediaTools;
use cookies::CookiesFile;

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
    /// The proxy yt-dlp goes through (D71.3): a proxy URL, `""` for none, or `None` to let it
    /// read the computer's proxy itself.
    pub proxy: Option<&'a str>,
}

/// Whether the browser's cookies go to yt-dlp from the first try for `page`.
///
/// Not for YouTube: with a signed-in browser's cookies yt-dlp reads YouTube as a
/// web player whose video addresses YouTube often refuses (403) for minutes at
/// a time, while without them it reads it as another player whose addresses
/// work. Measured on 2026-10-11: with cookies, 3 of 5 fresh reads gave refused
/// addresses; without, 5 of 5 worked. So YouTube is read without the cookies
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

/// Reads a page's formats with `yt-dlp -J`: on YouTube without the browser's
/// cookies first, then with them if yt-dlp could not read the page that way.
pub(crate) async fn extract(
    ctx: &Context<'_>,
    cancel: &CancellationToken,
) -> Result<Info, YtDlpError> {
    if let Some(anonymous) = without_cookies(ctx) {
        match extract_once(&anonymous, cancel).await {
            Ok(info) => return Ok(info),
            Err(Attempt::NeedsSignIn(error)) => {
                debug!(%error, "reading the page needs the browser's sign-in; trying with it");
            }
            Err(Attempt::Failed(error)) => return Err(error),
        }
    }
    extract_once(ctx, cancel).await.map_err(Attempt::into_error)
}

/// Why yt-dlp did not do what it was asked.
#[derive(Debug, thiserror::Error)]
pub(crate) enum YtDlpError {
    /// The folder yt-dlp runs in could not be made.
    #[error("could not make the folder {} for the media add-on", .path.display())]
    Folder {
        /// The folder.
        path: PathBuf,
        /// Why.
        #[source]
        source: std::io::Error,
    },
    /// The file with the page's cookies could not be written.
    #[error("could not write the page's sign-in for the media add-on")]
    Cookies(#[source] std::io::Error),
    /// The add-on's yt-dlp could not be started.
    #[error("yt-dlp could not be started")]
    Start(#[source] std::io::Error),
    /// Reading the page was cancelled.
    #[error("reading the page was stopped")]
    Cancelled,
    /// Reading the page took longer than allowed.
    #[error("yt-dlp took longer than {} seconds to read the page", .0.as_secs())]
    TimedOut(Duration),
    /// A download by yt-dlp reported nothing for this long and was stopped.
    #[error("yt-dlp reported nothing for {} seconds", .0.as_secs())]
    Stalled(Duration),
    /// yt-dlp ran and failed; the reason is its own error line.
    #[error("yt-dlp could not get the video: {reason}")]
    Refused {
        /// What yt-dlp said, without its `ERROR:` prefix.
        reason: String,
    },
    /// yt-dlp printed something that is not a page description.
    #[error("yt-dlp printed something that is not a page description")]
    BadOutput(#[source] serde_json::Error),
    /// yt-dlp described the page in more than Wirecatch reads.
    #[error("yt-dlp described the page at a length past what Wirecatch reads")]
    OutputTooLarge,
    /// yt-dlp finished without leaving the file it was asked for.
    #[error("yt-dlp finished without saving the video")]
    NothingSaved,
    /// A task of the run panicked, a bug, or was stopped as the app closed.
    #[error("part of the media add-on's run stopped unexpectedly ({0})")]
    Task(tokio::task::JoinError),
}

impl YtDlpError {
    fn from_run(error: RunError) -> Self {
        match error {
            RunError::Start { source, .. } => YtDlpError::Start(source),
            RunError::TimedOut { limit, .. } => YtDlpError::TimedOut(limit),
            RunError::Stalled { quiet, .. } => YtDlpError::Stalled(quiet),
            RunError::Cancelled => YtDlpError::Cancelled,
            other => YtDlpError::Refused {
                reason: other.to_string(),
            },
        }
    }
}

/// Why one run of yt-dlp gave no result.
enum Attempt {
    /// yt-dlp ran and said the video needs a sign-in; another try with the browser's sign-in may work.
    NeedsSignIn(YtDlpError),
    /// It failed for another reason (the video is gone, the network is down), was stopped, timed out, could not start, or printed nonsense: the browser's sign-in would not help.
    Failed(YtDlpError),
}

/// What yt-dlp's error says when a video needs the viewer signed in: an age check, a members-only or private video, or a refusal of an anonymous request. Only these are worth a second, signed-in try, which costs seconds and sends the person's cookies to the site. yt-dlp reports causes only as text, so its text is what is matched.
const SIGN_IN_MARKERS: [&str; 9] = [
    "sign in",
    "confirm your age",
    "age-restricted",
    "age restricted",
    "members-only",
    "members only",
    "private video",
    "http error 401",
    "http error 403",
];

/// Whether yt-dlp's failure says the video needs the viewer signed in (see [`SIGN_IN_MARKERS`]).
fn needs_sign_in(error_line: Option<&str>) -> bool {
    error_line.is_some_and(|line| {
        let line = line.to_ascii_lowercase();
        SIGN_IN_MARKERS.iter().any(|marker| line.contains(marker))
    })
}

/// A run that yt-dlp refused: worth a signed-in try only when it says so.
fn refused(ran: &Ran) -> Attempt {
    refusal(ran.stderr.error_line(), || ran.reason())
}

/// The refusal yt-dlp's most telling line `said` describes, its reason cut for a person (whether it asks for a sign-in is read from the whole line); `otherwise` gives the reason when it said nothing.
fn refusal(said: Option<&str>, otherwise: impl FnOnce() -> String) -> Attempt {
    let error = YtDlpError::Refused {
        reason: said
            .map(|l| l.trim_start_matches("ERROR:").trim().trim_end_matches('.'))
            .filter(|l| !l.is_empty())
            .map_or_else(otherwise, process::for_a_person),
    };
    if needs_sign_in(said) {
        Attempt::NeedsSignIn(error)
    } else {
        Attempt::Failed(error)
    }
}

impl Attempt {
    fn into_error(self) -> YtDlpError {
        match self {
            Attempt::NeedsSignIn(e) | Attempt::Failed(e) => e,
        }
    }
}

impl From<YtDlpError> for Attempt {
    fn from(error: YtDlpError) -> Self {
        Attempt::Failed(error)
    }
}

async fn extract_once(ctx: &Context<'_>, cancel: &CancellationToken) -> Result<Info, Attempt> {
    make_dir(ctx.temp_dir).await?;
    let cookies = CookiesFile::create(ctx).await?;
    let mut args = common_args(ctx, cookies.as_ref());
    args.push("-J".into());
    args.push("--".into());
    args.push(ctx.page.as_str().into());
    let result = process::run(
        &ctx.tools.ytdlp,
        &args,
        ctx.temp_dir,
        RunOptions::collect()
            .with_timeout(ctx.timeout)
            .with_environment(proxy_environment(ctx)),
        cancel,
        |_, _| {},
    )
    .await;
    CookiesFile::delete(cookies).await;
    let ran = result.map_err(YtDlpError::from_run)?;
    if !ran.success() {
        return Err(refused(&ran));
    }
    if ran.stdout_truncated {
        return Err(YtDlpError::OutputTooLarge.into());
    }
    serde_json::from_slice::<Info>(&ran.stdout).map_err(|e| YtDlpError::BadOutput(e).into())
}

/// Makes the folder yt-dlp runs in.
async fn make_dir(path: &Path) -> Result<(), YtDlpError> {
    tokio::fs::create_dir_all(path)
        .await
        .map_err(|source| YtDlpError::Folder {
            path: path.to_path_buf(),
            source,
        })
}

/// What keeps every run of yt-dlp, the version check of a new one included, from reading the machine's own yt-dlp setup: its configuration files and its plugins.
const ISOLATION: [&str; 2] = ["--ignore-config", "--no-plugin-dirs"];

/// The arguments that make yt-dlp print its version and nothing else, read the same isolated way as every other run.
pub(crate) fn version_args() -> Vec<OsString> {
    ISOLATION
        .iter()
        .chain(&["--version"])
        .map(OsString::from)
        .collect()
}

/// The arguments every run gets.
fn common_args(ctx: &Context<'_>, cookies: Option<&CookiesFile>) -> Vec<OsString> {
    let mut args: Vec<OsString> = ISOLATION.iter().map(OsString::from).collect();
    args.extend([
        "--no-playlist".into(),
        "--no-update".into(),
        "--no-js-runtimes".into(),
        "--js-runtimes".into(),
    ]);
    let mut deno = OsString::from("deno:");
    deno.push(ctx.tools.deno.as_os_str());
    args.push(deno);
    args.push("--ffmpeg-location".into());
    args.push(ctx.tools.ffmpeg.clone().into_os_string());
    args.push("--cache-dir".into());
    args.push(ctx.tools.ytdlp_cache.clone().into_os_string());
    // No proxy is said on the command line; a proxy, which may carry a login, goes in the
    // environment ([`proxy_environment`]).
    if ctx.proxy == Some("") {
        args.push("--proxy".into());
        args.push("".into());
    }
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

/// The proxy variables yt-dlp reads when it is given no `--proxy`, set to `ctx`'s proxy and
/// with no exceptions, so the computer's own neither add to it nor go around it. A proxy's login
/// may be in its URL: the environment is readable only by the user who started yt-dlp, where a
/// command line is readable by every user of the computer.
pub(crate) fn proxy_environment(ctx: &Context<'_>) -> Vec<(OsString, Option<OsString>)> {
    let Some(proxy) = ctx.proxy.filter(|p| !p.is_empty()) else {
        return Vec::new();
    };
    let set = [
        "ALL_PROXY",
        "all_proxy",
        "HTTP_PROXY",
        "http_proxy",
        "HTTPS_PROXY",
        "https_proxy",
    ]
    .into_iter()
    .map(|name| (name.into(), Some(proxy.into())));
    let unset = ["NO_PROXY", "no_proxy"]
        .into_iter()
        .map(|name| (name.into(), None));
    set.chain(unset).collect()
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

    /// yt-dlp's cache (player code, signatures) is the add-on's own, never the one another yt-dlp on the machine writes.
    #[test]
    fn yt_dlp_keeps_its_cache_in_the_add_on_folder() {
        let tools = MediaTools::for_test(Path::new("ffmpeg"));
        let page = Url::parse("https://example.com/v").unwrap();
        let ctx = Context {
            tools: &tools,
            page: &page,
            cookies: None,
            user_agent: None,
            temp_dir: Path::new("temp"),
            timeout: Duration::from_secs(1),
            proxy: None,
        };
        let args = common_args(&ctx, None);
        let at = args.iter().position(|a| a == "--cache-dir");
        let dir = at.and_then(|at| args.get(at + 1));
        assert_eq!(
            dir,
            Some(&tools.ytdlp_cache.clone().into_os_string()),
            "{args:?}"
        );
        assert!(!args.iter().any(|a| a == "--no-cache-dir"));
    }

    /// yt-dlp follows the app's proxy, or no proxy, and reads the computer's only when the app does (D71.3).
    #[test]
    fn yt_dlp_is_given_the_apps_proxy() {
        let tools = MediaTools::for_test(Path::new("ffmpeg"));
        let page = Url::parse("https://example.com/v").unwrap();
        let proxy_arg = |proxy: Option<&str>| {
            let ctx = Context {
                tools: &tools,
                page: &page,
                cookies: None,
                user_agent: None,
                temp_dir: Path::new("temp"),
                timeout: Duration::from_secs(1),
                proxy,
            };
            let args = common_args(&ctx, None);
            args.iter()
                .position(|a| a == "--proxy")
                .map(|at| args[at + 1].clone())
        };
        assert_eq!(proxy_arg(None), None);
        assert_eq!(proxy_arg(Some("")), Some(OsString::new()));
        assert_eq!(proxy_arg(Some("socks5h://alice:secret@proxy:1080")), None);
    }

    /// A proxy, login included, reaches yt-dlp only through its environment, which other users
    /// of the computer cannot read, with the computer's exceptions taken away.
    #[test]
    fn a_proxy_goes_to_yt_dlp_in_its_environment() {
        let tools = MediaTools::for_test(Path::new("ffmpeg"));
        let page = Url::parse("https://example.com/v").unwrap();
        let environment = |proxy: Option<&str>| {
            proxy_environment(&Context {
                tools: &tools,
                page: &page,
                cookies: None,
                user_agent: None,
                temp_dir: Path::new("temp"),
                timeout: Duration::from_secs(1),
                proxy,
            })
        };
        assert_eq!(environment(None), Vec::new());
        assert_eq!(environment(Some("")), Vec::new());
        let given = environment(Some("socks5h://alice:secret@proxy:1080"));
        let value = |name: &str| {
            given
                .iter()
                .find(|(n, _)| n == name)
                .map(|(_, v)| v.clone())
        };
        for name in ["ALL_PROXY", "HTTPS_PROXY", "http_proxy"] {
            assert_eq!(
                value(name),
                Some(Some("socks5h://alice:secret@proxy:1080".into())),
                "{name}"
            );
        }
        assert_eq!(value("NO_PROXY"), Some(None));
    }

    /// The version check of a new yt-dlp is isolated like every other run.
    #[test]
    fn every_run_is_isolated_from_the_machines_yt_dlp_setup() {
        let tools = MediaTools::for_test(Path::new("ffmpeg"));
        let page = Url::parse("https://example.com/v").unwrap();
        let ctx = Context {
            tools: &tools,
            page: &page,
            cookies: None,
            user_agent: None,
            temp_dir: Path::new("temp"),
            timeout: Duration::from_secs(1),
            proxy: None,
        };
        for args in [common_args(&ctx, None), version_args()] {
            for flag in ["--ignore-config", "--no-plugin-dirs"] {
                assert!(
                    args.iter().any(|a| a == flag),
                    "{flag} missing from {args:?}"
                );
            }
        }
        assert_eq!(
            version_args().last().map(OsString::as_os_str),
            Some("--version".as_ref())
        );
    }

    /// A site controls how long yt-dlp's error line is: the reason a person reads is cut, and a sign-in asked for past the cut is still seen.
    #[test]
    fn a_long_refusal_gives_a_short_reason_and_still_asks_for_a_sign_in() {
        let said = format!(
            "ERROR: [youtube] abc: {}. Sign in to confirm your age",
            "the site explains at length ".repeat(100)
        );
        let attempt = refusal(Some(&said), String::new);
        assert!(matches!(attempt, Attempt::NeedsSignIn(_)));
        let YtDlpError::Refused { reason } = attempt.into_error() else {
            panic!("not a refusal");
        };
        assert!(
            reason.chars().count() <= process::MAX_REASON_CHARS,
            "{} characters",
            reason.chars().count()
        );
        assert!(
            reason.starts_with("[youtube] abc: the site explains"),
            "{reason}"
        );
    }

    #[test]
    fn only_a_failure_that_needs_a_sign_in_is_tried_again_with_cookies() {
        for line in [
            "ERROR: [youtube] abc: Sign in to confirm your age",
            "ERROR: [youtube] abc: Join this channel to get access to members-only content",
            "ERROR: [youtube] abc: Private video. Sign in if you've been granted access",
            "ERROR: unable to download video data: HTTP Error 403: Forbidden",
        ] {
            assert!(needs_sign_in(Some(line)), "{line}");
        }
        for line in [
            "ERROR: [youtube] abc: Video unavailable",
            "ERROR: [youtube] abc: This video has been removed by the uploader",
            "ERROR: Unsupported URL: https://example.com/",
            "ERROR: unable to download webpage: <urlopen error [Errno -2] Name or service not known>",
        ] {
            assert!(!needs_sign_in(Some(line)), "{line}");
        }
    }
}
