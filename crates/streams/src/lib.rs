//! HLS and DASH video stream downloads for Wirecatch.
//!
//! A streamed video is not one file but a playlist (HLS `.m3u8`) or manifest
//! (DASH `.mpd`) that lists many small segments, often in several qualities
//! and with audio separate from video. This crate reads those, lets the
//! caller pick the tracks, downloads the segments in parallel and joins them
//! into one file.
//!
//! 1. [`inspect`] reads a link and returns a [`StreamInfo`]: the video
//!    qualities, audio tracks and subtitles on offer, the duration, and whether
//!    the stream is live.
//! 2. [`download`] takes a [`StreamRequest`] (the link, where to save, the
//!    chosen tracks and an optional ffmpeg), reports through a
//!    [`StreamProgress`], and returns an [`Outcome`].
//!
//! ```no_run
//! # async fn example() -> Result<(), wirecatch_streams::Error> {
//! use std::sync::Arc;
//! use wirecatch_engine::{CancellationToken, DownloadOptions, Url};
//! use wirecatch_streams::{Choice, StreamProgress, StreamRequest, download, inspect};
//!
//! let url = Url::parse("https://video.example/watch/master.m3u8").unwrap();
//! let info = inspect(&url, &DownloadOptions::default(), CancellationToken::new()).await?;
//! let smallest = info.video.iter().min_by_key(|v| v.height).map(|v| v.id.clone());
//!
//! let mut request = StreamRequest::new(url, "Downloads/My video.mp4");
//! if let Some(id) = smallest {
//!     request.video = Choice::Track(id);
//! }
//! request.ffmpeg = Some("C:/Wirecatch/addons/ffmpeg.exe".into());
//! let outcome = download(request, Arc::new(StreamProgress::default()), CancellationToken::new()).await?;
//! println!("{outcome:?}");
//! # Ok(())
//! # }
//! ```
//!
//! # What is supported
//!
//! - HLS master and media playlists; AES-128 encryption (keys fetched once,
//!   explicit IVs or the media sequence number as IV); `EXT-X-BYTERANGE`;
//!   `EXT-X-MAP` init sections (emitted again when they change).
//! - DASH with one period: `SegmentTemplate` (`$Number$`, `$Time$`,
//!   `$RepresentationID$`, `$Bandwidth$`, widths such as `%05d`,
//!   `SegmentTimeline`), `SegmentList`, and `SegmentBase` (split through its
//!   `sidx` index when it has one, else fetched whole).
//! - Resuming: finished segments are kept in `<output>.wcstream/` next to the
//!   output and not downloaded again.
//!
//! Refused with a plain-language [`Error`]: live broadcasts, DRM (FairPlay,
//! Widevine, PlayReady, Common Encryption) and HLS `SAMPLE-AES`, DASH
//! manifests with several periods. Subtitles are listed, not downloaded.
//!
//! # Credentials
//!
//! Cookies and other headers in [`StreamRequest::options`] are sent only to the
//! origin of the playlist link, exactly as the engine does for a download.
//! Segments and keys on another origin, such as a CDN, get only headers that
//! hold no credentials (see [`wirecatch_engine::fetch`]).
//!
//! A stream from the internet may not point Wirecatch at the local network:
//! segments, keys, init sections and DASH base addresses that are (or resolve
//! to, or redirect to) loopback, private, link-local, shared CGNAT or other
//! non-public addresses are refused with [`Error::PrivateNetwork`] (see
//! [`PrivateNetwork`] for the full list and [`StreamRequest::private_network`]).
//!
//! # Joining
//!
//! With ffmpeg, the tracks are copied (never re-encoded) into MP4, or Matroska
//! when the stream's codecs do not fit MP4 (or MP4 fails). ffmpeg is started
//! with an argument vector, never a shell, and every path is passed as an
//! absolute `file:` path. ffmpeg never guesses what a downloaded track is:
//! each track's format is recognized from its first bytes ([`InputFormat`])
//! and named to ffmpeg, which may read it only with that demuxer and may open
//! only plain files ([`ffmpeg_input_args`]). A track in any other format is
//! refused with [`Error::UnknownFormat`] before ffmpeg starts, so a crafted
//! segment cannot make ffmpeg read other files or addresses into the
//! result. Without ffmpeg, only a stream whose audio and video
//! share one track can be saved, as its segments joined end to end (for
//! example an MPEG-TS `.ts` file); separate tracks fail with
//! [`Error::NeedsFfmpeg`] before anything is downloaded.

#![deny(missing_docs)]

mod crypto;
mod dash;
mod download;
mod error;
mod ffmpeg;
mod hls;
mod model;
mod plan;
mod workdir;

pub use download::download;
pub use error::{Error, Result};
pub use ffmpeg::{InputFormat, ffmpeg_input_args};
pub use model::{
    AudioTrack, Choice, Outcome, Stage, StreamInfo, StreamKind, StreamProgress, StreamRequest,
    SubtitleTrack, VideoTrack,
};
pub use wirecatch_engine::PrivateNetwork;
pub use workdir::DIR_SUFFIX;

use wirecatch_engine::{CancellationToken, DownloadOptions, Fetcher, Url};

/// Reads the playlist or manifest at `url` and describes what it offers.
///
/// Headers, timeouts, retries and the redirect rule come from `options`, as
/// for a download; credentials stay with the origin of `url`. For an HLS
/// master playlist, the best quality's own playlist is read too, to learn the
/// duration and whether the stream is live. A live stream is described
/// (with [`StreamInfo::live`] set), but downloading it is refused.
///
/// # Errors
///
/// [`Error::NotAStream`] when the link is neither HLS nor DASH,
/// [`Error::Manifest`] when it is broken, [`Error::Request`] when it cannot be
/// fetched, and [`Error::Cancelled`] when `cancel` is cancelled first.
pub async fn inspect(
    url: &Url,
    options: &DownloadOptions,
    cancel: CancellationToken,
) -> Result<StreamInfo> {
    let fetcher = Fetcher::new(url, options, cancel).await?;
    let source = plan::Source::load(&fetcher).await?;
    source.info(&fetcher).await
}
