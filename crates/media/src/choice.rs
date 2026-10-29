//! Turning what a stream or a page offers into the quality choice the user sees:
//! best first, in plain words ("1080p, MP4"), with sizes when known.

#![forbid(unsafe_code)]

use std::collections::BTreeMap;

use wirecatch_engine::numeric::{approx_f64, f64_to_u64};

use wirecatch_protocol::app::{
    FixAction, MediaChoice, MediaOption, MediaSelection, OutputFit, OutputFormat, StreamAction,
    UnembeddedSubtitles, UserError,
};
use wirecatch_streams::{AudioTrack, Choice, StreamInfo, VideoTrack};

use super::output::{self, AudioAction, Extras, OutputJob, SubtitlePick, Variant};
use super::ytdlp::{Format, Info, ids};
use super::{Content, MediaJob, Pick, safe_ext};

/// Most options offered for one video.
const MAX_OPTIONS: usize = 12;
/// Most subtitle languages saved with one download.
pub(crate) const MAX_PICKED_SUBTITLES: usize = 30;

/// What choosing an option means.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plan {
    /// What the Original choice downloads.
    pub job: MediaJob,
    /// The extension the file will most likely get.
    pub ext: String,
    /// The quality in words, for the download row ("1080p", "Audio only").
    pub quality: String,
    pub audio_only: bool,
    pub needs_ffmpeg: bool,
    /// Each "Save as" choice of a page's quality, Original first (D24); empty
    /// for a stream, which is only saved as it comes.
    pub variants: Vec<Variant>,
}

/// One offered option and what it means.
pub type Offer = (MediaOption, Plan);

/// A file format by its name for a person, as Save as names it ("WebM", "Opus"), so a quality's
/// label and the format menu never spell one format two ways; an extension the app has no name for
/// is shown in capitals.
pub(crate) fn format_name(ext: &str) -> String {
    output::Container::from_ext(ext)
        .map_or_else(|| ext.to_ascii_uppercase(), |c| c.label().to_owned())
}

fn offer(id: String, label: String, height: Option<u32>, size: Option<u64>, plan: Plan) -> Offer {
    let outputs = if plan.variants.is_empty() {
        // A stream: only as it comes, never changed.
        vec![OutputFit {
            format: OutputFormat::Original,
            ext: plan.ext.clone(),
            video: if plan.audio_only {
                StreamAction::None
            } else {
                StreamAction::Copy
            },
            audio: StreamAction::Copy,
            video_codec: None,
            audio_codec: None,
            audio_kbps: None,
            lossless_source: false,
            size,
            embeds_subtitles: false,
            chapters: false,
        }]
    } else {
        plan.variants.iter().map(|v| v.fit.clone()).collect()
    };
    (
        MediaOption {
            id,
            label,
            height,
            audio_only: plan.audio_only,
            size,
            needs_ffmpeg: plan.needs_ffmpeg,
            quality: plan.quality.clone(),
            outputs,
        },
        plan,
    )
}

/// Why a selection cannot be downloaded: it asks for something that was not offered. Each message says what to choose instead.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ResolveError {
    /// The quality has no such "Save as" format.
    #[error(
        "This quality cannot be saved in that format. Choose one of the formats listed for it."
    )]
    FormatNotOffered(OutputFormat),
    /// A subtitle with this id was not offered.
    #[error(
        "One of the subtitles chosen is not offered for this video. Choose from the listed subtitles."
    )]
    SubtitleNotOffered(String),
    /// More subtitle languages than one video is saved with.
    #[error(
        "At most {MAX_PICKED_SUBTITLES} subtitle languages can be saved with one video. Untick some."
    )]
    TooManySubtitles(usize),
    /// An audio quality the format does not offer.
    #[error("{kbps} kbps is not one of the audio qualities for {}. Choose one of the listed qualities.", .container.label())]
    AudioQualityNotOffered {
        /// The quality asked for.
        kbps: u32,
        /// The format it was asked for.
        container: output::Container,
    },
}

/// What a chosen option, saved as the person chose, downloads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resolved {
    pub job: MediaJob,
    /// The file's extension.
    pub ext: String,
    /// ffmpeg is needed: to join, convert or add something to the file.
    pub needs_ffmpeg: bool,
}

