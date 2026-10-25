//! `wirecatch`, the command-line downloader built on the Wirecatch engine.
//!
//! Exit status: 0 when the file is complete, 1 on an error, 2 for a usage mistake (from clap) and 130 when the download was paused or stopped with Ctrl+C. Running the same command again continues a paused download.

use std::io::{self, Write as _};
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, bail};
use clap::Parser;
use indicatif::{HumanBytes, ProgressBar, ProgressStyle};
use wirecatch_engine::header::{HeaderName, HeaderValue};
use wirecatch_engine::{
    CancellationToken, DownloadOptions, DownloadRequest, ManualProxies, Outcome, Progress,
    ProxyChoice, Url, download,
};

/// Which lines are logged unless the log filter variable chooses others (`wirecatch-logging`): only problems.
const LOG_FILTER: &str = "warn";
/// Upper bound for `--connections`: the engine's limit, in the type the flag is read as.
const MAX_CONNECTIONS: u16 = 64;
const _: () = assert!(
    MAX_CONNECTIONS as usize == wirecatch_engine::MAX_CONNECTIONS,
    "the flag allows what the engine allows"
);
/// How often the progress bar redraws.
const PROGRESS_REFRESH: Duration = Duration::from_millis(100);
/// Exit status for a download stopped by Ctrl+C, the shell convention for SIGINT (128 + 2).
const EXIT_INTERRUPTED: u8 = 130;

const STYLE_KNOWN_SIZE: &str =
    "{msg} [{bar:30}] {bytes}/{total_bytes}  {binary_bytes_per_sec}  ETA {eta}  {prefix}";
const STYLE_UNKNOWN_SIZE: &str = "{spinner} {msg} {bytes}  {binary_bytes_per_sec}  {prefix}";
const PROGRESS_CHARS: &str = "=> ";

/// Fast, resumable downloads over parallel connections.
///
/// Run the same command again after an interruption to continue where it stopped.
#[derive(Parser, Debug)]
#[command(name = wirecatch_product::CLI_EXECUTABLE, version)]
struct Args {
    /// The URL to download: http, https, ftp, ftps or ftpes.
    ///
    /// FTP links may carry a login, as in `ftp://user:password@host/path`; without one the download logs in anonymously. `ftps://` is encrypted from the start (port 990), or with AUTH TLS on port 21; `ftpes://` always uses AUTH TLS. An encrypted link never falls back to an unencrypted connection.
    url: String,

    /// Folder to save into.
    #[arg(short = 'o', long, default_value = ".")]
    output_dir: PathBuf,

    /// Save under this file name instead of the one the server suggests.
    #[arg(short = 'n', long)]
    name: Option<String>,

    /// Most parallel connections.
    #[arg(
        short = 'c',
        long,
        default_value_t = default_connections(),
        value_parser = clap::value_parser!(u16).range(1..=i64::from(MAX_CONNECTIONS)),
    )]
    connections: u16,

    /// Extra request header, for example -H "Cookie: a=b". Repeatable. HTTP and HTTPS only.
    ///
    /// Cookies, Authorization and other credentials are only sent to the URL's own site, never to another site it redirects to.
    #[arg(short = 'H', long = "header")]
    headers: Vec<String>,

    /// Use HTTP/3 when the server offers it (experimental).
    ///
    /// The other connections switch to HTTP/3 when the server's first answer advertises it, and switch back to HTTP/2 or HTTP/1.1 on any problem. Needs a wirecatch built with the http3 feature.
    #[arg(long)]
    http3: bool,

    /// Send every request through this proxy instead of the one the environment or the system sets.
    ///
    /// `http://`, `https://`, `socks5://` or `socks5h://` (the proxy resolves names), with a login as `user:password@` when it asks for one. Hosts named in `NO_PROXY` are still reached directly. FTP links go only through a SOCKS5 proxy.
    #[arg(long, value_name = "URL", conflicts_with = "no_proxy")]
    proxy: Option<String>,

    /// Use no proxy, whatever the environment or the system sets.
    #[arg(long)]
    no_proxy: bool,

    /// Print no progress bar.
    #[arg(short, long)]
    quiet: bool,
}

/// The engine's own default, so the CLI and the library never disagree, kept inside the range `--connections` accepts.
fn default_connections() -> u16 {
    u16::try_from(DownloadOptions::default().connections)
        .unwrap_or(MAX_CONNECTIONS)
        .clamp(1, MAX_CONNECTIONS)
}

