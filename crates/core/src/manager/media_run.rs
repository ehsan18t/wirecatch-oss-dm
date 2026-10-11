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
    CancellationToken, DownloadOptions, DownloadRequest, Error as EngineError, Outcome, Progress,
    RateLimiter, Url,
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
}

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
        let work = Job {
            output: entry.path().to_path_buf(),
            url,
            origin: entry.origin.clone(),
            tools: state.addon.tools.clone(),
            options,
            temp_dir: self.inner.temp_dir.clone(),
            timeout: self.inner.media_timeout,
        };
        self.launch(state, id, run_id, answered, move |cancel| {
            let progress = Arc::new(MediaProgress::default());
            let gauge = Gauge::Media(progress.clone());
            (gauge, async move {
                RunEnd::Media(run(work, job, &progress, &cancel).await)
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
        Err(e) => Err(stream_error(
            &e,
            job.output.parent().unwrap_or(Path::new("")),
        )),
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
        // downloads the same selection.
        return match ytdlp::download(&ctx, format, &work, progress, cancel).await? {
            Fetched::Done(file) => finish(&file, &job.output, &work).await,
            Fetched::Cancelled => Ok(MediaDone::Paused),
        };
    }
    for format in &found {
        guard_network(&job.url, format.url.as_deref().unwrap_or_default()).await?;
    }
    let parts = found
        .iter()
        .zip(ROLES)
        .map(|(f, role)| fetch_part(job, f, role, &work, progress, cancel));
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
            let ffmpeg = &tools.ffmpeg;
            let ext = job
                .output
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("mkv");
            let joined = work.join(format!("joined.{ext}"));
            progress.set_processing(true);
            let result = merge::join(ffmpeg, video, audio, &joined, cancel).await;
            progress.set_processing(false);
            match result? {
                Joined::Done => joined,
                Joined::Cancelled => return Ok(MediaDone::Paused),
            }
        }
        _ => return Err(addon_missing()),
    };
    finish(&file, &job.output, &work).await
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
    let mut options = job.options.clone();
    let format_headers: Vec<Header> = format
        .http_headers
        .iter()
        .map(|(name, value)| Header {
            name: name.clone(),
            value: value.clone(),
        })
        .collect();
    // The page's cookies stay with the page: a format gets only the headers
    // yt-dlp gave for it, bound to its own address.
    options.headers = headers::to_map(&format_headers);
    if let Some(agent) = format
        .http_headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case("user-agent"))
        .map(|(_, v)| v.clone())
    {
        options.user_agent = agent;
    }
    let part = work.join(part_name(format, role));
    let file = match format.transport() {
        Transport::Hls => {
            let mut request = StreamRequest::new(url, part);
            request.ffmpeg = job.tools.as_ref().map(|t| t.ffmpeg.clone());
            request.options = options;
            let stream = Arc::new(StreamProgress::default());
            progress.add_stream(stream.clone());
            match wirecatch_streams::download(request, stream, cancel.clone()).await {
                Ok(wirecatch_streams::Outcome::Completed { path, .. }) => path,
                Ok(_) | Err(wirecatch_streams::Error::Cancelled) => return Ok(None),
                Err(e) => return Err(stream_error(&e, work)),
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
async fn finish(file: &Path, output: &Path, work: &Path) -> Result<MediaDone, UserError> {
    let (file, planned, work) = (file.to_path_buf(), output.to_path_buf(), work.to_path_buf());
    let moved = tokio::task::spawn_blocking(move || -> std::io::Result<(PathBuf, u64)> {
        let wanted = final_path(&file, &planned);
        let target = if wanted == planned {
            planned
        } else {
            free_path(&wanted)
        };
        std::fs::rename(&file, &target)?;
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

/// `wanted`, or `name (1).ext` and so on if it is taken. Blocks on the disk.
fn free_path(wanted: &Path) -> PathBuf {
    let dir = wanted.parent().unwrap_or(Path::new(""));
    let name = wanted
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    (0..)
        .map(|n| crate::naming::candidate(dir, &name, n))
        .find(|p| crate::naming::is_free_on_disk(p))
        .unwrap_or_else(|| wanted.to_path_buf())
}

/// Refuses a format on the local network when the page is on the internet, so a
/// website cannot make Wirecatch reach the router or other devices at home.
async fn guard_network(page: &Url, format: &str) -> Result<(), UserError> {
    let Ok(format) = Url::parse(format) else {
        return Ok(());
    };
    if all_private(page).await || !any_private(&format).await {
        return Ok(());
    }
    Err(UserError {
        message:
            "This video points to a device on your own network, which Wirecatch blocks for safety."
                .into(),
        fix: None,
    })
}

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

async fn any_private(url: &Url) -> bool {
    addresses(url).await.into_iter().any(is_private)
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