impl Plan {
    /// The download for `selection` of this option in `choice`: its format,
    /// audio quality, subtitles and chapters, checked against what was offered.
    ///
    /// # Errors
    ///
    /// When the selection asks for something that was not offered.
    pub fn resolve(
        &self,
        choice: &MediaChoice,
        selection: &MediaSelection,
    ) -> Result<Resolved, ResolveError> {
        let mut picks = subtitle_picks(choice, selection)?;
        let holds_subtitles = self
            .variants
            .iter()
            .find(|v| v.fit.format == selection.output)
            .is_some_and(|v| v.fit.embeds_subtitles);
        if !selection.subtitle_files
            && !holds_subtitles
            && selection.unembedded_subtitles == UnembeddedSubtitles::Skip
        {
            picks.clear();
        }
        let plain = selection.output == OutputFormat::Original
            && picks.is_empty()
            && !(selection.chapters && choice.chapters);
        if plain {
            return Ok(Resolved {
                job: self.job.clone(),
                ext: self.ext.clone(),
                needs_ffmpeg: self.needs_ffmpeg,
            });
        }
        let Some(variant) = self
            .variants
            .iter()
            .find(|v| v.fit.format == selection.output)
        else {
            return Err(ResolveError::FormatNotOffered(selection.output));
        };
        let audio = audio_action(variant, selection)?;
        // The streams this format downloads, kept as they came, are not always
        // the Original choice's (MP4 may take VP9 with AAC, which only MKV holds).
        let original_ext = variant.kept_ext.clone();
        let job = OutputJob {
            container: variant.container,
            video: variant.video_action,
            audio,
            subtitles: picks,
            subtitle_files: selection.subtitle_files || !variant.fit.embeds_subtitles,
            extras: Extras {
                chapters: selection.chapters && choice.chapters && variant.fit.chapters,
                tags: selection.output != OutputFormat::Original && !variant.container.is_video(),
            },
            duration_ms: choice.duration_ms,
            original_ext,
            original: selection.output == OutputFormat::Original,
        };
        Ok(Resolved {
            job: MediaJob::Page {
                video: variant.video.clone(),
                audio: variant.audio.clone(),
                output: Some(Box::new(job)),
                keep_ext: None,
            },
            ext: variant.fit.ext.clone(),
            needs_ffmpeg: true,
        })
    }
}

/// The subtitles `selection` asks for, each once, checked against what `choice`
/// offered.
fn subtitle_picks(
    choice: &MediaChoice,
    selection: &MediaSelection,
) -> Result<Vec<SubtitlePick>, ResolveError> {
    let mut picks: Vec<SubtitlePick> = Vec::new();
    for id in &selection.subtitles {
        let Some(offered) = choice.subtitles.iter().find(|s| &s.id == id) else {
            return Err(ResolveError::SubtitleNotOffered(id.clone()));
        };
        if picks
            .iter()
            .any(|p| p.key == key_of(id) && p.auto == offered.auto)
        {
            continue;
        }
        picks.push(SubtitlePick {
            key: key_of(id).to_string(),
            auto: offered.auto,
            language: offered.language.clone(),
            name: offered.name.clone(),
        });
    }
    if picks.len() > MAX_PICKED_SUBTITLES {
        return Err(ResolveError::TooManySubtitles(picks.len()));
    }
    Ok(picks)
}

/// What happens to the audio of `variant`: re-encoded at the quality
/// `selection` picked for an audio format, as the variant says otherwise.
fn audio_action(
    variant: &Variant,
    selection: &MediaSelection,
) -> Result<AudioAction, ResolveError> {
    match (selection.audio_kbps, variant.container.is_video()) {
        (Some(kbps), false) if selection.output != OutputFormat::Original => {
            let target = variant.container.audio_target();
            let choices = output::kbps_choices(target);
            if choices.is_empty() {
                // A lossless format has no audio quality to pick.
                Ok(variant.audio_action)
            } else if choices.contains(&kbps) {
                Ok(AudioAction::Encode {
                    codec: target,
                    kbps: Some(kbps),
                })
            } else {
                Err(ResolveError::AudioQualityNotOffered {
                    kbps,
                    container: variant.container,
                })
            }
        }
        _ => Ok(variant.audio_action),
    }
}

/// yt-dlp's key of a subtitle from its id in the choice (`auto:en-orig` is
/// `en-orig`).
fn key_of(id: &str) -> &str {
    id.split_once(':').map_or(id, |(_, key)| key)
}

/// Whether the media add-on's ffmpeg is there to join and remux a stream.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ffmpeg {
    Installed,
    Missing,
}

