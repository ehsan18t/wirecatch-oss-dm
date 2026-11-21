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
use crate::manager::failure_log::{Details, MediaFailure};
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
) -> Result<MediaDone, MediaFailure> {
    let mut delays = job.refresh_delays.iter();
    let mut delegated = false;
    loop {
        let attempt = page_attempt(job, page, progress, cancel, delegated).await;
        // A reading serves only a pause: a refusal needs fresh addresses, and an end needs none.
        if !matches!(attempt, Ok(MediaDone::Paused) | Err(PageError::Delegate)) {
            job.readings.forget(job.reading_owner);
        }
        let details = match attempt {
            Ok(done) => return Ok(done),
            Err(PageError::Failed(error)) => return Err(error),
            Err(PageError::Refused(details)) => details,
            Err(PageError::Delegate) => {
                delegated = true;
                continue;
            }
        };
        let Some(delay) = delays.next() else {
            let error = describe::refused_after_refreshing(details.status);
            return Err(details.with_error(error));
        };
        warn!(
            status = details.status,
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
/// everything (its list is in the work folder), the page's format addresses come from a reading of the last [`READING_LIFETIME`](crate::manager::page_readings::READING_LIFETIME), else the page is read again,
/// and what is not finished yet is downloaded: the formats,
/// and the subtitles, cover picture and metadata the output wants. Then the
/// file is saved; a conversion that was paused or failed starts again from the
/// downloaded parts, never from the download.
async fn page_attempt(
    job: &Job,
    page: &PageJob,
    progress: &MediaProgress,
    cancel: &CancellationToken,
    delegated: bool,
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
        browser: job.browser.as_ref(),
        tools,
        page: &job.url,
        cookies: cookies.as_deref(),
        user_agent: job.origin.user_agent.as_deref(),
        temp_dir: &job.temp_dir,
        timeout: job.timeout,
        proxy: proxy.as_deref(),
        rate_limit: tightest_limit(job),
    };
    let info = reading(job, &ctx, cancel).await?;
    let source = Formats {
        work: &work,
        ctx: &ctx,
        info: &info,
    };
    download_formats(job, page, source, progress, cancel, delegated)
        .await
        .map_err(|mut err| {
            let details = match &mut err {
                PageError::Refused(details) => details,
                PageError::Failed(failure) => &mut failure.details,
                PageError::Delegate => return err,
            };
            details.extractor.clone_from(&info.extractor_key);
            err
        })
}

struct Formats<'a> {
    work: &'a Path,
    ctx: &'a ytdlp::Context<'a>,
    info: &'a Info,
}

async fn download_formats(
    job: &Job,
    page: &PageJob,
    source: Formats<'_>,
    progress: &MediaProgress,
    cancel: &CancellationToken,
    delegated: bool,
) -> Result<MediaDone, PageError> {
    let Formats { work, ctx, info } = source;
    let tools = ctx.tools;
    // What an earlier attempt showed goes; this one adds its own parts.
    progress.start_over();
    let (video, audio) = &formats_now(job, page, info, work).await?;
    let formats = info.all_formats();
    let wanted: Vec<&FormatId> = [video, audio].into_iter().flatten().collect();
    let found: Vec<&Format> = wanted
        .iter()
        .filter_map(|id| formats.iter().find(|f| &&f.id == id))
        .collect();
    let ourselves = !delegated
        && !wanted.is_empty()
        && found.len() == wanted.len()
        && found.iter().all(|format| {
            format.url.is_some()
                && (format.transport() != Transport::YtDlp
                    || format.native_cookies(&job.suffixes).is_some())
        });
    let files = if ourselves {
        match fetch_parts(job, &found, work, progress, cancel).await? {
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
        progress.expect_own(formats_ytdlp_takes(info, &found).into_iter());
        let format = ytdlp::selection(video.as_ref(), audio.as_ref());
        match ytdlp::download(ctx, &format, merge_into, work, progress, cancel).await {
            Ok(Fetched::Done(file)) => vec![file],
            Ok(Fetched::Cancelled) => return Ok(MediaDone::Paused),
            Err(YtDlpError::AddressRefused { status, .. }) => {
                let mut details = Details::new("format_transfer", "yt_dlp");
                details.status = status;
                return Err(PageError::Refused(details));
            }
            Err(e) => {
                return Err(PageError::Failed(
                    Details::new("format_transfer", "yt_dlp")
                        .with_error(describe::ytdlp(&e, tools)),
                ));
            }
        }
    };
    let mut downloaded = Downloaded::of_media(&files);
    if let Some(output) = &page.output
        && !fetch_extras(job, info, output, work, &mut downloaded, cancel).await
    {
        return Ok(MediaDone::Paused);
    }
    downloaded
        .write(work)
        .await
        .map_err(|e| disk(&e, &job.output))?;
    save(job, page, tools, &downloaded, work, progress, cancel)
        .await
        .map_err(|e| PageError::Failed(Details::new("save", "media").with_error(e)))
}

/// The page's recent reading, else a new one, kept for the next start.
async fn reading(
    job: &Job,
    ctx: &ytdlp::Context<'_>,
    cancel: &CancellationToken,
) -> Result<Arc<Info>, PageError> {
    if let Some(info) = job
        .readings
        .fresh(job.reading_owner, job.clock.unix_millis())
    {
        return Ok(info);
    }
    let info = ytdlp::extract(ctx, cancel).await.map_err(|e| {
        PageError::Failed(
            Details::new("page_read", "yt_dlp").with_error(describe::ytdlp(&e, ctx.tools)),
        )
    })?;
    let info = Arc::new(info);
    job.readings.put(
        job.reading_owner,
        job.clock.unix_millis(),
        info.clone(),
        cancel,
    );
    Ok(info)
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
    let selection = cancel.child_token();
    let parts = found
        .iter()
        .zip(ROLES)
        .zip(rules)
        .map(|((format, role), rule)| {
            let selection = &selection;
            async move {
                let result = fetch_part(job, format, role, rule, work, progress, selection).await;
                if matches!(result, Err(PartError::Page(PageError::Delegate))) {
                    selection.cancel();
                }
                result
            }
        });
    let results = futures_util::future::join_all(parts).await;
    if cancel.is_cancelled() {
        return Ok(None);
    }
    if results
        .iter()
        .any(|result| matches!(result, Err(PartError::Page(PageError::Delegate))))
    {
        return Err(PageError::Delegate);
    }
    parts_end(job, results, progress)
}

/// One attempt's end from how its parts ended. Run once all have ended: what a failure promises to keep depends on the part that ended last.
pub(super) fn parts_end(
    job: &Job,
    results: Vec<Result<Option<PathBuf>, PartError>>,
    progress: &MediaProgress,
) -> Result<Option<Vec<PathBuf>>, PageError> {
    // Stopped wins over any error, and an error a fresh address would not fix
    // wins over a refused address.
    let mut files = Vec::new();
    let mut failed = None;
    let mut refused = None;
    let folder = job.output.parent().unwrap_or(Path::new(""));
    for result in results {
        match result {
            Ok(Some(file)) => files.push(file),
            Ok(None) => return Ok(None),
            Err(PartError::Transfer(e, retry)) => {
                let told = wirecatch_messages::describe(&e, folder, worked_so_far(progress));
                failed = failed.or_else(|| {
                    Some(Details::engine(&e, "format_transfer", retry).with_error(told))
                });
            }
            Err(PartError::Page(PageError::Delegate)) => return Err(PageError::Delegate),
            Err(PartError::Page(PageError::Failed(error))) => failed = failed.or(Some(error)),
            Err(PartError::Page(PageError::Refused(details))) => {
                refused = refused.or(Some(details));
            }
        }
    }
    if let Some(error) = failed {
        return Err(PageError::Failed(error));
    }
    if let Some(details) = refused {
        return Err(PageError::Refused(details));
    }
    Ok(Some(files))
}

fn worked_so_far(progress: &MediaProgress) -> wirecatch_messages::Worked {
    if progress.keeps_data() {
        wirecatch_messages::Worked::Before
    } else if progress.read().answered {
        wirecatch_messages::Worked::BeforeStartsOver
    } else {
        wirecatch_messages::Worked::Never
    }
}

/// Why one format did not finish.
#[derive(Debug)]
pub(super) enum PartError {
    Page(PageError),
    /// A failure of the transfer, put in words once every part has ended.
    Transfer(EngineError, Option<wirecatch_engine::Retry>),
}

impl From<PageError> for PartError {
    fn from(error: PageError) -> Self {
        PartError::Page(error)
    }
}

impl From<UserError> for PartError {
    fn from(error: UserError) -> Self {
        PartError::Page(error.into())
    }
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
) -> Result<Option<PathBuf>, PartError> {
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
            Err(e) if stream_address_gone(&e) => {
                return Err(PageError::Refused(Details::stream(&e)).into());
            }
            Err(e) => {
                let error = describe::stream(&e, work, job.tools.as_ref(), job.chosen_by);
                return Err(PageError::Failed(Details::stream(&e).with_error(error)).into());
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
                    let details = Details::new("format_transfer", "unexpected_content");
                    return Err(PageError::Refused(details).into());
                }
                progress.finish_file(&file);
                path
            }
            Ok(_) | Err(EngineError::Cancelled) => return Ok(None),
            Err(EngineError::PrivateNetwork { .. }) => {
                return Err(wirecatch_messages::private_network(job.chosen_by).into());
            }
            Err(EngineError::CookiesChanged) => return Err(PageError::Delegate.into()),
            Err(e) if address_gone(&e) => {
                let details = Details::engine(&e, "format_transfer", file.retry());
                return Err(PageError::Refused(details).into());
            }
            Err(e) => return Err(PartError::Transfer(e, file.retry())),
        }
    };
    mark_part_done(job, &file, &marker).await?;
    Ok(Some(file))
}

async fn mark_part_done(job: &Job, file: &Path, marker: &Path) -> Result<(), UserError> {
    tokio::fs::write(marker, crate::naming::file_name_of(file))
        .await
        .map_err(|e| disk(&e, &job.output))
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

fn stream_address_gone(error: &wirecatch_streams::Error) -> bool {
    match error {
        wirecatch_streams::Error::Request(e)
        | wirecatch_streams::Error::Segment { cause: e, .. } => address_gone(e),
        _ => false,
    }
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

/// The request options for one format: the format's own headers only (the
/// page's cookies stay with the page), bound to its own address, the local
/// network rule decided for it, and requests no larger than the format's
/// server allows at full speed (yt-dlp's `http_chunk_size`).
pub(super) fn part_options(job: &Job, format: &Format, rule: PrivateNetwork) -> DownloadOptions {
    let mut options = job.options.clone();
    let format_headers: Vec<Header> = format
        .http_headers
        .iter()
        .filter(|(name, _)| !name.eq_ignore_ascii_case("cookie"))
        .map(|(name, value)| Header {
            name: name.clone(),
            value: value.clone(),
        })
        .collect();
    options.headers = headers::to_map(&format_headers);
    options.http_cookies = format.native_cookies(&job.suffixes);
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