#[tokio::main]
async fn main() -> anyhow::Result<ExitCode> {
    let logging = wirecatch_logging::start(wirecatch_product::CLI_EXECUTABLE, LOG_FILTER);
    logging.write_to(wirecatch_logging::release_folder().as_deref());

    let args = Args::parse();
    let quiet = args.quiet;
    let request = build_request(args)?;

    let progress = Arc::new(Progress::default());
    let cancel = CancellationToken::new();
    let ctrl_c = tokio::spawn(stop_on_ctrl_c(cancel.clone()));

    let bar = (!quiet)
        .then(|| spawn_progress_bar(Arc::clone(&progress)))
        .transpose()?;
    let result = download(request, progress, cancel).await;
    ctrl_c.abort();
    if let Some((bar, task)) = bar {
        task.abort();
        bar.finish_and_clear();
    }

    let (ending, summary) = summary(&result?);
    let written = writeln!(io::stdout().lock(), "{summary}");
    Ok(exit_code(ending, written))
}

/// The exit status once the summary was written, or failed to be. A reader that went away
/// (`wirecatch URL | head -0`, a closed pipe) is not an error of the download: the status still
/// says how it ended. Any other failure to write is reported on stderr and exits 1, since the
/// result never reached the reader.
fn exit_code(ending: Ending, written: io::Result<()>) -> ExitCode {
    match written {
        Ok(()) => ending.exit_code(),
        Err(e) if e.kind() == io::ErrorKind::BrokenPipe => ending.exit_code(),
        Err(e) => {
            eprintln!("wirecatch: could not write the result to standard output: {e}");
            ExitCode::FAILURE
        }
    }
}

/// Turns the command line into an engine request, refusing combinations the engine cannot honour.
fn build_request(args: Args) -> anyhow::Result<DownloadRequest> {
    let url = Url::parse(&args.url).with_context(|| refused_link(&args.url))?;
    let is_ftp = match url.scheme() {
        "http" | "https" => false,
        "ftp" | "ftps" | "ftpes" => true,
        other => bail!(
            "wirecatch cannot download \"{other}\" links. Use an http, https, ftp, ftps or ftpes link"
        ),
    };
    if is_ftp && !args.headers.is_empty() {
        bail!(
            "request headers (-H) only apply to http and https links. For an FTP login, put it in the link: ftp://user:password@host/path"
        );
    }
    if args.http3 && !cfg!(feature = "http3") {
        bail!(
            "this wirecatch was built without HTTP/3 support. Build it with `cargo build -p wirecatch-cli --features http3`, or leave out --http3"
        );
    }

    let mut request = DownloadRequest::new(url, args.output_dir);
    request.options.proxy = proxy_choice(args.proxy.as_deref(), args.no_proxy, |name| {
        std::env::var(name).ok()
    })?;
    request.file_name = args.name;
    request.options.connections = args.connections.into();
    request.options.http3 = args.http3;
    for raw in &args.headers {
        let (name, value) = parse_header(raw)?;
        request.options.headers.append(name, value);
    }
    Ok(request)
}

/// The proxies `--proxy` or `--no-proxy` ask for, or the environment's and the system's when
/// neither is given. `env` reads `NO_PROXY`, whose hosts `--proxy` leaves alone.
fn proxy_choice(
    proxy: Option<&str>,
    no_proxy: bool,
    env: impl Fn(&str) -> Option<String>,
) -> anyhow::Result<ProxyChoice> {
    if no_proxy {
        return Ok(ProxyChoice::Direct);
    }
    let Some(proxy) = proxy.map(str::trim) else {
        return Ok(ProxyChoice::System);
    };
    let readable = if proxy.contains("://") {
        Url::parse(proxy)
    } else {
        Url::parse(&format!("http://{proxy}"))
    };
    let scheme_known = readable.as_ref().is_ok_and(|url| {
        url.has_host()
            && ["http", "https", "socks4", "socks4a", "socks5", "socks5h"].contains(&url.scheme())
    });
    if !scheme_known {
        bail!(
            "--proxy is not a proxy URL; use http://, https://, socks5:// or socks5h:// with a host, as in socks5h://proxy.example:1080"
        );
    }
    let mut manual = ManualProxies::default();
    manual.all = Some(proxy.to_string());
    manual.exceptions = env("NO_PROXY")
        .or_else(|| env("no_proxy"))
        .unwrap_or_default();
    Ok(ProxyChoice::Manual(manual))
}