/// The options of an HLS or DASH stream. `ffmpeg` decides the container a single track is saved in.
pub fn from_stream(info: &StreamInfo, ffmpeg: Ffmpeg) -> Result<Vec<Offer>, UserError> {
    if info.live {
        return Err(live());
    }
    let seconds = info.duration.map(|d| d.as_secs_f64());
    let audio = info
        .audio
        .iter()
        .max_by_key(|a| (a.default, a.bandwidth.unwrap_or(0)));
    let mut best_per_height: BTreeMap<u32, &VideoTrack> = BTreeMap::new();
    let mut no_height = Vec::new();
    for video in &info.video {
        match video.height {
            Some(h) => {
                let slot = best_per_height.entry(h).or_insert(video);
                if video.bandwidth.unwrap_or(0) > slot.bandwidth.unwrap_or(0) {
                    *slot = video;
                }
            }
            None => no_height.push(video),
        }
    }
    let mut videos: Vec<&VideoTrack> = best_per_height.into_values().rev().collect();
    if videos.is_empty() {
        no_height.sort_by_key(|v| std::cmp::Reverse(v.bandwidth.unwrap_or(0)));
        videos = no_height;
    }
    let mut offers = Vec::new();
    for (n, video) in videos.iter().enumerate() {
        let needs_ffmpeg = audio.is_some();
        let audio_pick = audio.map_or(Choice::Skip, |a| Choice::Track(a.id.clone()));
        let ext = safe_ext(
            Some(info.extension(
                &Choice::Track(video.id.clone()),
                &audio_pick,
                ffmpeg == Ffmpeg::Installed || needs_ffmpeg,
            )),
            Content::Video,
        );
        let quality = match video.height {
            Some(h) => format!("{h}p"),
            None => "Video".to_string(),
        };
        let size = estimate(seconds, [video.bandwidth, audio.and_then(|a| a.bandwidth)]);
        let plan = Plan {
            job: MediaJob::Stream {
                video: Pick::Track(video.id.clone()),
                audio: audio.map_or(Pick::Skip, |a| Pick::Track(a.id.clone())),
            },
            ext: ext.clone(),
            quality: quality.clone(),
            audio_only: false,
            needs_ffmpeg,
            variants: Vec::new(),
        };
        offers.push(offer(
            format!("s{n}"),
            format!("{quality}, {}", format_name(&ext)),
            video.height,
            size,
            plan,
        ));
    }
    let offers = with_audio_last(
        offers,
        audio.map(|a| audio_only_stream(info, a, ffmpeg, seconds)),
    );
    if offers.is_empty() {
        return Err(nothing());
    }
    Ok(offers)
}

fn audio_only_stream(
    info: &StreamInfo,
    audio: &AudioTrack,
    ffmpeg: Ffmpeg,
    seconds: Option<f64>,
) -> Offer {
    let ext = safe_ext(
        Some(info.extension(
            &Choice::Skip,
            &Choice::Track(audio.id.clone()),
            ffmpeg == Ffmpeg::Installed,
        )),
        Content::Audio,
    );
    let plan = Plan {
        job: MediaJob::Stream {
            video: Pick::Skip,
            audio: Pick::Track(audio.id.clone()),
        },
        ext: ext.clone(),
        quality: "Audio only".into(),
        audio_only: true,
        needs_ffmpeg: false,
        variants: Vec::new(),
    };
    offer(
        "audio".into(),
        format!("Audio only, {}", format_name(&ext)),
        None,
        estimate(seconds, [audio.bandwidth, None]),
        plan,
    )
}

/// Bytes for `seconds` at the sum of `bandwidths` (bits per second).
fn estimate(seconds: Option<f64>, bandwidths: [Option<u64>; 2]) -> Option<u64> {
    let seconds = seconds.filter(|s| s.is_finite() && *s > 0.0)?;
    let bits: u64 = bandwidths.iter().flatten().sum();
    if bits == 0 {
        return None;
    }
    f64_to_u64(approx_f64(bits) * seconds / 8.0)
}

/// The options of a page read by yt-dlp.
pub fn from_page(info: &Info) -> Result<Vec<Offer>, UserError> {
    if info.is_live() {
        return Err(live());
    }
    let all = info.all_formats();
    let usable = usable_formats(&all)?;
    let audios: Vec<&Format> = usable
        .iter()
        .copied()
        .filter(|f| f.has_audio() && !f.has_video())
        .collect();
    let videos: Vec<&Format> = usable.iter().copied().filter(|f| f.has_video()).collect();
    let duration_ms = duration_ms(info);
    let offers: Vec<Offer> = heights(&videos)
        .into_iter()
        .filter_map(|height| video_offer(&videos, &audios, height, duration_ms))
        .collect();
    let offers = with_audio_last(offers, audio_offer(&audios, duration_ms));
    if offers.is_empty() {
        return Err(nothing());
    }
    Ok(offers)
}

