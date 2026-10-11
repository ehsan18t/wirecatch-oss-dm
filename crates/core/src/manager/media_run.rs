//! Running a media download: a stream through `wirecatch-streams`, or a page's
//! formats found again with yt-dlp (their addresses expire) and downloaded by
//! the engine or `wirecatch-streams`, then joined with ffmpeg. Formats neither
//! can download are left to yt-dlp itself.
//!
//! Parts live in `<file>.wcmedia/` until the file is complete, so pausing,
//! failing or restarting keeps them; a part finished in an earlier run is not
//! downloaded again.

use std::net::IpAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use wirecatch_engine::{
    CancellationToken, DownloadOptions, DownloadRequest, Error as EngineError, Outcome,
    PrivateNetwork, Progress, RateLimiter, Url,
};
use wirecatch_protocol::DownloadId;
use wirecatch_protocol::app::{FixAction, UserError};
use wirecatch_protocol::extension::Header;
use wirecatch_streams::{Choice, StreamProgress, StreamRequest};

use super::Manager;
use super::media::{addon_missing, cookie_header, media_options};
use super::run::{MediaDone, RunEnd};
use super::state::{Gauge, State};
use crate::addons::MediaTools;
use crate::media::merge::{self, Joined};
use crate::media::ytdlp::{self, Fetched, Format, Transport};
use crate::media::{self, Content, MediaJob, MediaProgress, Pick, safe_ext};
use crate::store::Origin;
use crate::{headers, user_error};

/// Everything a media run needs, copied out of the manager's state.
struct Job {
    output: PathBuf,
    url: Url,
    origin: Origin,
    tools: Option<MediaTools>,
    options: DownloadOptions,
    temp_dir: PathBuf,
    timeout: Duration,
    /// Reserves a path for the finished file among the paths other downloads
    /// hold; `false` if one of them holds it already.
    reserve: Reserve,
}

/// Reserves a path for a finished file (`true` to reserve, `false` to give a
/// reservation back); reserving returns `false` when another download holds it.
type Reserve = Arc<dyn Fn(&Path, bool) -> bool + Send + Sync>;

impl Manager {
    /// Starts media download `id` (the caller checked every limit).
    #[allow(clippy::too_many_arguments)]
    pub(super) fn start_media_run(
        &self,
        state: &mut State,
        id: DownloadId,
        run_id: u64,
        answered: CancellationToken,
        job: MediaJob,
        connections: usize,
        limiters: Vec<Arc<RateLimiter>>,
    ) {
        let Some(entry) = state.downloads.get(&id) else {
            return;
        };
        let url = match Url::parse(&entry.row.url) {
            Ok(url) => url,
            Err(e) => {
                let error = user_error::describe(
                    &EngineError::InvalidUrl(e.to_string()),
                    Path::new(""),
                    false,
                );
                self.fail_to_start(state, id, error);
                return;
            }
        };
        let mut options = media_options(&entry.origin);
        options.connections = connections.max(1);
        options.rate_limiters = limiters;
        let tools = state.addon.tools.clone();
        let manager = Arc::downgrade(&self.inner);
        let work = Job {
            output: entry.path().to_path_buf(),
            url,
            origin: entry.origin.clone(),
            tools: state.addon.tools.clone(),
            options,
            temp_dir: self.inner.temp_dir.clone(),
            timeout: self.inner.media_timeout,
            reserve: {
                let manager = Arc::downgrade(&self.inner);
                Arc::new(move |path: &Path, take: bool| {
                    Manager::from_weak(&manager).is_some_and(|m| {
                        let mut state = m.lock();
                        if !take {
                            state.claimed.release(path);
                            true
                        } else if state.claimed.contains(path) {
                            false
                        } else {
                            state.claimed.reserve(path);
                            true
                        }
                    })
                })
            },
        };
        self.launch(state, id, run_id, answered, move |cancel| {
            let progress = Arc::new(MediaProgress::default());
            let gauge = Gauge::Media(progress.clone());
            (gauge, async move {
                let end = run(work, job, &progress, &cancel).await;
                if tools.is_some_and(|t| t.is_broken())
                    && let Some(manager) = Manager::from_weak(&manager)
                {
                    manager.spawn_addon_rescan();
                }
                RunEnd::Media(end)
            })
        });
    }
}

