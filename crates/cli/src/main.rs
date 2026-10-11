//! `wirecatch`, the command-line downloader built on the Wirecatch engine.
//!
//! Exit status: 0 when the file is complete, 1 on an error, 2 for a usage mistake (from clap) and 130 when the download was paused or stopped with Ctrl+C. Running the same command again continues a paused download.
#![expect(
    clippy::print_stdout,
    reason = "a command-line tool reports its result on standard output"
)]

use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, bail};
use clap::Parser;
use indicatif::{HumanBytes, ProgressBar, ProgressStyle};
use wirecatch_engine::header::{HeaderName, HeaderValue};
use wirecatch_engine::{
    CancellationToken, DownloadOptions, DownloadRequest, Outcome, Progress, Url, download,
};

/// Environment variable that sets the log filter, in `tracing` filter syntax.
const LOG_ENV: &str = "WIRECATCH_LOG";
/// Log filter used when [`LOG_ENV`] is not set: only problems.
const DEFAULT_LOG_FILTER: &str = "warn";
/// Upper bound for `--connections`; more than this only adds load on the server.
const MAX_CONNECTIONS: u16 = 64;
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
#[command(name = "wirecatch", version)]
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
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_env(LOG_ENV)
                .unwrap_or_else(|_| DEFAULT_LOG_FILTER.into()),
        )
        .with_writer(std::io::stderr)
        .init();

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

    let ending = report(&result?);
    Ok(ending.exit_code())
}

/// Turns the command line into an engine request, refusing combinations the engine cannot honour.
fn build_request(args: Args) -> anyhow::Result<DownloadRequest> {
    let url = Url::parse(&args.url).with_context(|| format!("not a valid URL: {}", args.url))?;
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
    request.file_name = args.name;
    request.options.connections = args.connections.into();
    request.options.http3 = args.http3;
    for raw in &args.headers {
        let (name, value) = parse_header(raw)?;
        request.options.headers.append(name, value);
    }
    Ok(request)
}

/// Parses one `-H "Name: value"` argument.
fn parse_header(raw: &str) -> anyhow::Result<(HeaderName, HeaderValue)> {
    let (name, value) = raw
        .split_once(':')
        .with_context(|| format!("header must look like 'Name: value': {raw}"))?;
    let name = HeaderName::from_bytes(name.trim().as_bytes())
        .with_context(|| format!("bad header name in {raw}"))?;
    let value = HeaderValue::from_str(value.trim())
        .with_context(|| format!("bad header value in {raw}"))?;
    Ok((name, value))
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

/// Prints how the download ended.
fn report(outcome: &Outcome) -> Ending {
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
            println!("Saved {} to {}{kept}", HumanBytes(*bytes), path.display());
            Ending::Saved
        }
        Outcome::Paused {
            path, remaining, ..
        } => {
            println!(
                "Paused with {} left for {}. Run the same command again to continue.",
                HumanBytes(*remaining),
                path.display()
            );
            Ending::Stopped
        }
        _ => {
            println!(
                "Wirecatch stopped before the download finished. Run the same command again to continue."
            );
            Ending::Stopped
        }
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
    fn header_without_colon_is_refused() {
        assert!(parse_header("Cookie a=b").is_err());
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

    #[test]
    fn progress_styles_are_valid() {
        assert!(ProgressStyle::with_template(STYLE_KNOWN_SIZE).is_ok());
        assert!(ProgressStyle::with_template(STYLE_UNKNOWN_SIZE).is_ok());
    }
}
