//! Running a media download: a stream through `wirecatch-streams`, or a page's
//! formats found again with yt-dlp (their addresses expire) and downloaded by
//! the engine or `wirecatch-streams`, then joined with ffmpeg. Formats neither
//! can download are left to yt-dlp itself.
//!
//! Parts live in `<file>.wcmedia/` until the file is complete, so pausing,
//! failing or restarting keeps them; a part finished in an earlier run is not
//! downloaded again.

use std::collections::HashMap;
use std::net::IpAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tracing::warn;
use wirecatch_engine::numeric::saturating_cast;
use wirecatch_engine::{
    CancellationToken, DownloadOptions, DownloadRequest, Error as EngineError, Outcome,
    PrivateNetwork, Progress, Url,
};
use wirecatch_protocol::app::{FixAction, ProcessingStep, UserError};
use wirecatch_protocol::extension::Header;
use wirecatch_protocol::{DownloadId, Secret};
use wirecatch_streams::{Choice, Role, StreamProgress, StreamRequest};

use super::Manager;
use super::media::{addon_missing, cookie_header, media_options};
use super::run::{Bandwidth, MediaDone, RunEnd};
use super::state::{Gauge, State};
use crate::addons::{Check, MediaTools};
use crate::clock::Clock;
use crate::local_network::{self, ChosenBy};
use crate::media::convert;
use crate::media::describe;
use crate::media::ffmpeg::{FfmpegError, Finished};
use crate::media::merge;
use crate::media::output::{
    self, Container, ImageFormat, Inputs, OutputJob, SubtitleFormat, SubtitlePick, VideoAction,
};
use crate::media::ytdlp::{self, Fetched, Format, FormatId, Transport};
use crate::media::{self, Content, MediaJob, MediaProgress, Pick, safe_ext};
use crate::store::Origin;
use crate::{headers, user_error};
use wirecatch_engine::units::MIB;

/// Everything a media run needs, copied out of the manager's state.
struct Job {
    output: PathBuf,
    url: Url,
    origin: Origin,
    /// Who chose the link, which decides the rule for the local network.
    chosen_by: ChosenBy,
    tools: Option<MediaTools>,
    options: DownloadOptions,
    temp_dir: PathBuf,
    timeout: Duration,
    /// The pauses before each automatic try with fresh addresses; see
    /// [`ManagerConfig::media_refresh_delays`](crate::ManagerConfig::media_refresh_delays).
    refresh_delays: Arc<[Duration]>,
    /// Tells the time of the next try, shown while waiting for it.
    clock: Arc<dyn Clock>,
    /// Reserves a path for the finished file among the paths other downloads
    /// hold; `false` if one of them holds it already.
    reserve: Reserve,
}

/// Reserves a path for a finished file (`true` to reserve, `false` to give a
/// reservation back); reserving returns `false` when another download holds it.
type Reserve = Arc<dyn Fn(&Path, bool) -> bool + Send + Sync>;

