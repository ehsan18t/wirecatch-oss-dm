//! Turning what a stream or a page offers into the quality choice the user sees:
//! best first, in plain words ("1080p, MP4"), with sizes when known.

use std::collections::BTreeMap;

use wirecatch_protocol::app::{FixAction, MediaOption, UserError};
use wirecatch_streams::{AudioTrack, Choice, StreamInfo, VideoTrack};

use super::ytdlp::{Format, Info};
use super::{Content, MediaJob, Pick, safe_ext};

/// Most options offered for one video.
const MAX_OPTIONS: usize = 12;

/// What choosing an option means.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Plan {
    pub job: MediaJob,
    /// The extension the file will most likely get.
    pub ext: String,
    /// The quality in words, for the download row ("1080p", "Audio only").
    pub quality: String,
    pub audio_only: bool,
    pub needs_ffmpeg: bool,
}

/// One offered option and what it means.
pub(crate) type Offer = (MediaOption, Plan);

fn offer(id: String, label: String, height: Option<u32>, size: Option<u64>, plan: Plan) -> Offer {
    (
        MediaOption {
            id,
            label,
            height,
            audio_only: plan.audio_only,
            size,
            needs_ffmpeg: plan.needs_ffmpeg,
        },
        plan,
    )
}

/// The options of an HLS or DASH stream. `ffmpeg` says whether the media add-on
/// is installed, which decides the container a single track is saved in.
pub(crate) fn from_stream(info: &StreamInfo, ffmpeg: bool) -> Result<Vec<Offer>, UserError> {
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
                ffmpeg || needs_ffmpeg,
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
        };
        offers.push(offer(
            format!("s{n}"),
            format!("{quality}, {}", ext.to_ascii_uppercase()),
            video.height,
            size,
            plan,
        ));
    }
    if let Some(audio) = audio {
        offers.push(audio_only_stream(info, audio, ffmpeg, seconds));
    }
    if offers.is_empty() {
        return Err(nothing());
    }
    offers.truncate(MAX_OPTIONS);
    Ok(offers)
}

fn audio_only_stream(
    info: &StreamInfo,
    audio: &AudioTrack,
    ffmpeg: bool,
    seconds: Option<f64>,
) -> Offer {
    let ext = safe_ext(
        Some(info.extension(&Choice::Skip, &Choice::Track(audio.id.clone()), ffmpeg)),
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
    };
    offer(
        "audio".into(),
        format!("Audio only, {}", ext.to_ascii_uppercase()),
        None,
        estimate(seconds, [audio.bandwidth, None]),
        plan,
    )
}

/// The bit rate yt-dlp reports for a format, 0 when unknown.
fn rate(format: &Format) -> f64 {
    format
        .tbr
        .or(format.abr)
        .filter(|r| r.is_finite())
        .unwrap_or(0.0)
}

/// The format with the highest bit rate.
fn best(formats: Vec<&Format>) -> Option<&Format> {
    formats
        .into_iter()
        .max_by(|a, b| rate(a).total_cmp(&rate(b)))
}

/// Bytes for `seconds` at the sum of `bandwidths` (bits per second).
fn estimate(seconds: Option<f64>, bandwidths: [Option<u64>; 2]) -> Option<u64> {
    let seconds = seconds.filter(|s| s.is_finite() && *s > 0.0)?;
    let bits: u64 = bandwidths.iter().flatten().sum();
    (bits > 0).then(|| (bits as f64 * seconds / 8.0) as u64)
}