async fn run(
    job: Job,
    what: MediaJob,
    progress: &MediaProgress,
    cancel: &CancellationToken,
) -> Result<MediaDone, UserError> {
    let result = match what {
        MediaJob::Stream { video, audio } => run_stream(&job, video, audio, progress, cancel).await,
        MediaJob::Page {
            format,
            video,
            audio,
        } => run_page(&job, &format, video, audio, progress, cancel).await,
        MediaJob::Unreadable => return Err(media::unreadable()),
    };
    match result {
        Err(_) if cancel.is_cancelled() => Ok(MediaDone::Paused),
        other => other,
    }
}

fn choice(pick: Pick) -> Choice {
    match pick {
        Pick::Auto => Choice::Auto,
        Pick::Track(id) => Choice::Track(id),
        Pick::Skip => Choice::Skip,
    }
}

async fn run_stream(
    job: &Job,
    video: Pick,
    audio: Pick,
    progress: &MediaProgress,
    cancel: &CancellationToken,
) -> Result<MediaDone, UserError> {
    let mut request = StreamRequest::new(job.url.clone(), &job.output);
    request.video = choice(video);
    request.audio = choice(audio);
    request.ffmpeg = job.tools.as_ref().map(|t| t.ffmpeg.clone());
    request.options = job.options.clone();
    let stream = Arc::new(StreamProgress::default());
    progress.add_stream(stream.clone());
    match wirecatch_streams::download(request, stream, cancel.clone()).await {
        Ok(wirecatch_streams::Outcome::Completed { path, bytes, .. }) => {
            Ok(MediaDone::Completed { path, bytes })
        }
        Ok(_) | Err(wirecatch_streams::Error::Cancelled) => Ok(MediaDone::Paused),
        Err(e) => {
            note_missing_ffmpeg(job, &e);
            Err(stream_error(
                &e,
                job.output.parent().unwrap_or(Path::new("")),
            ))
        }
    }
}

/// A part's role, which names its file in the work folder.
const ROLES: [&str; 2] = ["video", "audio"];

async fn run_page(
    job: &Job,
    format: &str,
    video: Option<String>,
    audio: Option<String>,
    progress: &MediaProgress,
    cancel: &CancellationToken,
) -> Result<MediaDone, UserError> {
    let tools = job.tools.as_ref().ok_or_else(addon_missing)?;
    let work = media::work_dir(&job.output);
    tokio::fs::create_dir_all(&work)
        .await
        .map_err(|e| disk(&e, &job.output))?;
    let cookies = cookie_header(&job.origin);
    let ctx = ytdlp::Context {
        tools,
        page: &job.url,
        cookies: cookies.as_deref(),
        user_agent: job.origin.user_agent.as_deref(),
        temp_dir: &job.temp_dir,
        timeout: job.timeout,
    };
    // Format addresses expire: read the page again on every start.
    let info = ytdlp::extract(&ctx, cancel).await?;
    let formats = info.all_formats();
    let wanted: Vec<&String> = [&video, &audio].into_iter().flatten().collect();
    let found: Vec<&Format> = wanted
        .iter()
        .filter_map(|id| formats.iter().find(|f| &&f.format_id == id))
        .collect();
    let ourselves = !wanted.is_empty()
        && found.len() == wanted.len()
        && found
            .iter()
            .all(|f| f.transport() != Transport::YtDlp && f.url.is_some());
    if !ourselves {
        // A format Wirecatch cannot fetch itself, or the page changed: yt-dlp
        // downloads the same selection. Its own requests are not covered by the
        // local network rule below (see `network_rule`); yt-dlp makes them.
        return match ytdlp::download(&ctx, format, &work, progress, cancel).await? {
            Fetched::Done(file) => finish(&file, job, &work).await,
            Fetched::Cancelled => Ok(MediaDone::Paused),
        };
    }
    let mut rules = Vec::with_capacity(found.len());
    for format in &found {
        let url = format
            .url
            .as_deref()
            .and_then(|u| Url::parse(u).ok())
            .ok_or_else(|| refresh("The page gave an address for the video that is not valid."))?;
        rules.push(network_rule(&job.url, &url).await?);
    }
    let parts = found
        .iter()
        .zip(ROLES)
        .zip(rules)
        .map(|((f, role), rule)| fetch_part(job, f, role, rule, &work, progress, cancel));
    let mut files = Vec::new();
    let results = futures_join(parts.collect()).await;
    for result in results {
        match result? {
            Some(file) => files.push(file),
            None => return Ok(MediaDone::Paused),
        }
    }
    let file = match files.as_slice() {
        [single] => single.clone(),
        [video, audio] => {
            let ext = job
                .output
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("mkv");
            let joined = work.join(format!("joined.{ext}"));
            progress.set_processing(true);
            let result = merge::join(tools, video, audio, &joined, cancel).await;
            progress.set_processing(false);
            match result? {
                Joined::Done => joined,
                Joined::Cancelled => return Ok(MediaDone::Paused),
            }
        }
        _ => return Err(addon_missing()),
    };
    finish(&file, job, &work).await
}