impl Manager {
    /// Starts media download `id` (the caller checked every limit).
    pub(super) fn start_media_run(
        &self,
        state: &mut State,
        id: DownloadId,
        run_id: u64,
        answered: CancellationToken,
        job: MediaJob,
        bandwidth: Bandwidth,
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
        if local_network::refuses_address(entry.chosen_by, &url) {
            let error = user_error::private_network(entry.chosen_by);
            self.fail_to_start(state, id, error);
            return;
        }
        let mut options = media_options(
            &entry.origin,
            entry.chosen_by.private_network(),
            state.proxy(),
        );
        options.connections = bandwidth.connections.max(1);
        options.rate_limiters = bandwidth.limiters;
        let tools = state.addon.tools.clone();
        let manager = Arc::downgrade(&self.inner);
        let work = Job {
            output: entry.path().to_path_buf(),
            url,
            origin: entry.origin.clone(),
            chosen_by: entry.chosen_by,
            tools: state.addon.tools.clone(),
            options,
            temp_dir: self.inner.temp_dir.clone(),
            timeout: self.inner.media_timeout,
            refresh_delays: self.inner.media_refresh_delays.clone(),
            clock: self.inner.clock.clone(),
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
                    // A program that would not start: check every file closely.
                    manager.spawn_addon_rescan(Check::Hashes);
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
            video,
            audio,
            output,
            keep_ext,
        } => {
            let page = PageJob {
                video,
                audio,
                output: output.map(|o| *o),
                keep_ext,
            };
            run_page(&job, &page, progress, cancel).await
        }
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
    // The stream is saved in the work folder first and put in place by
    // `finish`, which marks it as downloaded from the internet before it is
    // ever at its final name.
    let work = media::work_dir(&job.output);
    tokio::fs::create_dir_all(&work)
        .await
        .map_err(|e| disk(&e, &job.output))?;
    let staged = work.join(job.output.file_name().unwrap_or_else(|| "stream".as_ref()));
    let request = stream_request(job, &staged, video, audio);
    let stream = Arc::new(StreamProgress::default());
    progress.add_stream(stream.clone());
    match wirecatch_streams::download(request, stream, cancel.clone()).await {
        Ok(wirecatch_streams::Outcome::Completed { path, .. }) => {
            finish(&path, job, &work, &[]).await
        }
        Ok(_) | Err(wirecatch_streams::Error::Cancelled) => Ok(MediaDone::Paused),
        Err(e) => Err(describe::stream(
            &e,
            job.output.parent().unwrap_or(Path::new("")),
            job.tools.as_ref(),
            job.chosen_by,
        )),
    }
}

/// What stream job `job` asks of `wirecatch-streams`, saving at `staged`.
fn stream_request(job: &Job, staged: &Path, video: Pick, audio: Pick) -> StreamRequest {
    let mut request = StreamRequest::new(job.url.clone(), staged, job.chosen_by.private_network());
    request.video = choice(video);
    request.audio = choice(audio);
    request.ffmpeg = job.tools.as_ref().map(|t| t.ffmpeg.clone());
    request.options = job.options.clone();
    request
}

/// A part's role, which names its file in the work folder.
const ROLES: [&str; 2] = ["video", "audio"];

/// Why one attempt at a page's video did not finish.
#[derive(Debug)]
enum PageError {
    /// The server refused a format's address or said it expired (`status`,
    /// when it answered with one). Reading the page again gives fresh
    /// addresses, which usually work at once.
    Refused {
        status: Option<u16>,
    },
    Failed(UserError),
}

impl From<UserError> for PageError {
    fn from(error: UserError) -> Self {
        PageError::Failed(error)
    }
}

/// What a page's video downloads and how it is saved.
struct PageJob {
    video: Option<FormatId>,
    audio: Option<FormatId>,
    output: Option<OutputJob>,
    keep_ext: Option<String>,
}

/// Downloads a page's chosen formats. When the server refuses a format's
/// address, the page is read again for fresh addresses and the download goes on
/// by itself after each of [`Job::refresh_delays`], keeping the parts already
/// finished; only when every try is used up does the person see an error.
async fn run_page(
    job: &Job,
    page: &PageJob,
    progress: &MediaProgress,
    cancel: &CancellationToken,
) -> Result<MediaDone, UserError> {
    let mut delays = job.refresh_delays.iter();
    loop {
        let status = match page_attempt(job, page, progress, cancel).await {
            Ok(done) => return Ok(done),
            Err(PageError::Failed(error)) => return Err(error),
            Err(PageError::Refused { status }) => status,
        };
        let Some(delay) = delays.next() else {
            return Err(refused_after_refreshing(status));
        };
        warn!(
            status,
            retry_in = ?delay,
            "the server refused a video address; reading the page again for fresh ones"
        );
        let wait_ms: i64 = saturating_cast(delay.as_millis());
        progress.set_retry_at(Some(job.clock.unix_millis().saturating_add(wait_ms)));
        tokio::select! {
            () = cancel.cancelled() => return Ok(MediaDone::Paused),
            () = tokio::time::sleep(*delay) => {}
        }
    }
}

/// One attempt at a page's video. Unless an earlier attempt downloaded
/// everything (its list is in the work folder), the page is read again for fresh
/// format addresses and what is not finished yet is downloaded: the formats,
/// and the subtitles, cover picture and metadata the output wants. Then the
/// file is saved; a conversion that was paused or failed starts again from the
/// downloaded parts, never from the download.
async fn page_attempt(
    job: &Job,
    page: &PageJob,
    progress: &MediaProgress,
    cancel: &CancellationToken,
) -> Result<MediaDone, PageError> {
    let tools = job.tools.as_ref().ok_or_else(addon_missing)?;
    let work = media::work_dir(&job.output);
    tokio::fs::create_dir_all(&work)
        .await
        .map_err(|e| disk(&e, &job.output))?;
    if let Some((downloaded, bytes)) = Downloaded::read(&work).await {
        progress.start_over();
        progress.add_done(bytes);
        return Ok(save(job, page, tools, &downloaded, &work, progress, cancel).await?);
    }
    let (video, audio) = (&page.video, &page.audio);
    let format = &ytdlp::selection(video.as_ref(), audio.as_ref());
    let cookies = cookie_header(&job.origin);
    let proxy = crate::proxy::ytdlp_proxy(&job.options.proxy);
    let ctx = ytdlp::Context {
        tools,
        page: &job.url,
        cookies: cookies.as_deref(),
        user_agent: job.origin.user_agent.as_deref(),
        temp_dir: &job.temp_dir,
        timeout: job.timeout,
        proxy: proxy.as_deref(),
    };
    // Format addresses expire: read the page again on every start.
    let info = ytdlp::extract(&ctx, cancel)
        .await
        .map_err(|e| describe::ytdlp(&e, tools))?;
    // What an earlier attempt showed goes; this one adds its own parts.
    progress.start_over();
    let formats = info.all_formats();
    let wanted: Vec<&FormatId> = [video, audio].into_iter().flatten().collect();
    let found: Vec<&Format> = wanted
        .iter()
        .filter_map(|id| formats.iter().find(|f| &&f.id == id))
        .collect();
    let ourselves = !wanted.is_empty()
        && found.len() == wanted.len()
        && found
            .iter()
            .all(|f| f.transport() != Transport::YtDlp && f.url.is_some());
    let files = if ourselves {
        match fetch_parts(job, &found, &work, progress, cancel).await? {
            Some(files) => files,
            None => return Ok(MediaDone::Paused),
        }
    } else {
        // A format Wirecatch cannot fetch itself, or the page changed: yt-dlp
        // downloads the same selection. Its own requests are not covered by the
        // local network rule below (see `network_rule`); yt-dlp makes them.
        // yt-dlp joins two formats into the container its own table picks (AV1 with AAC into MP4); the offer named the shared policy's.
        let merge_into = (video.is_some() && audio.is_some())
            .then(|| joined_container(stored_kept_ext(page), job));
        match ytdlp::download(&ctx, format, merge_into, &work, progress, cancel)
            .await
            .map_err(|e| describe::ytdlp(&e, tools))?
        {
            Fetched::Done(file) => vec![file],
            Fetched::Cancelled => return Ok(MediaDone::Paused),
        }
    };
    let mut downloaded = Downloaded::of_media(&files);
    if let Some(output) = &page.output
        && !fetch_extras(job, &info, output, &work, &mut downloaded, cancel).await
    {
        return Ok(MediaDone::Paused);
    }
    downloaded
        .write(&work)
        .await
        .map_err(|e| disk(&e, &job.output))?;
    Ok(save(job, page, tools, &downloaded, &work, progress, cancel).await?)
}

/// Downloads the formats `found` side by side into `work`: their files, in the
/// order found (video first), or `None` when stopped.
async fn fetch_parts(
    job: &Job,
    found: &[&Format],
    work: &Path,
    progress: &MediaProgress,
    cancel: &CancellationToken,
) -> Result<Option<Vec<PathBuf>>, PageError> {
    let mut rules = Vec::with_capacity(found.len());
    for format in found {
        let url = format
            .url
            .as_deref()
            .and_then(|u| Url::parse(u).ok())
            .ok_or_else(|| refresh("The page gave an address for the video that is not valid."))?;
        rules.push(network_rule(job.chosen_by, &job.url, &url).await?);
    }
    let parts = found
        .iter()
        .zip(ROLES)
        .zip(rules)
        .map(|((f, role), rule)| fetch_part(job, f, role, rule, work, progress, cancel));
    let results = futures_join(parts.collect()).await;
    // Stopped wins over any error, and an error a fresh address would not fix
    // wins over a refused address.
    let mut files = Vec::new();
    let mut failed = None;
    let mut refused = None;
    for result in results {
        match result {
            Ok(Some(file)) => files.push(file),
            Ok(None) => return Ok(None),
            Err(PageError::Failed(error)) => failed = failed.or(Some(error)),
            Err(PageError::Refused { status }) => refused = refused.or(Some(status)),
        }
    }
    if let Some(error) = failed {
        return Err(PageError::Failed(error));
    }
    if let Some(status) = refused {
        return Err(PageError::Refused { status });
    }
    Ok(Some(files))
}

/// The list of what a page's video downloaded into its work folder, written
/// once everything is there, so a conversion that is paused or fails starts
/// again from these files without reading the page or downloading anything.
#[derive(Debug, Default, Serialize, Deserialize)]
struct Downloaded {
    /// The video and the separate audio, or the one file.
    media: Vec<String>,
    #[serde(default)]
    subtitles: Vec<DownloadedSubtitle>,
    #[serde(default)]
    cover: Option<(String, ImageFormat)>,
    /// The ffmpeg metadata file (tags, chapters).
    #[serde(default)]
    metadata: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct DownloadedSubtitle {
    file: String,
    format: SubtitleFormat,
    pick: SubtitlePick,
}

/// The list's name in the work folder.
const DOWNLOADED: &str = "downloaded.json";
/// The largest subtitle file taken.
const MAX_SUBTITLE_BYTES: u64 = 20 * MIB;
/// The largest cover picture taken.
const MAX_COVER_BYTES: u64 = 10 * MIB;

impl Downloaded {
    fn of_media(files: &[PathBuf]) -> Self {
        Self {
            media: files
                .iter()
                .filter_map(|f| f.file_name())
                .map(|n| n.to_string_lossy().into_owned())
                .collect(),
            ..Self::default()
        }
    }