/// Why a link was refused. The link is repeated only when it cannot hold a password
/// (`user:password@host`), since error text reaches the terminal and bug reports.
fn refused_link(raw: &str) -> String {
    if raw.contains('@') {
        "not a valid URL (it is not repeated here, since it may hold a password)".into()
    } else {
        format!("not a valid URL: {raw}")
    }
}

/// Parses one `-H "Name: value"` argument. Errors name the header but never repeat its
/// value, which is often a password or a token.
fn parse_header(raw: &str) -> anyhow::Result<(HeaderName, HeaderValue)> {
    let (name, value) = raw
        .split_once(':')
        .context("a -H header must look like 'Name: value'")?;
    let name = name.trim();
    let header = HeaderName::from_bytes(name.as_bytes()).ok().context(
        "a -H header name may hold only letters, digits and the characters !#$%&'*+-.^_`|~",
    )?;
    let value = HeaderValue::from_str(value.trim()).map_err(|_| {
        anyhow::anyhow!(
            "bad value for header {name}: it may hold only visible characters, spaces and tabs"
        )
    })?;
    Ok((header, value))
}

/// The first Ctrl+C pauses (the engine saves its place); a second one quits at once.
async fn stop_on_ctrl_c(cancel: CancellationToken) {
    if tokio::signal::ctrl_c().await.is_err() {
        return;
    }
    eprintln!(
        "
Pausing... (press Ctrl+C again to quit immediately)"
    );
    cancel.cancel();
    if tokio::signal::ctrl_c().await.is_ok() {
        std::process::exit(i32::from(EXIT_INTERRUPTED));
    }
}

/// How a download ended, as far as a script running `wirecatch` is concerned.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Ending {
    /// The file is complete.
    Saved,
    /// The download stopped before the end; running the same command continues it.
    Stopped,
}

impl Ending {
    /// The exit status that tells a script the same thing.
    fn exit_code(self) -> ExitCode {
        ExitCode::from(self.status())
    }

    fn status(self) -> u8 {
        match self {
            Ending::Saved => 0,
            Ending::Stopped => EXIT_INTERRUPTED,
        }
    }
}

/// How the download ended, and the line that says so on standard output.
fn summary(outcome: &Outcome) -> (Ending, String) {
    match outcome {
        Outcome::Completed {
            path,
            bytes,
            resumed_from,
            ..
        } => {
            let kept = if *resumed_from > 0 {
                format!(" ({} kept from earlier)", HumanBytes(*resumed_from))
            } else {
                String::new()
            };
            let line = format!("Saved {} to {}{kept}", HumanBytes(*bytes), path.display());
            (Ending::Saved, line)
        }
        Outcome::Paused {
            path, remaining, ..
        } => {
            let line = format!(
                "Paused with {} left for {}. Run the same command again to continue.",
                HumanBytes(*remaining),
                path.display()
            );
            (Ending::Stopped, line)
        }
        _ => (
            Ending::Stopped,
            "Wirecatch stopped before the download finished. Run the same command again to continue."
                .to_owned(),
        ),
    }
}