/// The video offers, best first, cut so that with the audio-only offer after them there are at most [`MAX_OPTIONS`]: the audio-only choice is never the one cut.
fn with_audio_last(mut videos: Vec<Offer>, audio: Option<Offer>) -> Vec<Offer> {
    videos.truncate(MAX_OPTIONS - usize::from(audio.is_some()));
    videos.extend(audio);
    videos
}

/// The formats of `all` Wirecatch can offer: with video or audio, and not
/// protected with DRM. The error says why there are none.
fn usable_formats(all: &[Format]) -> Result<Vec<&Format>, UserError> {
    let usable: Vec<&Format> = all
        .iter()
        .filter(|f| !f.drm() && (f.has_video() || f.has_audio()))
        .collect();
    if !usable.is_empty() {
        return Ok(usable);
    }
    if all.iter().any(Format::drm) {
        return Err(UserError {
            message: "This video is protected with DRM, which only licensed players can open, so Wirecatch cannot download it.".into(),
            fix: None,
        });
    }
    Err(nothing())
}

/// The heights the videos come in, tallest first; unknown only when no video
/// says its height.
fn heights(videos: &[&Format]) -> Vec<Option<u32>> {
    let mut heights: Vec<Option<u32>> = videos.iter().map(|f| f.height).collect();
    heights.sort_by_key(|h| std::cmp::Reverse(*h));
    heights.dedup();
    if heights.iter().any(Option::is_some) {
        heights.retain(Option::is_some);
    }
    heights
}

/// The best of `audios`, in the `prefer`red extension when there is one.
fn best_audio<'a>(audios: &[&'a Format], prefer: Option<&str>) -> Option<&'a Format> {
    let preferred: Vec<&Format> = audios
        .iter()
        .copied()
        .filter(|a| a.ext.as_deref() == prefer)
        .collect();
    output::best_by_rate(preferred.into_iter())
        .or_else(|| output::best_by_rate(audios.iter().copied()))
}

/// The option for the videos of `height`: the best video-only format joined
/// with the best audio in a matching container, or else the best format with
/// both; `None` when there is neither.
fn video_offer(
    videos: &[&Format],
    audios: &[&Format],
    height: Option<u32>,
    duration_ms: Option<u64>,
) -> Option<Offer> {
    let at: Vec<&Format> = videos
        .iter()
        .copied()
        .filter(|f| f.height == height)
        .collect();
    let separate: Vec<&Format> = at.iter().copied().filter(|f| !f.has_audio()).collect();
    let mp4: Vec<&Format> = separate
        .iter()
        .copied()
        .filter(|f| f.ext.as_deref() == Some("mp4"))
        .collect();
    let video_only = output::best_by_rate(mp4.into_iter())
        .or_else(|| output::best_by_rate(separate.iter().copied()));
    let together = output::best_by_rate(at.iter().copied().filter(|f| f.has_audio()));
    let pair = video_only.and_then(|v| {
        let prefer = match v.ext.as_deref() {
            Some("mp4") => Some("m4a"),
            Some("webm") => Some("webm"),
            _ => None,
        };
        best_audio(audios, prefer).map(|a| (v, a))
    });
    let fps = at.iter().filter_map(|f| f.fps).fold(0.0, f64::max);
    let quality = match height {
        Some(h) if fps >= 50.0 => format!("{h}p, {} fps", fps.round()),
        Some(h) => format!("{h}p"),
        None => "Best quality".to_string(),
    };
    let (video, audio) = match pair {
        Some((v, a)) => (v, Some(a)),
        None => (together.or(video_only)?, None),
    };
    let (id, ext, needs_ffmpeg, size) = if let Some(a) = audio {
        (
            ids(Some(&video.id), Some(&a.id)),
            output::kept_ext(Some(video), Some(a)),
            true,
            video.size().zip(a.size()).map(|(x, y)| x + y),
        )
    } else {
        let content = if video.has_video() {
            Content::Video
        } else {
            Content::Audio
        };
        let ext = safe_ext(video.ext.as_deref(), content);
        (video.id.to_string(), ext, false, video.size())
    };
    let job = MediaJob::Page {
        video: Some(video.id.clone()),
        audio: audio.map(|a| a.id.clone()),
        output: None,
        keep_ext: None,
    };
    let label = format!("{quality}, {}", format_name(&ext));
    let mut variants =
        output::video_variants(&at, audios, (video, audio), &ext, height, duration_ms);
    if let Some(first) = variants.first_mut() {
        // The Original choice is exactly today's download, size included.
        first.fit.size = size;
    }
    let plan = Plan {
        job,
        ext,
        quality,
        audio_only: false,
        needs_ffmpeg,
        variants,
    };
    Some(offer(id, label, height, size, plan))
}