    /// Every file name in the list.
    fn names(&self) -> impl Iterator<Item = &str> {
        self.media
            .iter()
            .map(String::as_str)
            .chain(self.subtitles.iter().map(|s| s.file.as_str()))
            .chain(self.cover.iter().map(|c| c.0.as_str()))
            .chain(self.metadata.iter().map(String::as_str))
    }

    /// The list in `work`, with the size of the media files, when it is there
    /// and every file it names is a file straight inside `work`.
    async fn read(work: &Path) -> Option<(Downloaded, u64)> {
        let text = tokio::fs::read(work.join(DOWNLOADED)).await.ok()?;
        let list: Downloaded = serde_json::from_slice(&text).ok()?;
        if list.media.is_empty() || !(1..=2).contains(&list.media.len()) {
            return None;
        }
        for name in list.names() {
            if !plain_name(name) {
                return None;
            }
            let meta = tokio::fs::symlink_metadata(work.join(name)).await.ok()?;
            if !meta.is_file() {
                return None;
            }
        }
        let mut bytes = 0;
        for name in &list.media {
            bytes += tokio::fs::metadata(work.join(name)).await.ok()?.len();
        }
        Some((list, bytes))
    }

    async fn write(&self, work: &Path) -> std::io::Result<()> {
        let json = serde_json::to_vec(self).map_err(std::io::Error::other)?;
        tokio::fs::write(work.join(DOWNLOADED), json).await
    }
}

/// A file name with no folder in it, nor anything Windows reads as a stream or
/// a drive.
fn plain_name(name: &str) -> bool {
    !name.is_empty() && name != "." && name != ".." && !name.contains(['/', '\\', ':', '\0'])
}

/// Downloads what `output` adds to the file besides the video: the chosen
/// subtitles, the cover picture and the metadata file, into `work`, and lists
/// them in `downloaded`. None of it is worth failing the video for: a
/// subtitle or picture that cannot be had is left out with a note in the log.
/// Returns `false` when stopped.
async fn fetch_extras(
    job: &Job,
    info: &ytdlp::Info,
    output: &OutputJob,
    work: &Path,
    downloaded: &mut Downloaded,
    cancel: &CancellationToken,
) -> bool {
    if !fetch_subtitles(job, info, output, work, downloaded, cancel).await {
        return false;
    }
    if output.wants_cover()
        && let Some(url) = info.thumbnail.as_deref().and_then(|u| Url::parse(u).ok())
    {
        let path = work.join("cover.image");
        match fetch_small(job, &url, &HashMap::new(), &path, MAX_COVER_BYTES, cancel).await {
            Fetch::Done => {
                if let Some(format) = ImageFormat::detect(&head(&path).await) {
                    downloaded.cover = Some(("cover.image".into(), format));
                } else {
                    warn!("the video's picture is not a JPEG, PNG or WebP picture");
                }
            }
            Fetch::Stopped => return false,
            Fetch::Failed(reason) => warn!(%reason, "could not download the video's picture"),
        }
    }
    if output.wants_metadata() {
        let tags = output.extras.tags && output.container.tags();
        let title = info.track.as_deref().or(info.title.as_deref());
        let artist = info
            .artist
            .as_deref()
            .or(info.uploader.as_deref())
            .or(info.channel.as_deref());
        let chapters: &[ytdlp::Chapter] = if output.extras.chapters {
            &info.chapters
        } else {
            &[]
        };
        let text = output::ffmetadata(
            title.filter(|_| tags),
            artist.filter(|_| tags),
            chapters,
            output.duration_ms,
        );
        match tokio::fs::write(work.join("metadata.txt"), text).await {
            Ok(()) => downloaded.metadata = Some("metadata.txt".into()),
            Err(e) => warn!(error = %e, "could not write the metadata file"),
        }
    }
    true
}

/// Downloads the subtitles `output` chose into `work` and lists those that
/// are what they claim in `downloaded`; the others are left out with a note
/// in the log. Returns `false` when stopped.
async fn fetch_subtitles(
    job: &Job,
    info: &ytdlp::Info,
    output: &OutputJob,
    work: &Path,
    downloaded: &mut Downloaded,
    cancel: &CancellationToken,
) -> bool {
    for (n, pick) in output.subtitles.iter().enumerate() {
        let Some(file) = output::subtitle_files(info, pick) else {
            warn!(language = %pick.language, "the page no longer offers these subtitles");
            continue;
        };
        let Some(url) = file.url.as_deref().and_then(|u| Url::parse(u).ok()) else {
            continue;
        };
        let ext = file.ext.as_deref().unwrap_or("vtt");
        let path = work.join(format!("sub-{n}.{ext}"));
        match fetch_small(
            job,
            &url,
            &file.http_headers,
            &path,
            MAX_SUBTITLE_BYTES,
            cancel,
        )
        .await
        {
            Fetch::Done => {}
            Fetch::Stopped => return false,
            Fetch::Failed(reason) => {
                warn!(language = %pick.language, %reason, "could not download subtitles");
                continue;
            }
        }
        match SubtitleFormat::detect(&head(&path).await, ext) {
            Some(format) => downloaded.subtitles.push(DownloadedSubtitle {
                file: format!("sub-{n}.{ext}"),
                format,
                pick: pick.clone(),
            }),
            None => {
                warn!(language = %pick.language, "the subtitles are not in the format they claim");
            }
        }
    }
    true
}

/// The first bytes of a file, to recognise its format.
async fn head(path: &Path) -> Vec<u8> {
    use tokio::io::AsyncReadExt;
    let mut buf = vec![0u8; 512];
    let Ok(mut file) = tokio::fs::File::open(path).await else {
        return Vec::new();
    };
    let mut len = 0;
    while len < buf.len() {
        match file.read(&mut buf[len..]).await {
            Ok(0) | Err(_) => break,
            Ok(n) => len += n,
        }
    }
    buf.truncate(len);
    buf
}

/// How downloading a small file ended.
enum Fetch {
    Done,
    Stopped,
    Failed(String),
}

/// Downloads a small file (subtitles, a picture) to `path` under the same rule
/// for the local network as the video, with only its own headers, and stops it
/// once it is larger than `limit` bytes.
async fn fetch_small(
    job: &Job,
    url: &Url,
    own_headers: &HashMap<String, Secret>,
    path: &Path,
    limit: u64,
    cancel: &CancellationToken,
) -> Fetch {
    let rule = match network_rule(job.chosen_by, &job.url, url).await {
        Ok(rule) => rule,
        Err(e) => return Fetch::Failed(e.message),
    };
    let mut options = job.options.clone();
    let list: Vec<Header> = own_headers
        .iter()
        .map(|(name, value)| Header {
            name: name.clone(),
            value: value.clone(),
        })
        .collect();
    options.headers = headers::to_map(&list);
    options.connections = 1;
    options.private_network = rule;
    let mut request = DownloadRequest::new(url.clone(), path.parent().unwrap_or(Path::new(".")));
    request.exact_path = Some(path.to_path_buf());
    request.options = options;
    let progress = Arc::new(Progress::default());
    let stop = cancel.child_token();
    let download = wirecatch_engine::download(request, progress.clone(), stop.clone());
    tokio::pin!(download);
    // A small file needs no progress events: its size is checked a few times a
    // second until it is done.
    let mut check = tokio::time::interval(Duration::from_millis(200));
    let result = loop {
        tokio::select! {
            result = &mut download => break result,
            _ = check.tick() => {
                if progress.done() > limit || progress.total().is_some_and(|t| t > limit) {
                    stop.cancel();
                }
            }
        }
    };
    match result {
        Ok(Outcome::Completed { .. }) => Fetch::Done,
        _ if cancel.is_cancelled() => Fetch::Stopped,
        Ok(_) | Err(EngineError::Cancelled) => {
            Fetch::Failed(format!("it is larger than {} MB", limit / MIB))
        }
        Err(e) => Fetch::Failed(e.to_string()),
    }
}

/// Saves what was downloaded into the file the person chose: as it came
/// (joined, if it came as separate video and audio), or converted by ffmpeg,
/// with subtitle files next to it when they do not go inside.
async fn save(
    job: &Job,
    page: &PageJob,
    tools: &MediaTools,
    downloaded: &Downloaded,
    work: &Path,
    progress: &MediaProgress,
    cancel: &CancellationToken,
) -> Result<MediaDone, UserError> {
    let files: Vec<PathBuf> = downloaded.media.iter().map(|n| work.join(n)).collect();
    let Some(output) = page.output.as_ref() else {
        return match join_original(
            tools,
            &files,
            page.keep_ext.as_deref(),
            job,
            work,
            progress,
            cancel,
        )
        .await?
        {
            Some(file) => finish(&file, job, work, &[]).await,
            None => Ok(MediaDone::Paused),
        };
    };
    let Some(sidecars) = subtitle_files(tools, output, downloaded, work, cancel).await? else {
        return Ok(MediaDone::Paused);
    };
    let file = if output.needs_pass() {
        let inputs = inputs_for(output, downloaded, &files, work)
            .await?
            .map_err(|e| describe::conversion(&e, output, tools))?;
        let target = work.join(format!("output.{}", output.container.ext()));
        let result = convert::convert(tools, output, &inputs, &target, progress, cancel).await;
        progress.set_step(None);
        match result {
            Ok(Finished::Done) => target,
            Ok(Finished::Cancelled) => return Ok(MediaDone::Paused),
            Err(e) => return Err(describe::conversion(&e, output, tools)),
        }
    } else {
        match join_original(
            tools,
            &files,
            Some(output.original_ext.as_str()),
            job,
            work,
            progress,
            cancel,
        )
        .await?
        {
            Some(file) => file,
            None => return Ok(MediaDone::Paused),
        }
    };
    finish(&file, job, work, &sidecars).await
}

/// The downloaded file as it came: the one file, or the video and audio joined
/// (copied, never re-encoded) into a file with extension `ext` (the planned
/// file's when `None`). `None` when stopped.
async fn join_original(
    tools: &MediaTools,
    files: &[PathBuf],
    ext: Option<&str>,
    job: &Job,
    work: &Path,
    progress: &MediaProgress,
    cancel: &CancellationToken,
) -> Result<Option<PathBuf>, UserError> {
    match files {
        [single] => Ok(Some(single.clone())),
        [video, audio] => {
            let container = joined_container(ext, job);
            let joined = work.join(format!("joined.{}", container.ext()));
            progress.set_step(Some(ProcessingStep::Joining));
            let result = merge::join(tools, (video, audio), &joined, container, cancel).await;
            progress.set_step(None);
            match result.map_err(|e| describe::join(&e, tools))? {
                Finished::Done => Ok(Some(joined)),
                Finished::Cancelled => Ok(None),
            }
        }
        _ => Err(addon_missing()),
    }
}

/// The container separately downloaded video and audio are joined into: the one `ext` names (the planned file's extension when `None`), always a video one.
fn joined_container(ext: Option<&str>, job: &Job) -> Container {
    ext.or_else(|| job.output.extension().and_then(|e| e.to_str()))
        .and_then(Container::from_ext)
        .filter(|c| c.is_video())
        .unwrap_or(Container::Mkv)
}

/// The extension stored with a page job for its downloaded streams when they are not converted, as [`media::output::kept_ext`] chose it.
fn stored_kept_ext(page: &PageJob) -> Option<&str> {
    match &page.output {
        Some(output) => Some(output.original_ext.as_str()),
        None => page.keep_ext.as_deref(),
    }
}

/// What ffmpeg reads for `output`: the downloaded media, each read only as the
/// format it was recognised as, and the subtitles, metadata and cover picture.
/// The outer error is the download's state, the inner one a file ffmpeg may not read.
async fn inputs_for(
    output: &OutputJob,
    downloaded: &Downloaded,
    files: &[PathBuf],
    work: &Path,
) -> Result<Result<Inputs, FfmpegError>, UserError> {
    let (video, audio) = match (files, output.video) {
        ([video, audio], _) => (Some(video), Some(audio)),
        ([single], VideoAction::None) => (None, Some(single)),
        ([single], _) => (Some(single), None),
        _ => return Err(addon_missing()),
    };
    let video = match merge::optional_input(video, Role::Video).await {
        Ok(video) => video,
        Err(e) => return Ok(Err(e)),
    };
    let audio = match merge::optional_input(audio, Role::Audio).await {
        Ok(audio) => audio,
        Err(e) => return Ok(Err(e)),
    };
    Ok(Ok(Inputs {
        video,
        audio,
        subtitles: downloaded
            .subtitles
            .iter()
            .map(|s| (s.format, work.join(&s.file), s.pick.clone()))
            .collect(),
        metadata: downloaded.metadata.as_ref().map(|m| work.join(m)),
        cover: downloaded
            .cover
            .as_ref()
            .map(|(f, format)| (*format, work.join(f))),
    }))
}

/// The subtitles saved next to the file, as `SubRip` files in `work` with their
/// language, made with ffmpeg; nothing when they go inside the file. One that
/// ffmpeg cannot read is left out. `None` when stopped.
async fn subtitle_files(
    tools: &MediaTools,
    output: &OutputJob,
    downloaded: &Downloaded,
    work: &Path,
    cancel: &CancellationToken,
) -> Result<Option<Vec<(PathBuf, String)>>, UserError> {
    let mut made = Vec::new();
    if !output.sidecar_subtitles() {
        return Ok(Some(made));
    }
    for (n, subtitle) in downloaded.subtitles.iter().enumerate() {
        let target = work.join(format!("subtitle-{n}.srt"));
        match convert::to_srt(
            tools,
            subtitle.format,
            &work.join(&subtitle.file),
            &target,
            cancel,
        )
        .await
        {
            Ok(Finished::Done) => made.push((target, subtitle.pick.language.clone())),
            Ok(Finished::Cancelled) => return Ok(None),
            Err(e @ FfmpegError::Start(_)) => return Err(describe::conversion(&e, output, tools)),
            // One subtitle ffmpeg cannot read is left out; the video is saved without it.
            Err(e) => {
                warn!(language = %subtitle.pick.language, error = %e, "could not make a subtitle file");
            }
        }
    }
    Ok(Some(made))
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

/// Downloads one format into the work folder: `Ok(None)` when stopped. A part
/// finished in an earlier run or attempt (its `.done` marker names its file) is
/// not downloaded again.
async fn fetch_part(
    job: &Job,
    format: &Format,
    role: &str,
    private_network: PrivateNetwork,
    work: &Path,
    progress: &MediaProgress,
    cancel: &CancellationToken,
) -> Result<Option<PathBuf>, PageError> {
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
    let options = part_options(job, format, private_network);
    let part = work.join(part_name(format, role));
    let file = if format.transport() == Transport::Hls {
        let mut request = StreamRequest::new(url, part, private_network);
        request.ffmpeg = job.tools.as_ref().map(|t| t.ffmpeg.clone());
        request.options = options;
        let stream = Arc::new(StreamProgress::default());
        progress.add_stream(stream.clone());
        // Boxed: a download's state is tens of kilobytes, which every future
        // awaiting this one would otherwise carry inline.
        match Box::pin(wirecatch_streams::download(request, stream, cancel.clone())).await {
            Ok(wirecatch_streams::Outcome::Completed { path, .. }) => path,
            Ok(_) | Err(wirecatch_streams::Error::Cancelled) => return Ok(None),
            Err(e) => {
                return Err(describe::stream(&e, work, job.tools.as_ref(), job.chosen_by).into());
            }
        }
    } else {
        let path = part;
        let mut request = DownloadRequest::new(url, work);
        request.exact_path = Some(path.clone());
        request.options = options;
        let file = Arc::new(Progress::default());
        progress.add_file(file.clone(), format.size());
        match Box::pin(wirecatch_engine::download(request, file, cancel.clone())).await {
            Ok(Outcome::Completed { .. }) => path,
            Ok(_) | Err(EngineError::Cancelled) => return Ok(None),
            Err(e) => {
                let folder = job.output.parent().unwrap_or(Path::new(""));
                // A format address that was refused or has expired: a
                // fresh one from the page usually works.
                if let EngineError::PrivateNetwork { .. } = e {
                    return Err(user_error::private_network(job.chosen_by).into());
                }
                let error = user_error::describe(&e, folder, true);
                if error.fix == Some(FixAction::RefreshLink) {
                    let status = match e {
                        EngineError::HttpStatus { status, .. } => Some(status),
                        _ => None,
                    };
                    return Err(PageError::Refused { status });
                }
                return Err(error.into());
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
/// planned one and the name is taken, a free name is used. Subtitle files
/// (`sidecars`, each with its language) go next to it as
/// `<name>.<language>.srt`, never replacing a file.
async fn finish(
    file: &Path,
    job: &Job,
    work: &Path,
    sidecars: &[(PathBuf, String)],
) -> Result<MediaDone, UserError> {
    let output = &job.output;
    let (file, planned, work) = (file.to_path_buf(), output.clone(), work.to_path_buf());
    let (url, referrer) = (
        job.url.to_string(),
        headers::referrer(&job.origin.headers).map(str::to_string),
    );
    let reserve = job.reserve.clone();
    let sidecars = sidecars.to_vec();
    let moved = tokio::task::spawn_blocking(move || -> std::io::Result<(PathBuf, u64)> {
        // Marked before it is put in place, so it is never there unmarked.
        crate::zone::mark(&file, &url, referrer.as_deref());
        let target = place(&file, &planned, &reserve)?;
        let bytes = std::fs::metadata(&target)?.len();
        let anywhere: Reserve = Arc::new(|_: &Path, _| true);
        for (subtitle, language) in &sidecars {
            crate::zone::mark(subtitle, &url, referrer.as_deref());
            let beside = sidecar_path(&target, language);
            if let Err(e) = place(subtitle, &beside, &anywhere) {
                warn!(error = %e, "could not put a subtitle file next to the video");
            }
        }
        crate::discard::dir(&work);
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

/// Where a subtitle file in `language` goes next to `file`:
/// `<file's name>.<language>.srt`, with the language reduced to letters,
/// digits and dashes.
fn sidecar_path(file: &Path, language: &str) -> PathBuf {
    let stem = file
        .file_stem()
        .map_or_else(|| "video".into(), |s| s.to_string_lossy().into_owned());
    let language: String = language
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-')
        .take(35)
        .collect();
    let language = if language.is_empty() {
        "und".into()
    } else {
        language
    };
    file.with_file_name(format!("{stem}.{language}.srt"))
}

/// The request options for one format: the format's own headers only (the
/// page's cookies stay with the page), bound to its own address, the local
/// network rule decided for it, and requests no larger than the format's
/// server allows at full speed (yt-dlp's `http_chunk_size`).
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
        .map(|(_, v)| v.expose().to_owned())
    {
        options.user_agent = agent;
    }
    options.private_network = rule;
    options.max_request_bytes = format.request_limit();
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
                crate::discard::file(file);
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
/// whose name does not resolve (so cannot be checked), is refused early with a
/// clear message, and the rest download with the local network blocked for
/// every request, redirect hop and connection. That block, not this early
/// check, is what keeps a format out of the local network, so a redirect or a
/// name that answers local only when the engine connects (DNS rebinding) is
/// refused too.
///
/// When yt-dlp downloads a format by itself (the fallback), its own requests
/// are not covered by this.
async fn network_rule(
    chosen_by: ChosenBy,
    page: &Url,
    format: &Url,
) -> Result<PrivateNetwork, UserError> {
    if page_is_local(chosen_by, page).await {
        return Ok(PrivateNetwork::Allow);
    }
    format_rule(chosen_by, &local_network::addresses(format, LOOKUP).await)
}

/// How long the early check waits for a format's name to resolve.
const LOOKUP: Duration = Duration::from_secs(10);

/// Whether a video page is on the local network. A page the person chose is
/// where its name leads for them, as [`PrivateNetwork::Auto`] decides for a
/// link they typed. A page on their network by its own address or their list
/// of sites (D39) is. A website's page never is: where its name resolves is
/// for the site's owner to answer, who could make it answer local for this
/// one lookup (CORE-3).
async fn page_is_local(chosen_by: ChosenBy, page: &Url) -> bool {
    match chosen_by {
        ChosenBy::Person => {
            let found = local_network::addresses(page, LOOKUP).await;
            !found.is_empty() && found.into_iter().all(local_network::is_local)
        }
        ChosenBy::LocalPage => true,
        ChosenBy::Website => false,
    }
}

/// The rule for a format of a page on the internet, chosen by `chosen_by`,
/// whose host has the addresses `found`.
fn format_rule(chosen_by: ChosenBy, found: &[IpAddr]) -> Result<PrivateNetwork, UserError> {
    if found.is_empty() {
        return Err(UserError {
            message: "The server of this video's file could not be found, so Wirecatch could not check where it is and did not download it. Check your connection, then try again.".into(),
            fix: Some(FixAction::Retry),
        });
    }
    if found.iter().copied().any(local_network::is_local) {
        return Err(user_error::private_network(chosen_by));
    }
    Ok(PrivateNetwork::Block)
}

/// The error once every automatic try with fresh addresses was refused too.
fn refused_after_refreshing(status: Option<u16>) -> UserError {
    let answered = status
        .map(|s| format!(" (it answered {s})"))
        .unwrap_or_default();
    UserError {
        message: format!(
            "The video's server kept refusing to send the video{answered}, even though Wirecatch tried again by itself several times with fresh addresses from the page. Sites sometimes refuse one video for a while. Try again later; what was already downloaded is kept."
        ),
        fix: Some(FixAction::Retry),
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
    user_error::disk(error, folder)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn formats_of_a_page_on_the_internet_stay_on_the_internet() {
        let url = |u: &str| Url::parse(u).unwrap();
        let public_page = url("http://93.184.215.14/watch");
        // A page at home may point anywhere: one the person chose, or one on
        // their network by its own address (D39).
        for chosen_by in [ChosenBy::Person, ChosenBy::LocalPage] {
            assert_eq!(
                network_rule(
                    chosen_by,
                    &url("http://127.0.0.1:9/watch"),
                    &url("http://192.168.1.10/v.mp4")
                )
                .await
                .unwrap(),
                PrivateNetwork::Allow
            );
        }
        // A page on the internet: public formats download with the local
        // network blocked for every hop and connection.
        assert_eq!(
            network_rule(
                ChosenBy::Website,
                &public_page,
                &url("http://93.184.215.15/v.mp4")
            )
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
            let error = network_rule(ChosenBy::Website, &public_page, &url(local))
                .await
                .unwrap_err();
            assert!(
                error.message.contains("your own network"),
                "{local}: {}",
                error.message
            );
        }
        // A name that does not resolve cannot be checked, so it is refused.
        let error = network_rule(
            ChosenBy::Website,
            &public_page,
            &url("http://no-such-host.invalid/v.mp4"),
        )
        .await
        .unwrap_err();
        assert!(
            error.message.contains("could not be found"),
            "{}",
            error.message
        );
    }

    /// A website's page gets no exception for being written as a local
    /// address: whether the page is local was decided where it was captured,
    /// from the page it came from (D39).
    #[tokio::test]
    async fn a_websites_page_is_never_local() {
        let url = |u: &str| Url::parse(u).unwrap();
        let error = network_rule(
            ChosenBy::Website,
            &url("http://127.0.0.1:9/watch"),
            &url("http://192.168.1.10/v.mp4"),
        )
        .await
        .unwrap_err();
        assert!(
            error.message.contains("your own network"),
            "{}",
            error.message
        );
    }

    /// Where a page's name resolves counts only for a page the person chose:
    /// a website's owner decides what its name answers (CORE-3).
    #[tokio::test]
    async fn only_the_persons_page_is_local_by_its_name() {
        let url = |u: &str| Url::parse(u).unwrap();
        let page = url("http://localhost/watch");
        let format = url("http://192.168.1.10/v.mp4");
        assert_eq!(
            network_rule(ChosenBy::Person, &page, &format)
                .await
                .unwrap(),
            PrivateNetwork::Allow
        );
        let error = network_rule(ChosenBy::Website, &page, &format)
            .await
            .unwrap_err();
        assert!(
            error.message.contains("your own network"),
            "{}",
            error.message
        );
    }

    /// Core and the engine agree on what is local (CORE-2): a page on a
    /// carrier-grade NAT address (a Tailscale media server) is local, and a
    /// format at a NAT64 or 6to4 address that carries a local one is refused.
    #[tokio::test]
    async fn every_kind_of_local_address_counts_as_local() {
        let url = |u: &str| Url::parse(u).unwrap();
        assert_eq!(
            network_rule(
                ChosenBy::Person,
                &url("http://100.100.100.100/watch"),
                &url("http://192.168.1.10/v.mp4")
            )
            .await
            .unwrap(),
            PrivateNetwork::Allow
        );
        let public_page = url("http://93.184.215.14/watch");
        for local in [
            "http://[64:ff9b::c0a8:101]/v.mp4",
            "http://[2002:c0a8:101::1]/v.mp4",
            "http://100.64.0.1/v.mp4",
        ] {
            let error = network_rule(ChosenBy::Website, &public_page, &url(local))
                .await
                .unwrap_err();
            assert!(error.message.contains("your own network"), "{local}");
        }
    }

    /// A format's name that answered public to the early check and answers
    /// local when the engine looks again is still refused, by the engine
    /// under the rule core hands on: the early check only ever refuses. (The
    /// refusal at the connection itself is the engine's own test,
    /// `net_guard::tests::a_download_cannot_be_rebound_to_the_local_network`.)
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_format_rebound_to_the_local_network_after_the_check_is_refused() {
        let server = wirecatch_test_support::files::FileServer::start().await;
        server.put("v.mp4", b"video".as_slice());
        let mut format = server.url("v.mp4");
        format.set_host(Some("localhost")).unwrap();
        // What the check saw: a public address.
        let rule = format_rule(ChosenBy::Website, &["93.184.215.15".parse().unwrap()]).unwrap();
        let dir = tempfile::tempdir().unwrap();
        let mut request = DownloadRequest::new(format, dir.path());
        request.options.private_network = rule;
        let result = wirecatch_engine::download(
            request,
            Arc::new(Progress::default()),
            CancellationToken::new(),
        )
        .await;
        assert!(
            matches!(result, Err(EngineError::PrivateNetwork { .. })),
            "{result:?}"
        );
        assert_eq!(server.requests("v.mp4"), 0);
    }

    /// A stream job runs under the rule of whoever chose its link.
    #[test]
    fn a_stream_downloads_under_the_rule_of_whoever_chose_it() {
        for (chosen_by, rule) in [
            (ChosenBy::Website, PrivateNetwork::Block),
            (ChosenBy::LocalPage, PrivateNetwork::Auto),
            (ChosenBy::Person, PrivateNetwork::Auto),
        ] {
            let job = Job {
                output: PathBuf::from("/v/out.mp4"),
                url: Url::parse("https://videos.example.com/master.m3u8").unwrap(),
                origin: Origin::default(),
                chosen_by,
                tools: None,
                options: DownloadOptions::default(),
                temp_dir: PathBuf::new(),
                timeout: Duration::from_secs(1),
                refresh_delays: Arc::new([]),
                clock: Arc::new(crate::clock::SystemClock),
                reserve: Arc::new(|_: &Path, _| true),
            };
            let request = stream_request(&job, Path::new("/v/w/out"), Pick::Auto, Pick::Auto);
            assert_eq!(request.private_network, rule, "{chosen_by:?}");
        }
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
            chosen_by: ChosenBy::Website,
            tools: None,
            options: DownloadOptions::default(),
            temp_dir: PathBuf::new(),
            timeout: Duration::from_secs(1),
            refresh_delays: Arc::new([]),
            clock: Arc::new(crate::clock::SystemClock),
            reserve: Arc::new(|_: &Path, _| true),
        };
        let options = part_options(&job, &format, PrivateNetwork::Block);
        assert_eq!(options.private_network, PrivateNetwork::Block);
        assert_eq!(options.headers.get("x-format").unwrap(), "yes");
        assert_eq!(options.max_request_bytes, None);

        // yt-dlp's chunk size becomes the engine's limit on one request.
        let chunked: Format = serde_json::from_value(serde_json::json!({
            "format_id": "137", "ext": "mp4",
            "downloader_options": {"http_chunk_size": 10_485_760}
        }))
        .unwrap();
        let options = part_options(&job, &chunked, PrivateNetwork::Block);
        assert_eq!(options.max_request_bytes, Some(10_485_760));
    }

    #[tokio::test]
    async fn a_job_that_cannot_be_read_never_runs() {
        let job = Job {
            output: PathBuf::from("/v/out.mp4"),
            url: Url::parse("http://127.0.0.1:9/watch").unwrap(),
            origin: Origin::default(),
            chosen_by: ChosenBy::Website,
            tools: None,
            options: DownloadOptions::default(),
            temp_dir: PathBuf::new(),
            timeout: Duration::from_secs(1),
            refresh_delays: Arc::new([]),
            clock: Arc::new(crate::clock::SystemClock),
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

    /// A stream (here one HLS segment, joined without ffmpeg) is saved in the
    /// work folder, marked, and only then put at its final name: the final
    /// file never exists without the mark.
    #[cfg(windows)]
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_finished_stream_is_marked_before_it_is_put_in_place() {
        // MPEG-TS packets: 188 bytes each, starting with the sync byte.
        let segment: Vec<u8> = (0..188 * 40)
            .map(|i| {
                if i % 188 == 0 {
                    0x47
                } else {
                    u8::try_from(i % 251).unwrap()
                }
            })
            .collect();
        let server = wirecatch_test_support::files::FileServer::start().await;
        server.put(
            "index.m3u8",
            "#EXTM3U\n#EXT-X-VERSION:3\n#EXT-X-TARGETDURATION:4\n#EXTINF:4.0,\nseg0.ts\n#EXT-X-ENDLIST\n",
        );
        server.put("seg0.ts", segment);

        let dir = tempfile::tempdir().unwrap();
        let output = dir.path().join("clip.mp4");
        let job = Job {
            output: output.clone(),
            url: server.url("index.m3u8"),
            origin: Origin::default(),
            chosen_by: ChosenBy::Website,
            tools: None,
            options: DownloadOptions::default(),
            temp_dir: PathBuf::new(),
            timeout: Duration::from_secs(5),
            refresh_delays: Arc::new([]),
            clock: Arc::new(crate::clock::SystemClock),
            reserve: Arc::new(|_: &Path, _| true),
        };
        let done = run_stream(
            &job,
            Pick::Auto,
            Pick::Auto,
            &MediaProgress::default(),
            &CancellationToken::new(),
        )
        .await
        .unwrap();
        let MediaDone::Completed { path, .. } = done else {
            panic!("not completed: {done:?}");
        };
        assert_eq!(path.parent(), output.parent(), "{}", path.display());
        let mark = std::fs::read_to_string(format!("{}:Zone.Identifier", path.display()))
            .expect("the stream was put in place without the mark");
        assert!(mark.contains("ZoneId=3"), "{mark}");
        assert!(
            !media::work_dir(&output).exists(),
            "the work folder was left"
        );
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
            chosen_by: ChosenBy::Website,
            tools: None,
            options: DownloadOptions::default(),
            temp_dir: PathBuf::new(),
            timeout: Duration::from_secs(1),
            refresh_delays: Arc::new([]),
            clock: Arc::new(crate::clock::SystemClock),
            reserve: Arc::new(|_: &Path, _| true),
        };
        let done = finish(&part, &job, &work, &[]).await.unwrap();
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
}