/// The options of a page read by yt-dlp.
pub(crate) fn from_page(info: &Info) -> Result<Vec<Offer>, UserError> {
    if info.is_live() {
        return Err(live());
    }
    let all = info.all_formats();
    let usable: Vec<&Format> = all
        .iter()
        .filter(|f| !f.drm() && (f.has_video() || f.has_audio()))
        .collect();
    if usable.is_empty() {
        if all.iter().any(Format::drm) {
            return Err(UserError {
                message: "This video is protected with DRM, which only licensed players can open, so Wirecatch cannot download it.".into(),
                fix: None,
            });
        }
        return Err(nothing());
    }
    let audios: Vec<&Format> = usable
        .iter()
        .copied()
        .filter(|f| f.has_audio() && !f.has_video())
        .collect();
    let videos: Vec<&Format> = usable.iter().copied().filter(|f| f.has_video()).collect();
    let best_audio = |prefer: Option<&str>| -> Option<&Format> {
        let preferred: Vec<&Format> = audios
            .iter()
            .copied()
            .filter(|a| a.ext.as_deref() == prefer)
            .collect();
        best(preferred).or_else(|| best(audios.clone()))
    };

    let mut heights: Vec<Option<u32>> = videos.iter().map(|f| f.height).collect();
    heights.sort_by_key(|h| std::cmp::Reverse(*h));
    heights.dedup();
    if heights.iter().any(Option::is_some) {
        heights.retain(Option::is_some);
    }
    let mut offers = Vec::new();
    for height in heights {
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
        let video_only = best(mp4).or_else(|| best(separate));
        let together = best(at.iter().copied().filter(|f| f.has_audio()).collect());
        let pair = video_only.and_then(|v| {
            let prefer = match v.ext.as_deref() {
                Some("mp4") => Some("m4a"),
                Some("webm") => Some("webm"),
                _ => None,
            };
            best_audio(prefer).map(|a| (v, a))
        });
        let fps = at.iter().filter_map(|f| f.fps).fold(0.0, f64::max);
        let quality = match height {
            Some(h) if fps >= 50.0 => format!("{h}p, {} fps", fps.round()),
            Some(h) => format!("{h}p"),
            None => "Best quality".to_string(),
        };
        let (job, ext, needs_ffmpeg, size) = if let Some((v, a)) = pair {
            let (v_ext, a_ext) = (
                safe_ext(v.ext.as_deref(), Content::Video),
                safe_ext(a.ext.as_deref(), Content::Audio),
            );
            // Matroska holds anything, so it is the choice when either side
            // named something that is not a media extension.
            let named = [&v.ext, &a.ext]
                .iter()
                .all(|e| e.as_deref().is_some_and(super::is_media_ext));
            let ext = match (v_ext.as_str(), a_ext.as_str()) {
                ("mp4", "m4a" | "mp4") if named => "mp4",
                ("webm", "webm") if named => "webm",
                _ => "mkv",
            };
            (
                MediaJob::Page {
                    format: format!("{}+{}", v.format_id, a.format_id),
                    video: Some(v.format_id.clone()),
                    audio: Some(a.format_id.clone()),
                },
                ext.to_string(),
                true,
                v.size().zip(a.size()).map(|(x, y)| x + y),
            )
        } else if let Some(f) = together.or(video_only) {
            (
                MediaJob::Page {
                    format: f.format_id.clone(),
                    video: Some(f.format_id.clone()),
                    audio: None,
                },
                safe_ext(
                    f.ext.as_deref(),
                    if f.has_video() {
                        Content::Video
                    } else {
                        Content::Audio
                    },
                ),
                false,
                f.size(),
            )
        } else {
            continue;
        };
        let label = format!("{quality}, {}", ext.to_ascii_uppercase());
        let id = match &job {
            MediaJob::Page { format, .. } => format.clone(),
            MediaJob::Stream { .. } => continue,
        };
        offers.push(offer(
            id,
            label,
            height,
            size,
            Plan {
                job,
                ext,
                quality,
                audio_only: false,
                needs_ffmpeg,
            },
        ));
    }
    if let Some(audio) = best_audio(Some("m4a")) {
        let ext = safe_ext(audio.ext.as_deref(), Content::Audio);
        offers.push(offer(
            audio.format_id.clone(),
            format!("Audio only, {}", ext.to_ascii_uppercase()),
            None,
            audio.size(),
            Plan {
                job: MediaJob::Page {
                    format: audio.format_id.clone(),
                    video: None,
                    audio: Some(audio.format_id.clone()),
                },
                ext,
                quality: "Audio only".into(),
                audio_only: true,
                needs_ffmpeg: false,
            },
        ));
    }
    if offers.is_empty() {
        return Err(nothing());
    }
    offers.truncate(MAX_OPTIONS);
    Ok(offers)
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
                "2160p, 60 fps, WEBM",
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
            assert!(!plan.ext.is_empty());
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
}
