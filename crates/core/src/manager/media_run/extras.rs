//! What a page's video takes besides its formats: subtitles, the cover picture and the metadata file.

use std::collections::HashMap;
use std::path::Path;

use tracing::warn;
use wirecatch_engine::units::MIB;
use wirecatch_engine::{CancellationToken, Error as EngineError, Fetcher, Url};
use wirecatch_media::output::{self, ImageFormat, OutputJob, SubtitleFormat};
use wirecatch_media::ytdlp::{self};
use wirecatch_protocol::Secret;
use wirecatch_protocol::extension::Header;

use super::network::network_rule;
use super::workdir::{Downloaded, DownloadedSubtitle};
use super::{Job, workdir};
use crate::headers;

/// The largest subtitle file taken.
pub(super) const MAX_SUBTITLE_BYTES: u64 = 20 * MIB;

/// The largest cover picture taken.
pub(super) const MAX_COVER_BYTES: u64 = 10 * MIB;

/// Downloads what `output` adds to the file besides the video: the chosen
/// subtitles, the cover picture and the metadata file, into `work`, and lists
/// them in `downloaded`. None of it is worth failing the video for: a
/// subtitle or picture that cannot be had is left out with a note in the log.
/// Returns `false` when stopped.
pub(super) async fn fetch_extras(
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
        let path = work.join(workdir::COVER);
        match fetch_small(job, &url, &HashMap::new(), &path, MAX_COVER_BYTES, cancel).await {
            Fetch::Done => {
                if let Some(format) = ImageFormat::detect(&head(&path).await) {
                    downloaded.cover = Some((workdir::COVER.into(), format));
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
        match tokio::fs::write(work.join(workdir::METADATA), text).await {
            Ok(()) => downloaded.metadata = Some(workdir::METADATA.into()),
            Err(e) => warn!(error = %e, "could not write the metadata file"),
        }
    }
    true
}

/// Downloads the subtitles `output` chose into `work` and lists those that
/// are what they claim in `downloaded`; the others are left out with a note
/// in the log. Returns `false` when stopped.
pub(super) async fn fetch_subtitles(
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
        let path = work.join(workdir::subtitle(n, ext));
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
                file: workdir::subtitle(n, ext),
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

/// Enough of a file to recognise a subtitle or image format.
pub(super) const HEAD_BYTES: usize = 512;

/// The first bytes of a file, to recognise its format.
pub(super) async fn head(path: &Path) -> Vec<u8> {
    use tokio::io::AsyncReadExt;
    let mut buf = vec![0u8; HEAD_BYTES];
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
pub(super) enum Fetch {
    Done,
    Stopped,
    Failed(String),
}

/// Downloads a small file (subtitles, a picture) to `path` under the same rule
/// for the local network as the video, with only its own headers. One larger than `limit` bytes
/// is refused before any of it is written.
pub(super) async fn fetch_small(
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
    let fetched =
        match Fetcher::with_private_network(url, &options, cancel.child_token(), rule).await {
            Ok(fetcher) => fetcher.get(url, None, limit).await,
            Err(e) => Err(e),
        };
    match fetched {
        Ok(fetched) => match tokio::fs::write(path, &fetched.body).await {
            Ok(()) => Fetch::Done,
            Err(e) => Fetch::Failed(e.to_string()),
        },
        Err(_) if cancel.is_cancelled() => Fetch::Stopped,
        Err(EngineError::TooLarge { .. }) => {
            Fetch::Failed(format!("it is larger than {} MB", limit / MIB))
        }
        Err(e) => Fetch::Failed(e.to_string()),
    }
}
