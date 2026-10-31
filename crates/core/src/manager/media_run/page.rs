//! A page's formats: read with yt-dlp, each part downloaded by the engine or `wirecatch-streams`.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use tracing::warn;
use wirecatch_engine::{
    CancellationToken, DownloadOptions, DownloadRequest, Error as EngineError, Outcome,
    PrivateNetwork, Progress, Url,
};
use wirecatch_media::describe;
use wirecatch_media::ytdlp::{self, Fetched, Format, FormatId, Transport};
use wirecatch_media::{Content, MediaProgress, safe_ext};
use wirecatch_protocol::app::UserError;
use wirecatch_protocol::extension::Header;
use wirecatch_streams::{StreamProgress, StreamRequest};

use super::extras::fetch_extras;
use super::network::network_rule;
use super::save::{joined_container, save, stored_kept_ext};
use super::workdir::Downloaded;
use super::{Job, PageError, PageJob, ROLES, disk, refresh, refused_after_refreshing, workdir};
use crate::headers;
use crate::manager::media::{addon_missing, cookie_header};
use crate::manager::run::MediaDone;

/// Downloads a page's chosen formats. When the server refuses a format's
/// address, the page is read again for fresh addresses and the download goes on
/// by itself after each of [`Job::refresh_delays`], keeping the parts already
/// finished; only when every try is used up does the person see an error.
pub(super) async fn run_page(
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
        progress.set_retry_at(Some(job.clock.unix_millis().saturating_add(*delay)));
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
pub(super) async fn page_attempt(
    job: &Job,
    page: &PageJob,
    progress: &MediaProgress,
    cancel: &CancellationToken,
) -> Result<MediaDone, PageError> {
    let tools = job.tools.as_ref().ok_or_else(addon_missing)?;
    let work = wirecatch_media::work_dir(&job.output);
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
    let proxy = wirecatch_settings::proxy::ytdlp_proxy(&job.options.proxy);
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
pub(super) async fn fetch_parts(
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
    let results = futures_util::future::join_all(parts).await;
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

/// Downloads one format into the work folder: `Ok(None)` when stopped. A part
/// finished in an earlier run or attempt (its `.done` marker names its file) is
/// not downloaded again.
pub(super) async fn fetch_part(
    job: &Job,
    format: &Format,
    role: &str,
    private_network: PrivateNetwork,
    work: &Path,
    progress: &MediaProgress,
    cancel: &CancellationToken,
) -> Result<Option<PathBuf>, PageError> {
    let marker = work.join(workdir::marker(role));
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
        request.file_work = job.file_work.clone();
        let file = Arc::new(Progress::default());
        progress.add_file(file.clone(), format.size());
        match Box::pin(wirecatch_engine::download(request, file, cancel.clone())).await {
            Ok(Outcome::Completed { .. }) => path,
            Ok(_) | Err(EngineError::Cancelled) => return Ok(None),
            Err(EngineError::PrivateNetwork { .. }) => {
                return Err(wirecatch_messages::private_network(job.chosen_by).into());
            }
            Err(e) if address_gone(&e) => {
                let status = match e {
                    EngineError::HttpStatus { status, .. } => Some(status),
                    _ => None,
                };
                return Err(PageError::Refused { status });
            }
            Err(e) => {
                let folder = job.output.parent().unwrap_or(Path::new(""));
                let error =
                    wirecatch_messages::describe(&e, folder, wirecatch_messages::Worked::Before);
                return Err(error.into());
            }
        }
    };
    let name = crate::naming::file_name_of(&file);
    tokio::fs::write(&marker, name)
        .await
        .map_err(|e| disk(&e, &job.output))?;
    Ok(Some(file))
}

/// A format address the server refused, or one that no longer leads to the file: a fresh one
/// from the page usually works.
pub(super) fn address_gone(e: &EngineError) -> bool {
    matches!(
        e,
        EngineError::HttpStatus {
            status: 401 | 403 | 404 | 410,
            ..
        } | EngineError::InvalidUrl(_)
            | EngineError::TooManyRedirects { .. }
            | EngineError::RedirectWithoutLocation { .. }
            | EngineError::TargetRejected(_)
    )
}

/// The request options for one format: the format's own headers only (the
/// page's cookies stay with the page), bound to its own address, the local
/// network rule decided for it, and requests no larger than the format's
/// server allows at full speed (yt-dlp's `http_chunk_size`).
pub(super) fn part_options(job: &Job, format: &Format, rule: PrivateNetwork) -> DownloadOptions {
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
    if let Some(agent) = headers::find(&format_headers, "user-agent") {
        agent.clone_into(&mut options.user_agent);
    }
    options.private_network = rule;
    options.max_request_bytes = format.request_limit();
    options
}

/// The name of a part in the work folder: its role and a media extension,
/// never one the site chose otherwise.
pub(super) fn part_name(format: &Format, role: &str) -> String {
    let content = if format.has_video() {
        Content::Video
    } else {
        Content::Audio
    };
    format!("{role}.{}", safe_ext(format.ext.as_deref(), content))
}