/// An ffmpeg that could not be started means the add-on needs looking at again.
fn note_missing_ffmpeg(job: &Job, error: &wirecatch_streams::Error) {
    if let (wirecatch_streams::Error::FfmpegMissing(_), Some(tools)) = (error, &job.tools) {
        tools.mark_broken();
    }
}

/// Runs the part downloads side by side.
async fn futures_join<F: std::future::Future>(parts: Vec<F>) -> Vec<F::Output> {
    let mut parts = parts.into_iter();
    match (parts.next(), parts.next()) {
        (Some(a), Some(b)) => {
            let (a, b) = tokio::join!(a, b);
            vec![a, b]
        }
        (Some(a), None) => vec![a.await],
        _ => Vec::new(),
    }
}

/// Downloads one format into the work folder: `Ok(None)` when stopped.
async fn fetch_part(
    job: &Job,
    format: &Format,
    role: &str,
    rule: PrivateNetwork,
    work: &Path,
    progress: &MediaProgress,
    cancel: &CancellationToken,
) -> Result<Option<PathBuf>, UserError> {
    let marker = work.join(format!("{role}.done"));
    if let Ok(name) = tokio::fs::read_to_string(&marker).await {
        let file = work.join(name.trim());
        if let Ok(meta) = tokio::fs::metadata(&file).await
            && meta.is_file()
            && file.parent() == Some(work)
        {
            progress.add_done(meta.len());
            return Ok(Some(file));
        }
    }
    let url = format
        .url
        .as_deref()
        .and_then(|u| Url::parse(u).ok())
        .ok_or_else(|| refresh("The page gave an address for the video that is not valid."))?;
    let options = part_options(job, format, rule);
    let part = work.join(part_name(format, role));
    let file = match format.transport() {
        Transport::Hls => {
            let mut request = StreamRequest::new(url, part);
            request.ffmpeg = job.tools.as_ref().map(|t| t.ffmpeg.clone());
            request.private_network = rule;
            request.options = options;
            let stream = Arc::new(StreamProgress::default());
            progress.add_stream(stream.clone());
            match wirecatch_streams::download(request, stream, cancel.clone()).await {
                Ok(wirecatch_streams::Outcome::Completed { path, .. }) => path,
                Ok(_) | Err(wirecatch_streams::Error::Cancelled) => return Ok(None),
                Err(e) => {
                    note_missing_ffmpeg(job, &e);
                    return Err(stream_error(&e, work));
                }
            }
        }
        _ => {
            let path = part;
            let mut request = DownloadRequest::new(url, work);
            request.exact_path = Some(path.clone());
            request.options = options;
            let file = Arc::new(Progress::default());
            progress.add_file(file.clone(), format.size());
            match wirecatch_engine::download(request, file, cancel.clone()).await {
                Ok(Outcome::Completed { .. }) => path,
                Ok(_) | Err(EngineError::Cancelled) => return Ok(None),
                Err(e) => {
                    let folder = job.output.parent().unwrap_or(Path::new(""));
                    // A format address that stops working has usually expired;
                    // trying again reads the page for fresh ones.
                    let mut error = user_error::describe(&e, folder, true);
                    if error.fix == Some(FixAction::RefreshLink) {
                        error = UserError {
                            message: "The video's address on the server stopped working, as such addresses do after a while. Try again: Wirecatch reads the page again for fresh addresses and keeps what was already downloaded.".into(),
                            fix: Some(FixAction::Retry),
                        };
                    }
                    return Err(error);
                }
            }
        }
    };
    let name = file
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    tokio::fs::write(&marker, name)
        .await
        .map_err(|e| disk(&e, &job.output))?;
    Ok(Some(file))
}