/// The audio-only option: the best audio, preferably M4A, as it comes or
/// converted.
fn audio_offer(audios: &[&Format], duration_ms: Option<u64>) -> Option<Offer> {
    let audio = best_audio(audios, Some("m4a"))?;
    let ext = safe_ext(audio.ext.as_deref(), Content::Audio);
    let variants = output::audio_variants(audios, audio, &ext, duration_ms);
    Some(offer(
        audio.id.to_string(),
        format!("Audio only, {}", format_name(&ext)),
        None,
        audio.size(),
        Plan {
            job: MediaJob::Page {
                video: None,
                audio: Some(audio.id.clone()),
                output: None,
                keep_ext: None,
            },
            ext,
            quality: "Audio only".into(),
            audio_only: true,
            needs_ffmpeg: false,
            variants,
        },
    ))
}

/// The longest a video can be, in seconds (over three years); a longer length is a mistake in the page.
const MAX_DURATION_SECONDS: f64 = 1e8;

/// A page's length in milliseconds, when yt-dlp gave a sensible one.
#[must_use]
pub fn duration_ms(info: &Info) -> Option<u64> {
    info.duration
        .filter(|d| d.is_finite() && *d > 0.0 && *d < MAX_DURATION_SECONDS)
        .and_then(|d| f64_to_u64((d * 1000.0).round()))
}

fn live() -> UserError {
    UserError {
        message:
            "This video is a live broadcast. Wirecatch can download it after the broadcast ends."
                .into(),
        fix: None,
    }
}