/// Draws a progress bar from the shared counters until the returned task is aborted.
fn spawn_progress_bar(
    progress: Arc<Progress>,
) -> anyhow::Result<(ProgressBar, tokio::task::JoinHandle<()>)> {
    let known = ProgressStyle::with_template(STYLE_KNOWN_SIZE)?.progress_chars(PROGRESS_CHARS);
    let unknown = ProgressStyle::with_template(STYLE_UNKNOWN_SIZE)?;
    let bar = ProgressBar::new_spinner();
    bar.set_style(unknown);

    let task = {
        let bar = bar.clone();
        tokio::spawn(async move {
            let mut has_total = false;
            let mut ticks = tokio::time::interval(PROGRESS_REFRESH);
            loop {
                ticks.tick().await;
                if let Some(name) = progress.file_name() {
                    bar.set_message(name);
                }
                if let Some(total) = progress.total().filter(|_| !has_total) {
                    bar.set_style(known.clone());
                    bar.set_length(total);
                    has_total = true;
                }
                bar.set_position(progress.done());
                let connections = progress.connections();
                let plural = if connections == 1 { "" } else { "s" };
                bar.set_prefix(format!("{connections} conn{plural}"));
            }
        })
    };
    Ok((bar, task))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_connections_match_the_engine() {
        assert_eq!(
            usize::from(default_connections()),
            DownloadOptions::default().connections
        );
    }

    #[test]
    fn header_argument_is_split_and_trimmed() {
        let (name, value) = parse_header("Cookie:  a=b ").unwrap();
        assert_eq!(name, "cookie");
        assert_eq!(value, "a=b");
    }

    #[test]
    fn the_proxy_flags_choose_the_engines_proxies() {
        let no_env = |_: &str| None;
        assert_eq!(
            proxy_choice(None, false, no_env).unwrap(),
            ProxyChoice::System
        );
        assert_eq!(
            proxy_choice(None, true, no_env).unwrap(),
            ProxyChoice::Direct
        );
        let with_exceptions =
            |name: &str| (name == "no_proxy").then(|| "files.example".to_string());
        let ProxyChoice::Manual(manual) =
            proxy_choice(Some("socks5h://u:p@proxy:1080"), false, with_exceptions).unwrap()
        else {
            panic!("expected a manual proxy");
        };
        assert_eq!(manual.all.as_deref(), Some("socks5h://u:p@proxy:1080"));
        assert_eq!(manual.exceptions, "files.example");
        assert!(proxy_choice(Some("proxy:3128"), false, no_env).is_ok());
    }

    #[test]
    fn a_proxy_that_is_not_a_proxy_url_is_refused_without_repeating_it() {
        for bad in [
            "ftp://user:secret@proxy",
            "://",
            "socks9://user:secret@proxy",
        ] {
            let error = proxy_choice(Some(bad), false, |_| None).unwrap_err();
            assert!(!printed(&error).contains("secret"), "{bad}");
        }
    }

    #[test]
    fn header_without_colon_is_refused() {
        assert!(parse_header("Cookie a=b").is_err());
    }

    /// The whole error, its context and every cause, as `main` prints it.
    fn printed(error: &anyhow::Error) -> String {
        format!("{error:?} / {error:#}")
    }

    // A header value is often a password or a token, and error text reaches the terminal,
    // shell history tools and bug reports (IPB-11).
    #[test]
    fn a_refused_header_is_named_without_its_value() {
        let bad_value = parse_header("Authorization: Bearer tok\u{7f}en-secret").unwrap_err();
        let shown = printed(&bad_value);
        assert!(!shown.contains("en-secret"), "{shown}");
        assert!(shown.contains("Authorization"), "{shown}");

        let no_colon = parse_header("Authorization Bearer token-secret").unwrap_err();
        let shown = printed(&no_colon);
        assert!(!shown.contains("token-secret"), "{shown}");

        let bad_name = parse_header("Bad Name: token-secret").unwrap_err();
        let shown = printed(&bad_name);
        assert!(!shown.contains("token-secret"), "{shown}");
    }

    #[test]
    fn a_refused_link_is_not_repeated_when_it_may_hold_a_password() {
        let args = Args::parse_from(["wirecatch", "ftp://me:pass-secret@[bad/a.zip"]);
        let shown = printed(&build_request(args).unwrap_err());
        assert!(!shown.contains("pass-secret"), "{shown}");
        let args = Args::parse_from(["wirecatch", "not a link"]);
        let shown = printed(&build_request(args).unwrap_err());
        assert!(shown.contains("not a link"), "{shown}");
    }

    #[test]
    fn default_connections_fit_the_accepted_range() {
        assert!((1..=MAX_CONNECTIONS).contains(&default_connections()));
    }

    // A paused download used to exit 0, so `wirecatch URL && next-step` went on with an unfinished file.
    #[test]
    fn only_a_saved_file_exits_zero() {
        assert_eq!(Ending::Saved.status(), 0);
        assert_eq!(Ending::Stopped.status(), EXIT_INTERRUPTED);
    }

    // A reader that went away does not change what the status says about the download; any
    // other failure to report it does.
    #[test]
    fn a_closed_pipe_keeps_the_status_and_another_write_failure_is_an_error() {
        let ok = Ok(());
        assert_eq!(exit_code(Ending::Saved, ok), ExitCode::SUCCESS);
        let closed = || Err(io::Error::from(io::ErrorKind::BrokenPipe));
        assert_eq!(exit_code(Ending::Saved, closed()), ExitCode::SUCCESS);
        assert_eq!(
            exit_code(Ending::Stopped, closed()),
            ExitCode::from(EXIT_INTERRUPTED)
        );
        let full = Err(io::Error::other("disk full"));
        assert_eq!(exit_code(Ending::Saved, full), ExitCode::FAILURE);
    }

    #[test]
    fn progress_styles_are_valid() {
        assert!(ProgressStyle::with_template(STYLE_KNOWN_SIZE).is_ok());
        assert!(ProgressStyle::with_template(STYLE_UNKNOWN_SIZE).is_ok());
    }
}