/// Moves the finished file from the work folder to its place and removes the
/// work folders. The file keeps its own extension; if that differs from the
/// planned one and the name is taken, a free name is used.
async fn finish(file: &Path, job: &Job, work: &Path) -> Result<MediaDone, UserError> {
    let output = &job.output;
    let (file, planned, work) = (file.to_path_buf(), output.to_path_buf(), work.to_path_buf());
    let (url, referrer) = (
        job.url.to_string(),
        headers::referrer(&job.origin.headers).map(str::to_string),
    );
    let reserve = job.reserve.clone();
    let moved = tokio::task::spawn_blocking(move || -> std::io::Result<(PathBuf, u64)> {
        // Marked before it is put in place, so it is never there unmarked.
        crate::zone::mark(&file, &url, referrer.as_deref());
        let target = place(&file, &planned, &reserve)?;
        let bytes = std::fs::metadata(&target)?.len();
        let _ = std::fs::remove_dir_all(&work);
        Ok((target, bytes))
    })
    .await
    .map_err(|e| UserError {
        message: format!("Something went wrong inside Wirecatch ({e}). Try again."),
        fix: Some(FixAction::Retry),
    })?;
    let (path, bytes) = moved.map_err(|e| disk(&e, output))?;
    Ok(MediaDone::Completed { path, bytes })
}

/// The request options for one format: the format's own headers only (the
/// page's cookies stay with the page), bound to its own address, and the local
/// network rule decided for it.
fn part_options(job: &Job, format: &Format, rule: PrivateNetwork) -> DownloadOptions {
    let mut options = job.options.clone();
    let format_headers: Vec<Header> = format
        .http_headers
        .iter()
        .map(|(name, value)| Header {
            name: name.clone(),
            value: value.clone(),
        })
        .collect();
    options.headers = headers::to_map(&format_headers);
    if let Some(agent) = format
        .http_headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case("user-agent"))
        .map(|(_, v)| v.clone())
    {
        options.user_agent = agent;
    }
    options.private_network = rule;
    options
}

/// The name of a part in the work folder: its role and a media extension,
/// never one the site chose otherwise.
fn part_name(format: &Format, role: &str) -> String {
    let content = if format.has_video() {
        Content::Video
    } else {
        Content::Audio
    };
    format!("{role}.{}", safe_ext(format.ext.as_deref(), content))
}

/// Where a finished file goes: the planned path, with the file's own extension
/// when that is a different media extension (the container actually written).
/// An extension that is not a media one is never taken from the file: the
/// site, not the user, chose it.
fn final_path(file: &Path, planned: &Path) -> PathBuf {
    let own = file
        .extension()
        .and_then(|e| e.to_str())
        .filter(|e| media::is_media_ext(e))
        .map(str::to_ascii_lowercase);
    match own {
        Some(ext)
            if !planned
                .extension()
                .and_then(|p| p.to_str())
                .is_some_and(|p| p.eq_ignore_ascii_case(&ext)) =>
        {
            planned.with_extension(ext)
        }
        _ => planned.to_path_buf(),
    }
}

