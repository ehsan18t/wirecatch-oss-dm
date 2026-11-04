//! A page's formats: read with yt-dlp, each part downloaded by the engine or `wirecatch-streams`.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use tracing::warn;
use wirecatch_engine::{
    CancellationToken, DownloadOptions, DownloadRequest, Error as EngineError, Outcome,
    PrivateNetwork, Progress, Url,
};
use wirecatch_media::ytdlp::{self, Fetched, Format, FormatId, Info, Transport, YtDlpError};
use wirecatch_media::{Content, MediaProgress, choice, describe, safe_ext};
use wirecatch_protocol::app::UserError;
use wirecatch_protocol::extension::Header;
use wirecatch_streams::{StreamProgress, StreamRequest};

use super::extras::fetch_extras;
use super::network::{network_rule, ytdlp_off_local_network};
use super::save::{joined_container, save, stored_kept_ext};
use super::workdir::Downloaded;
use super::{Job, PageError, PageJob, ROLES, disk, workdir};
use crate::headers;
use crate::manager::media::cookie_header;
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
            return Err(describe::refused_after_refreshing(status));
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
    let tools = job.tools.as_ref().ok_or_else(describe::addon_missing)?;
    let work = wirecatch_media::work_dir(&job.output);
    workdir::claim_for_link(&work, &job.url)
        .await
        .map_err(|e| disk(&e, &job.output))?;
    if let Some((downloaded, bytes)) = Downloaded::read(&work).await {
        progress.start_over();
        progress.add_done(bytes);
        return Ok(save(job, page, tools, &downloaded, &work, progress, cancel).await?);
    }
    let cookies = cookie_header(&job.origin);
    let proxy = wirecatch_settings::proxy::ytdlp_proxy(&job.options.proxy, &job.url);
    let ctx = ytdlp::Context {
        off_local_network: off_local_network(job, proxy.as_deref()).await,
        tools,
        page: &job.url,
        cookies: cookies.as_deref(),
        user_agent: job.origin.user_agent.as_deref(),
        temp_dir: &job.temp_dir,
        timeout: job.timeout,
        proxy: proxy.as_deref(),
        rate_limit: tightest_limit(job),
    };
    // Format addresses expire: read the page again on every start.
    let info = ytdlp::extract(&ctx, cancel)
        .await
        .map_err(|e| describe::ytdlp(&e, tools))?;
    // What an earlier attempt showed goes; this one adds its own parts.
    progress.start_over();
    let (video, audio) = &formats_now(job, page, &info, &work).await?;
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
        // A format Wirecatch cannot fetch itself: yt-dlp downloads the same
        // selection, under the rule of its own (D136).
        if !cookie_formats_pass_the_rule(job, &found, cancel).await? {
            return Ok(MediaDone::Paused);
        }
        // yt-dlp joins two formats into the container its own table picks (AV1 with AAC into MP4); the offer named the shared policy's.
        let merge_into = (video.is_some() && audio.is_some())
            .then(|| joined_container(stored_kept_ext(page), job));
        progress.expect_own(formats_ytdlp_takes(&info, &found).into_iter());
        let format = ytdlp::selection(video.as_ref(), audio.as_ref());
        match ytdlp::download(&ctx, &format, merge_into, &work, progress, cancel).await {
            Ok(Fetched::Done(file)) => vec![file],
            Ok(Fetched::Cancelled) => return Ok(MediaDone::Paused),
            Err(YtDlpError::AddressRefused { status, .. }) => {
                return Err(PageError::Refused { status });
            }
            Err(e) => return Err(describe::ytdlp(&e, tools).into()),
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

/// The formats this attempt downloads: the saved ones while the page lists them, else what it offers now at the row's quality. A page with none of that quality ends the download, since no try can change it.
async fn formats_now(
    job: &Job,
    page: &PageJob,
    info: &Info,
    work: &Path,
) -> Result<(Option<FormatId>, Option<FormatId>), PageError> {
    let listed = info.all_formats();
    let lists = |id: &FormatId| listed.iter().any(|f| &f.id == id);
    if [&page.video, &page.audio].into_iter().flatten().all(lists) {
        workdir::claim_for_formats(work, &job.url, None)
            .await
            .map_err(|e| disk(&e, &job.output))?;
        return Ok((page.video.clone(), page.audio.clone()));
    }
    let (video, audio) = choice::formats_of_quality(info, &job.quality)
        .ok_or_else(|| describe::quality_gone(&job.quality))?;
    let ids = ytdlp::ids(video.as_ref(), audio.as_ref());
    warn!(%ids, "the page no longer lists the saved formats; taking its formats of the same quality");
    workdir::claim_for_formats(work, &job.url, Some(&ids))
        .await
        .map_err(|e| disk(&e, &job.output))?;
    Ok((video, audio))
}

/// The formats yt-dlp downloads and their sizes where the page says: those `found`, or with none named, what its best selection takes.
pub(super) fn formats_ytdlp_takes(info: &Info, found: &[&Format]) -> Vec<(String, Option<u64>)> {
    if found.is_empty() {
        return info.best_formats();
    }
    found.iter().map(|f| (f.id.to_string(), f.size())).collect()
}

/// The one rate a yt-dlp process takes (D135).
fn tightest_limit(job: &Job) -> Option<u64> {
    job.options
        .rate_limiters
        .iter()
        .filter_map(|limiter| limiter.limit())
        .min()
}

/// Gives the addresses of formats with cookies the early check of the rule: the engine would have taken them but for the cookies. `false` when stopped.
async fn cookie_formats_pass_the_rule(
    job: &Job,
    found: &[&Format],
    cancel: &CancellationToken,
) -> Result<bool, UserError> {
    for url in found
        .iter()
        .filter(|f| f.cookies.is_some())
        .filter_map(|f| Url::parse(f.url.as_deref()?).ok())
    {
        let Some(rule) = cancel
            .run_until_cancelled(network_rule(job.chosen_by, &job.url, &url))
            .await
        else {
            return Ok(false);
        };
        rule?;
    }
    Ok(true)
}

async fn off_local_network(job: &Job, proxy: Option<&str>) -> bool {
    ytdlp_off_local_network(job.chosen_by, &job.url, proxy).await
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
            .ok_or_else(describe::invalid_format_address)?;
        let Some(rule) = cancel
            .run_until_cancelled(network_rule(job.chosen_by, &job.url, &url))
            .await
        else {
            return Ok(None);
        };
        rules.push(rule?);
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
        .ok_or_else(describe::invalid_format_address)?;
    let options = part_options(job, format, private_network);
    let part = work.join(part_name(format, role));
    let file = if format.transport() == Transport::Hls {
        let mut request = StreamRequest::new(url, part, private_network);
        request.ffmpeg = job.tools.as_ref().map(|t| t.ffmpeg.clone());
        request.options = options;
        // A format's address moves between CDNs when the page is read again; the page's link stays.
        request.work_owner.clone_from(&job.work_owner);
        request.file_work = job.file_work.clone();
        let stream = Arc::new(StreamProgress::default());
        progress.add_stream(stream.clone());
        // Boxed: a download's state is tens of kilobytes, which every future
        // awaiting this one would otherwise carry inline.
        match Box::pin(wirecatch_streams::download(request, stream, cancel.clone())).await {
            Ok(wirecatch_streams::Outcome::Completed { path, .. }) => path,
            Ok(_) | Err(wirecatch_streams::Error::Cancelled) => return Ok(None),
            Err(wirecatch_streams::Error::Request(e)) if address_gone(&e) => {
                return Err(refused(&e));
            }
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
        match Box::pin(wirecatch_engine::download(
            request,
            file.clone(),
            cancel.clone(),
        ))
        .await
        {
            Ok(Outcome::Completed { .. }) => {
                if holds_no_media(job, &path).await? {
                    return Err(PageError::Refused { status: None });
                }
                progress.finish_file(&file);
                path
            }
            Ok(_) | Err(EngineError::Cancelled) => return Ok(None),
            Err(EngineError::PrivateNetwork { .. }) => {
                return Err(wirecatch_messages::private_network(job.chosen_by).into());
            }
            Err(e) if address_gone(&e) => return Err(refused(&e)),
            Err(e) => {
                let folder = job.output.parent().unwrap_or(Path::new(""));
                let worked = if progress.keeps_data() {
                    wirecatch_messages::Worked::Before
                } else if progress.read().answered {
                    wirecatch_messages::Worked::BeforeStartsOver
                } else {
                    wirecatch_messages::Worked::Never
                };
                return Err(wirecatch_messages::describe(&e, folder, worked).into());
            }
        }
    };
    let name = crate::naming::file_name_of(&file);
    tokio::fs::write(&marker, name)
        .await
        .map_err(|e| disk(&e, &job.output))?;
    Ok(Some(file))
}

/// Whether the format saved at `path` is nothing or a web page, as an address that stopped working can answer; such a file is removed, so a fresh address starts over. Refusing the other formats here would turn away what ffmpeg is never asked to read.
async fn holds_no_media(job: &Job, path: &Path) -> Result<bool, PageError> {
    let file = path.to_path_buf();
    let checked = job
        .file_work
        .spawn_blocking(move || {
            let empty = wirecatch_streams::holds_no_media(&file)?;
            if empty {
                std::fs::remove_file(&file)?;
            }
            Ok::<bool, std::io::Error>(empty)
        })
        .await
        .unwrap_or_else(|e| Err(std::io::Error::other(e)));
    checked.map_err(|e| disk(&e, &job.output).into())
}

/// A format address the server refused, or one that no longer leads to the file: a fresh one
/// from the page usually works.
pub(super) fn address_gone(e: &EngineError) -> bool {
    match e {
        EngineError::HttpStatus { status, .. } => ytdlp::REFUSED_ADDRESS_STATUSES.contains(status),
        EngineError::InvalidUrl(_)
        | EngineError::TooManyRedirects { .. }
        | EngineError::RedirectWithoutLocation { .. }
        | EngineError::TargetRejected(_) => true,
        _ => false,
    }
}

/// The attempt's end for `e`, an [`address_gone`] error.
fn refused(e: &EngineError) -> PageError {
    let status = match e {
        EngineError::HttpStatus { status, .. } => Some(*status),
        _ => None,
    };
    PageError::Refused { status }
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