fn nothing() -> UserError {
    UserError {
        message: "Wirecatch found no video or audio it can download on this page. Open the video page again and start the video, then try again.".into(),
        fix: Some(FixAction::Retry),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info(json: &str) -> Info {
        serde_json::from_str(json).unwrap()
    }

    /// A stream's size is estimated from its length and bandwidth, and an
    /// estimate no file could reach is no estimate at all.
    #[test]
    fn a_size_is_estimated_only_when_it_is_a_count() {
        assert_eq!(estimate(Some(10.0), [Some(8_000), Some(800)]), Some(11_000));
        assert_eq!(estimate(Some(10.0), [None, None]), None);
        assert_eq!(estimate(None, [Some(8_000), None]), None);
        assert_eq!(estimate(Some(1e300), [Some(8_000), None]), None);
    }

    #[test]
    fn separate_video_and_audio_are_paired_best_first() {
        let info = info(
            r#"{"title":"T","formats":[
            {"format_id":"18","ext":"mp4","vcodec":"avc1","acodec":"mp4a","height":360,"tbr":500,"url":"u"},
            {"format_id":"140","ext":"m4a","vcodec":"none","acodec":"mp4a","abr":128,"url":"u","filesize":100},
            {"format_id":"251","ext":"webm","vcodec":"none","acodec":"opus","abr":160,"url":"u"},
            {"format_id":"137","ext":"mp4","vcodec":"avc1","acodec":"none","height":1080,"tbr":4000,"url":"u","filesize":900},
            {"format_id":"248","ext":"webm","vcodec":"vp9","acodec":"none","height":1080,"tbr":5000,"url":"u"},
            {"format_id":"313","ext":"webm","vcodec":"vp9","acodec":"none","height":2160,"fps":60,"tbr":20000,"url":"u"}
        ]}"#,
        );
        let offers = from_page(&info).unwrap();
        let labels: Vec<&str> = offers.iter().map(|(o, _)| o.label.as_str()).collect();
        assert_eq!(
            labels,
            [
                "2160p, 60 fps, WebM",
                "1080p, MP4",
                "360p, MP4",
                "Audio only, M4A"
            ]
        );
        let (option, plan) = &offers[1];
        assert_eq!(option.id, "137+140");
        assert!(option.needs_ffmpeg && plan.needs_ffmpeg);
        assert_eq!(option.size, Some(1000));
        assert_eq!(option.height, Some(1080));
        let (option, _) = &offers[0];
        assert_eq!(option.id, "313+251");
        let (option, plan) = &offers[2];
        assert_eq!(option.id, "18");
        assert!(!option.needs_ffmpeg);
        assert_eq!(plan.ext, "mp4");
        let (option, plan) = &offers[3];
        assert!(option.audio_only && plan.audio_only);
        assert_eq!(option.id, "140");
    }

    #[test]
    fn a_site_cannot_choose_a_program_extension() {
        let offers = from_page(&info(
            r#"{"title":"Invoice","formats":[
            {"format_id":"1","ext":"exe","vcodec":"avc1","acodec":"mp4a","height":720,"url":"u"},
            {"format_id":"2","ext":"bat","vcodec":"avc1","acodec":"none","height":480,"url":"u"},
            {"format_id":"3","ext":"lnk","vcodec":"none","acodec":"mp4a","url":"u"},
            {"format_id":"4","ext":"","vcodec":"avc1","acodec":"mp4a","height":360,"url":"u"},
            {"format_id":"5","ext":"m4a.exe","vcodec":"none","acodec":"opus","abr":1,"url":"u"}
        ]}"#,
        ))
        .unwrap();
        for (option, plan) in &offers {
            for bad in ["exe", "bat", "lnk"] {
                assert!(!plan.ext.contains(bad), "{} -> {}", option.id, plan.ext);
                assert!(
                    !option.label.to_lowercase().contains(bad),
                    "{}",
                    option.label
                );
            }
            assert_ne!(plan.ext, "");
        }
        let ext = |id: &str| {
            offers
                .iter()
                .find(|(o, _)| o.id == id)
                .map(|(_, p)| p.ext.clone())
                .unwrap()
        };
        assert_eq!(ext("1"), "mp4");
        let paired = offers
            .iter()
            .find(|(o, _)| o.height == Some(480))
            .map(|(_, p)| p.ext.clone());
        assert_eq!(paired.as_deref(), Some("mkv"));
        assert_eq!(ext("4"), "mp4");
        assert!(offers.iter().any(|(o, p)| o.audio_only && p.ext == "m4a"));
    }

    #[test]
    fn live_and_drm_pages_are_refused_plainly() {
        let live = from_page(&info(r#"{"is_live":true,"formats":[]}"#)).unwrap_err();
        assert!(live.message.contains("live broadcast"));
        let drm = from_page(&info(
            r#"{"formats":[{"format_id":"1","url":"u","has_drm":true}]}"#,
        ))
        .unwrap_err();
        assert!(drm.message.contains("DRM"));
        assert!(drm.fix.is_none());
        let none = from_page(&info(r#"{"formats":[]}"#)).unwrap_err();
        assert_eq!(none.fix, Some(FixAction::Retry));
    }

    #[test]
    fn a_page_with_one_plain_file_offers_it() {
        let offers = from_page(&info(
            r#"{"title":"T","url":"https://x/v.mp4","ext":"mp4"}"#,
        ))
        .unwrap();
        assert_eq!(offers.len(), 1);
        assert_eq!(offers[0].1.ext, "mp4");
    }

    /// A 2160p row that comes only as VP9, a 1080p row with H.264 too, and
    /// audio as Opus and AAC.
    fn youtube_like() -> Vec<Offer> {
        from_page(&info(
            r#"{"title":"T","duration":2.0,"formats":[
            {"format_id":"251","ext":"webm","vcodec":"none","acodec":"opus","abr":160,"url":"u"},
            {"format_id":"140","ext":"m4a","vcodec":"none","acodec":"mp4a.40.2","abr":128,"url":"u"},
            {"format_id":"313","ext":"webm","vcodec":"vp9","acodec":"none","height":2160,"tbr":12000,"url":"u"},
            {"format_id":"248","ext":"webm","vcodec":"vp9","acodec":"none","height":1080,"tbr":3000,"url":"u"},
            {"format_id":"137","ext":"mp4","vcodec":"avc1.640028","acodec":"none","height":1080,"tbr":4000,"url":"u"}
        ]}"#,
        ))
        .unwrap()
    }

    fn choice_for(offers: &[Offer]) -> MediaChoice {
        MediaChoice {
            id: wirecatch_protocol::QuestionId(1),
            title: "T".into(),
            page_url: None,
            options: offers.iter().map(|(o, _)| o.clone()).collect(),
            folder: "/v".into(),
            type_folder: None,
            queue: wirecatch_protocol::QueueId(1),
            queue_reason: wirecatch_protocol::rules::QueueReason::BrowserQueue,
            subtitles: vec![
                wirecatch_protocol::app::MediaSubtitle {
                    id: "sub:en".into(),
                    language: "en".into(),
                    name: "English".into(),
                    auto: false,
                },
                wirecatch_protocol::app::MediaSubtitle {
                    id: "auto:de-orig".into(),
                    language: "de".into(),
                    name: "German".into(),
                    auto: true,
                },
            ],
            chapters: true,
            duration_ms: Some(2_000),
            start: selection(&offers[0].0.id, OutputFormat::Original),
            missing_quality: None,
        }
    }

    fn selection(option: &str, output: OutputFormat) -> MediaSelection {
        MediaSelection {
            option_id: option.into(),
            folder: None,
            file_name: None,
            queue: None,
            later: false,
            output,
            audio_kbps: None,
            subtitles: Vec::new(),
            subtitle_files: false,
            unembedded_subtitles: UnembeddedSubtitles::Files,
            chapters: false,
            every_video: None,
        }
    }

    #[test]
    fn each_row_offers_every_format_and_says_what_converts() {
        let offers = youtube_like();
        let (uhd, _) = &offers[0];
        assert_eq!(uhd.quality, "2160p");
        let formats: Vec<OutputFormat> = uhd.outputs.iter().map(|f| f.format).collect();
        assert_eq!(
            formats,
            [
                OutputFormat::Original,
                OutputFormat::Mp4,
                OutputFormat::Mkv,
                OutputFormat::Webm
            ]
        );
        let video: Vec<StreamAction> = uhd.outputs.iter().map(|f| f.video).collect();
        assert_eq!(
            video,
            [
                StreamAction::Copy,
                StreamAction::Encode,
                StreamAction::Copy,
                StreamAction::Copy
            ]
        );
        assert_eq!(uhd.outputs[0].ext, "webm");
        // 1080p has H.264: MP4 copies it.
        let (full_hd, _) = &offers[1];
        assert!(
            full_hd
                .outputs
                .iter()
                .all(|f| f.video == StreamAction::Copy)
        );
        let (audio, _) = offers.last().unwrap();
        assert!(audio.audio_only);
        let formats: Vec<OutputFormat> = audio.outputs.iter().map(|f| f.format).collect();
        assert_eq!(
            formats,
            [
                OutputFormat::Original,
                OutputFormat::Mp3,
                OutputFormat::M4a,
                OutputFormat::Opus,
                OutputFormat::Flac,
                OutputFormat::Wav
            ]
        );
    }

    #[test]
    fn audio_only_stays_offered_when_there_are_many_heights() {
        let mut formats: Vec<String> = (1..=14)
            .map(|n| {
                format!(
                    r#"{{"format_id":"v{n}","ext":"mp4","vcodec":"avc1","acodec":"none","height":{},"tbr":{n},"url":"u"}}"#,
                    n * 100
                )
            })
            .collect();
        formats.push(
            r#"{"format_id":"140","ext":"m4a","vcodec":"none","acodec":"mp4a","abr":128,"url":"u"}"#
                .into(),
        );
        let info = info(&format!(
            r#"{{"title":"T","formats":[{}]}}"#,
            formats.join(",")
        ));
        let offers = from_page(&info).unwrap();
        assert_eq!(offers.len(), MAX_OPTIONS);
        let labels: Vec<&str> = offers.iter().map(|(o, _)| o.label.as_str()).collect();
        assert!(
            offers.last().is_some_and(|(o, _)| o.audio_only),
            "{labels:?}"
        );
        assert_eq!(offers[0].0.height, Some(1400));
    }

    #[test]
    fn a_stream_is_offered_only_as_it_comes() {
        let stream = |audio_only: bool| {
            let plan = Plan {
                job: MediaJob::Stream {
                    video: if audio_only {
                        Pick::Skip
                    } else {
                        Pick::Track("v".into())
                    },
                    audio: Pick::Track("a".into()),
                },
                ext: "mp4".into(),
                quality: "720p".into(),
                audio_only,
                needs_ffmpeg: false,
                variants: Vec::new(),
            };
            let (option, _) = offer("s0".into(), "720p, MP4".into(), None, None, plan);
            let [fit] = option.outputs.as_slice() else {
                panic!("a stream offers one choice");
            };
            assert_eq!(fit.format, OutputFormat::Original);
            (fit.video, fit.audio)
        };
        assert_eq!(stream(false), (StreamAction::Copy, StreamAction::Copy));
        assert_eq!(stream(true), (StreamAction::None, StreamAction::Copy));
    }

    #[test]
    fn a_selection_resolves_to_its_formats_streams() {
        let offers = youtube_like();
        let choice = choice_for(&offers);
        let (uhd, plan) = &offers[0];

        // Original with nothing added: exactly the download of before.
        let original = plan
            .resolve(&choice, &selection(&uhd.id, OutputFormat::Original))
            .unwrap();
        assert_eq!(original.job, plan.job);
        assert_eq!(original.ext, "webm");

        // MP4 with subtitles and chapters: VP9 re-encoded, AAC copied.
        let mut mp4 = selection(&uhd.id, OutputFormat::Mp4);
        mp4.subtitles = vec!["sub:en".into(), "auto:de-orig".into()];
        mp4.chapters = true;
        let resolved = plan.resolve(&choice, &mp4).unwrap();
        assert_eq!(resolved.ext, "mp4");
        let MediaJob::Page {
            video,
            audio,
            output: Some(output),
            ..
        } = &resolved.job
        else {
            panic!("no output: {:?}", resolved.job);
        };
        assert_eq!(ids(video.as_ref(), audio.as_ref()), "313+140");
        assert!(matches!(output.video, output::VideoAction::Encode { .. }));
        assert_eq!(output.audio, AudioAction::Copy);
        assert_eq!(output.subtitles.len(), 2);
        assert_eq!(output.subtitles[1].key, "de-orig");
        assert!(output.subtitles[1].auto);
        assert!(output.extras.chapters && !output.subtitle_files && !output.extras.tags);
        assert_eq!(output.original_ext, "mkv", "VP9 with AAC is kept as MKV");

        // Original with subtitles: copied back into WebM with them.
        let mut original = selection(&uhd.id, OutputFormat::Original);
        original.subtitles = vec!["sub:en".into()];
        let resolved = plan.resolve(&choice, &original).unwrap();
        let MediaJob::Page {
            output: Some(output),
            ..
        } = &resolved.job
        else {
            panic!("no output");
        };
        assert!(output.original && output.embedded_subtitles());
        assert_eq!(output.container, output::Container::Webm);

        // The audio row as MP3 at 320 kbps, with tags; subtitles as files.
        let (audio, plan) = offers.last().unwrap();
        let mut mp3 = selection(&audio.id, OutputFormat::Mp3);
        mp3.audio_kbps = Some(320);
        mp3.subtitles = vec!["sub:en".into()];
        let resolved = plan.resolve(&choice, &mp3).unwrap();
        let MediaJob::Page {
            video,
            audio,
            output: Some(output),
            ..
        } = &resolved.job
        else {
            panic!("no output");
        };
        assert_eq!(
            ids(video.as_ref(), audio.as_ref()),
            "251",
            "the best audio is re-encoded"
        );
        assert_eq!(
            output.audio,
            AudioAction::Encode {
                codec: output::AudioCodec::Mp3,
                kbps: Some(320)
            }
        );
        assert!(output.extras.tags && output.sidecar_subtitles());
        assert!(!output.extras.chapters, "an MP3 holds no chapters");
        assert_eq!(resolved.ext, "mp3");
    }

    /// Subtitles meant for inside a file whose format cannot hold them are saved as files, or
    /// left out when the person asked (D93); asked for as files, they are files whatever the format.
    #[test]
    fn subtitles_a_format_cannot_hold_are_files_or_left_out() {
        let offers = youtube_like();
        let choice = choice_for(&offers);
        let (audio, plan) = offers.last().unwrap();
        let mut mp3 = selection(&audio.id, OutputFormat::Mp3);
        mp3.subtitles = vec!["sub:en".into()];
        let output = |selection: &MediaSelection| {
            let resolved = plan.resolve(&choice, selection).unwrap();
            let MediaJob::Page {
                output: Some(output),
                ..
            } = resolved.job
            else {
                panic!("no output");
            };
            *output
        };
        assert!(output(&mp3).sidecar_subtitles());
        mp3.unembedded_subtitles = UnembeddedSubtitles::Skip;
        let skipped = output(&mp3);
        assert!(skipped.subtitles.is_empty() && !skipped.sidecar_subtitles());
        mp3.subtitle_files = true;
        assert!(output(&mp3).sidecar_subtitles());
    }

    #[test]
    fn a_selection_outside_the_offer_is_refused_plainly() {
        let offers = youtube_like();
        let choice = choice_for(&offers);
        let (uhd, plan) = &offers[0];
        let error = plan
            .resolve(&choice, &selection(&uhd.id, OutputFormat::Mp3))
            .unwrap_err();
        assert_eq!(error, ResolveError::FormatNotOffered(OutputFormat::Mp3));
        let mut unknown = selection(&uhd.id, OutputFormat::Mkv);
        unknown.subtitles = vec!["sub:xx".into()];
        assert_eq!(
            plan.resolve(&choice, &unknown),
            Err(ResolveError::SubtitleNotOffered("sub:xx".into()))
        );
        let (audio, plan) = offers.last().unwrap();
        let mut odd = selection(&audio.id, OutputFormat::Mp3);
        odd.audio_kbps = Some(321);
        let error = plan.resolve(&choice, &odd).unwrap_err();
        assert!(
            matches!(
                error,
                ResolveError::AudioQualityNotOffered { kbps: 321, .. }
            ),
            "{error}"
        );
    }
}