/// Moves the finished `file` to `planned` (with the file's own media
/// extension, see [`final_path`]) or, if that name is taken on disk or by
/// another download, to `name (1).ext` and so on. An existing file is never
/// replaced. Blocks on the disk.
fn place(file: &Path, planned: &Path, reserve: &Reserve) -> std::io::Result<PathBuf> {
    let wanted = final_path(file, planned);
    let dir = wanted.parent().unwrap_or(Path::new(""));
    let name = wanted
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    for n in 0..10_000 {
        let candidate = crate::naming::candidate(dir, &name, n);
        if !crate::naming::is_free_on_disk(&candidate) {
            continue;
        }
        // The planned path is this download's own; any other is reserved, and
        // given back if it cannot be used after all.
        let reserved = candidate != planned;
        if reserved && !reserve(&candidate, true) {
            continue;
        }
        // A hard link never replaces an existing file, so a file that appeared
        // since the check is left alone.
        let moved = match std::fs::hard_link(file, &candidate) {
            Ok(()) => {
                let _ = std::fs::remove_file(file);
                Ok(true)
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => Ok(false),
            // A drive without hard links (FAT, exFAT): move it, having checked.
            Err(_) => std::fs::rename(file, &candidate).map(|()| true),
        };
        match moved {
            Ok(true) => return Ok(candidate),
            Ok(false) | Err(_) if reserved => {
                reserve(&candidate, false);
                moved?;
            }
            other => {
                other?;
            }
        }
    }
    Err(std::io::Error::new(
        std::io::ErrorKind::AlreadyExists,
        "no free name for the video",
    ))
}

/// How a format's address may use the local network. A page that is itself on
/// the local network (a media server at home) may point anywhere. A page on the
/// internet may only point to the internet: a format whose host is local, or
/// whose name does not resolve (so cannot be checked), is refused, and the rest
/// download with the local network blocked for every request, redirect hop and
/// connection, so a later redirect or a name that changes its address (DNS
/// rebinding) cannot reach it either.
///
/// When yt-dlp downloads a format by itself (the fallback), its own requests
/// are not covered by this.
async fn network_rule(page: &Url, format: &Url) -> Result<PrivateNetwork, UserError> {
    if all_private(page).await {
        return Ok(PrivateNetwork::Allow);
    }
    let found = addresses(format).await;
    if found.is_empty() {
        return Err(UserError {
            message: "The server of this video's file could not be found, so Wirecatch could not check where it is and did not download it. Check your connection, then try again.".into(),
            fix: Some(FixAction::Retry),
        });
    }
    if found.into_iter().any(is_private) {
        return Err(UserError {
            message:
                "This video points to a device on your own network, which Wirecatch blocks for safety."
                    .into(),
            fix: None,
        });
    }
    Ok(PrivateNetwork::Block)
}

/// The addresses of `url`'s host: the address itself, or what its name
/// resolves to (nothing if it does not resolve).
async fn addresses(url: &Url) -> Vec<IpAddr> {
    let Some(host) = url.host_str() else {
        return Vec::new();
    };
    let host = host.trim_start_matches('[').trim_end_matches(']');
    if let Ok(ip) = host.parse::<IpAddr>() {
        return vec![ip];
    }
    let port = url.port_or_known_default().unwrap_or(443);
    match tokio::time::timeout(
        Duration::from_secs(10),
        tokio::net::lookup_host((host, port)),
    )
    .await
    {
        Ok(Ok(addrs)) => addrs.map(|a| a.ip()).collect(),
        _ => Vec::new(),
    }
}

async fn all_private(url: &Url) -> bool {
    let found = addresses(url).await;
    !found.is_empty() && found.into_iter().all(is_private)
}

fn is_private(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            v4.is_loopback()
                || v4.is_private()
                || v4.is_link_local()
                || v4.is_unspecified()
                || v4.octets()[0] == 0
        }
        IpAddr::V6(v6) => {
            if let Some(v4) = v6.to_ipv4_mapped() {
                return is_private(IpAddr::V4(v4));
            }
            let first = v6.segments()[0];
            v6.is_loopback()
                || v6.is_unspecified()
                || (first & 0xfe00) == 0xfc00
                || (first & 0xffc0) == 0xfe80
        }
    }
}

fn refresh(message: &str) -> UserError {
    UserError {
        message: format!("{message} Try again; Wirecatch reads the page again."),
        fix: Some(FixAction::Retry),
    }
}

fn disk(error: &std::io::Error, near: &Path) -> UserError {
    let folder = near.parent().unwrap_or(near);
    user_error::describe(
        &EngineError::Io(std::io::Error::new(error.kind(), error.to_string())),
        folder,
        false,
    )
}

