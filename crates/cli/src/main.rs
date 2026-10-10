use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, bail};
use clap::Parser;
use indicatif::{HumanBytes, ProgressBar, ProgressStyle};
use wirecatch_engine::header::{HeaderName, HeaderValue};
use wirecatch_engine::{CancellationToken, DownloadRequest, Outcome, Progress, Url, download};

/// Fast, resumable downloads over parallel connections.
///
/// Run the same command again after an interruption to continue where it stopped.
#[derive(Parser, Debug)]
#[command(name = "wirecatch", version)]
struct Args {
    /// The URL to download: http, https, ftp, ftps or ftpes.
    ///
    /// FTP links may carry a login, as in ftp://user:password@host/path; without one the download logs in anonymously. ftps:// is encrypted from the start (port 990), or with AUTH TLS on port 21; ftpes:// always uses AUTH TLS. An encrypted link never falls back to an unencrypted connection.
    url: String,

    /// Folder to save into.
    #[arg(short = 'o', long, default_value = ".")]
    output_dir: PathBuf,

    /// Save under this file name instead of the one the server suggests.
    #[arg(short = 'n', long)]
    name: Option<String>,

    /// Most parallel connections.
    #[arg(short = 'c', long, default_value_t = 8, value_parser = clap::value_parser!(u16).range(1..=64))]
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

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_env("WIRECATCH_LOG")
                .unwrap_or_else(|_| "warn".into()),
        )
        .with_writer(std::io::stderr)
        .init();

    let args = Args::parse();
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
        let (name, value) = raw
            .split_once(':')
            .with_context(|| format!("header must look like 'Name: value': {raw}"))?;
        request.options.headers.append(
            HeaderName::from_bytes(name.trim().as_bytes())
                .with_context(|| format!("bad header name in {raw}"))?,
            HeaderValue::from_str(value.trim())
                .with_context(|| format!("bad header value in {raw}"))?,
        );
    }

    let progress = Arc::new(Progress::default());
    let cancel = CancellationToken::new();
    {
        let cancel = cancel.clone();
        tokio::spawn(async move {
            if tokio::signal::ctrl_c().await.is_ok() {
                eprintln!("\nPausing... (press Ctrl+C again to quit immediately)");
                cancel.cancel();
                if tokio::signal::ctrl_c().await.is_ok() {
                    std::process::exit(130);
                }
            }
        });
    }

    let bar = (!args.quiet).then(|| spawn_progress_bar(progress.clone()));
    let result = download(request, progress, cancel).await;
    if let Some((bar, task)) = bar {
        task.abort();
        bar.finish_and_clear();
    }

    match result? {
        Outcome::Completed {
            path,
            bytes,
            resumed_from,
            ..
        } => {
            let kept = if resumed_from > 0 {
                format!(" ({} kept from earlier)", HumanBytes(resumed_from))
            } else {
                String::new()
            };
            println!("Saved {} to {}{kept}", HumanBytes(bytes), path.display());
        }
        Outcome::Paused {
            path, remaining, ..
        } => {
            println!(
                "Paused with {} left for {}. Run the same command again to continue.",
                HumanBytes(remaining),
                path.display()
            );
        }
        other => println!("Finished: {other:?}"),
    }
    Ok(())
}

fn spawn_progress_bar(progress: Arc<Progress>) -> (ProgressBar, tokio::task::JoinHandle<()>) {
    let bar = ProgressBar::new_spinner();
    let known = ProgressStyle::with_template(
        "{msg} [{bar:30}] {bytes}/{total_bytes}  {binary_bytes_per_sec}  ETA {eta}  {prefix}",
    )
    .expect("valid template")
    .progress_chars("=> ");
    let unknown =
        ProgressStyle::with_template("{spinner} {msg} {bytes}  {binary_bytes_per_sec}  {prefix}")
            .expect("valid template");
    bar.set_style(unknown.clone());

    let task = {
        let bar = bar.clone();
        tokio::spawn(async move {
            let mut has_total = false;
            loop {
                if let Some(name) = progress.file_name() {
                    bar.set_message(name);
                }
                match progress.total() {
                    Some(total) if !has_total => {
                        bar.set_style(known.clone());
                        bar.set_length(total);
                        has_total = true;
                    }
                    _ => {}
                }
                bar.set_position(progress.done());
                let n = progress.connections();
                bar.set_prefix(format!("{n} conn{}", if n == 1 { "" } else { "s" }));
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        })
    };
    (bar, task)
}