/// A stream error, worded for a person with the fix to offer.
pub(super) fn stream_error(error: &wirecatch_streams::Error, folder: &Path) -> UserError {
    use wirecatch_streams::Error as E;
    let with = |fix: Option<FixAction>| UserError {
        message: error.to_string(),
        fix,
    };
    match error {
        E::Request(e) => user_error::describe(e, folder, false),
        E::Io(e) => user_error::describe(
            &EngineError::Io(std::io::Error::new(e.kind(), e.to_string())),
            folder,
            false,
        ),
        E::Live | E::Drm(_) | E::Unsupported(_) | E::PrivateNetwork | E::NothingChosen => {
            with(None)
        }
        E::NotAStream
        | E::Manifest(_)
        | E::Key { .. }
        | E::Decrypt { .. }
        | E::NoSuchTrack { .. } => with(Some(FixAction::RefreshLink)),
        _ => with(Some(FixAction::Retry)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn formats_of_a_page_on_the_internet_stay_on_the_internet() {
        let url = |u: &str| Url::parse(u).unwrap();
        let public_page = url("http://93.184.215.14/watch");
        let local_page = url("http://127.0.0.1:9/watch");
        // A page at home may point anywhere.
        assert_eq!(
            network_rule(&local_page, &url("http://192.168.1.10/v.mp4"))
                .await
                .unwrap(),
            PrivateNetwork::Allow
        );
        // A page on the internet: public formats download with the local
        // network blocked for every hop and connection.
        assert_eq!(
            network_rule(&public_page, &url("http://93.184.215.15/v.mp4"))
                .await
                .unwrap(),
            PrivateNetwork::Block
        );
        for local in [
            "http://127.0.0.1:8080/v.mp4",
            "http://localhost/v.mp4",
            "http://10.0.0.1/v.mp4",
            "http://[::1]/v.mp4",
            "http://169.254.169.254/latest",
        ] {
            let error = network_rule(&public_page, &url(local)).await.unwrap_err();
            assert!(
                error.message.contains("your own network"),
                "{local}: {}",
                error.message
            );
        }
        // A name that does not resolve cannot be checked, so it is refused.
        let error = network_rule(&public_page, &url("http://no-such-host.invalid/v.mp4"))
            .await
            .unwrap_err();
        assert!(
            error.message.contains("could not be found"),
            "{}",
            error.message
        );
    }

    #[test]
    fn a_format_downloads_under_its_network_rule() {
        let format: Format = serde_json::from_value(serde_json::json!({
            "format_id": "1", "ext": "mp4", "http_headers": {"X-Format": "yes"}
        }))
        .unwrap();
        let job = Job {
            output: PathBuf::from("/v/out.mp4"),
            url: Url::parse("http://93.184.215.14/watch").unwrap(),
            origin: Origin::default(),
            tools: None,
            options: DownloadOptions::default(),
            temp_dir: PathBuf::new(),
            timeout: Duration::from_secs(1),
            reserve: Arc::new(|_: &Path, _| true),
        };
        let options = part_options(&job, &format, PrivateNetwork::Block);
        assert_eq!(options.private_network, PrivateNetwork::Block);
        assert_eq!(options.headers.get("x-format").unwrap(), "yes");
    }

    #[tokio::test]
    async fn a_job_that_cannot_be_read_never_runs() {
        let job = Job {
            output: PathBuf::from("/v/out.mp4"),
            url: Url::parse("http://127.0.0.1:9/watch").unwrap(),
            origin: Origin::default(),
            tools: None,
            options: DownloadOptions::default(),
            temp_dir: PathBuf::new(),
            timeout: Duration::from_secs(1),
            reserve: Arc::new(|_: &Path, _| true),
        };
        let progress = MediaProgress::default();
        let ended = run(
            job,
            MediaJob::Unreadable,
            &progress,
            &CancellationToken::new(),
        )
        .await;
        assert_eq!(ended, Err(media::unreadable()));
    }

    /// The finished file carries the Mark of the Web the moment it is in place.
    #[cfg(windows)]
    #[tokio::test]
    async fn a_finished_video_is_marked_before_it_is_put_in_place() {
        let dir = tempfile::tempdir().unwrap();
        let work = dir.path().join("out.mp4.wcmedia");
        std::fs::create_dir_all(&work).unwrap();
        let part = work.join("video.mp4");
        std::fs::write(&part, b"video").unwrap();
        let job = Job {
            output: dir.path().join("out.mp4"),
            url: Url::parse("https://videos.example/watch?v=1&token=secret").unwrap(),
            origin: Origin::default(),
            tools: None,
            options: DownloadOptions::default(),
            temp_dir: PathBuf::new(),
            timeout: Duration::from_secs(1),
            reserve: Arc::new(|_: &Path, _| true),
        };
        let done = finish(&part, &job, &work).await.unwrap();
        let MediaDone::Completed { path, .. } = done else {
            panic!("not completed: {done:?}");
        };
        let mark = std::fs::read_to_string(format!("{}:Zone.Identifier", path.display()))
            .expect("no mark when the file was put in place");
        assert!(
            mark.contains("HostUrl=https://videos.example/watch\r\n"),
            "{mark}"
        );
        assert!(!mark.contains("secret"));
    }

    #[test]
    fn a_finished_video_never_replaces_a_file_or_takes_a_reserved_name() {
        let dir = tempfile::tempdir().unwrap();
        let planned = dir.path().join("out.mp4");
        std::fs::write(&planned, b"someone else's").unwrap();
        let file = dir.path().join("video.mp4");
        std::fs::write(&file, b"video").unwrap();
        let free: Reserve = Arc::new(|_: &Path, _| true);
        let placed = place(&file, &planned, &free).unwrap();
        assert_eq!(placed, dir.path().join("out (1).mp4"));
        assert_eq!(std::fs::read(&planned).unwrap(), b"someone else's");
        assert_eq!(std::fs::read(&placed).unwrap(), b"video");
        assert!(!file.exists());

        // Another extension: a name another download holds is skipped.
        let file = dir.path().join("video.webm");
        std::fs::write(&file, b"webm").unwrap();
        let held = dir.path().join("out.webm");
        let reserve: Reserve = Arc::new(move |p: &Path, _| p != held);
        let placed = place(&file, &planned, &reserve).unwrap();
        assert_eq!(placed, dir.path().join("out (1).webm"));
    }

    #[test]
    fn parts_get_media_extensions() {
        let format = |ext: &str, vcodec: &str| -> Format {
            serde_json::from_value(serde_json::json!({
                "format_id": "1", "ext": ext, "vcodec": vcodec, "acodec": "mp4a"
            }))
            .unwrap()
        };
        assert_eq!(part_name(&format("exe", "avc1"), "video"), "video.mp4");
        assert_eq!(part_name(&format("lnk", "none"), "audio"), "audio.m4a");
        assert_eq!(part_name(&format("webm", "vp9"), "video"), "video.webm");
    }

    #[test]
    fn a_file_keeps_its_extension_only_when_it_is_a_media_one() {
        let planned = Path::new("/v/Invoice.mp4");
        assert_eq!(
            final_path(Path::new("/w/media.webm"), planned),
            Path::new("/v/Invoice.webm")
        );
        assert_eq!(
            final_path(Path::new("/w/media.MKV"), planned),
            Path::new("/v/Invoice.mkv")
        );
        for bad in [
            "media.exe",
            "media.bat",
            "media.lnk",
            "media",
            "media.mp4.scr",
        ] {
            assert_eq!(
                final_path(&Path::new("/w").join(bad), planned),
                planned,
                "{bad}"
            );
        }
    }

    #[test]
    fn local_addresses_are_recognised() {
        for ip in [
            "127.0.0.1",
            "10.1.2.3",
            "192.168.1.1",
            "169.254.1.1",
            "0.0.0.0",
            "::1",
            "fd00::1",
            "fe80::1",
            "::ffff:192.168.0.1",
        ] {
            assert!(is_private(ip.parse().unwrap()), "{ip}");
        }
        for ip in ["8.8.8.8", "2606:4700::1111"] {
            assert!(!is_private(ip.parse().unwrap()), "{ip}");
        }
    }

    #[test]
    fn every_stream_error_reads_as_sentences_with_a_fix_where_one_helps() {
        use wirecatch_streams::Error as E;
        let folder = Path::new("/downloads");
        let errors = [
            (E::Live, None),
            (E::Drm("Widevine".into()), None),
            (E::NotAStream, Some(FixAction::RefreshLink)),
            (
                E::Manifest("no segments".into()),
                Some(FixAction::RefreshLink),
            ),
            (E::NeedsFfmpeg, Some(FixAction::Retry)),
            (E::Ffmpeg("bad".into()), Some(FixAction::Retry)),
            (E::PrivateNetwork, None),
            (E::Unsupported("several periods".into()), None),
            (E::Cancelled, Some(FixAction::Retry)),
        ];
        for (error, fix) in errors {
            let user = stream_error(&error, folder);
            assert_eq!(user.fix, fix, "{error:?}");
            assert!(user.message.ends_with('.'), "{}", user.message);
            assert!(!user.message.contains('\u{2014}'));
        }
        let network = stream_error(
            &E::Request(EngineError::HttpStatus {
                status: 404,
                url: "u".into(),
            }),
            folder,
        );
        assert_eq!(network.fix, Some(FixAction::RefreshLink));
    }
}
