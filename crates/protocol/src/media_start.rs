//! Where a video's quality window starts, and whether it needs to open at all (D50): the
//! quality the saved resolution picks, or its fallback when the video does not offer it, saved
//! as the saved choices say, as far as the video offers them.
//!
//! The core downloads this without asking when the person chose not to be asked. The desktop's
//! sample backend has a TypeScript port (`packages/protocol/src/media-start.ts`), tested
//! against the cases `pnpm protocol:generate` writes from here.

use serde::Serialize;

use crate::app::{
    MediaDefaults, MediaOption, MediaSelection, MediaSubtitle, OutputDefaults, OutputFormat,
    QualityFallback, StreamAction, VideoQuality,
};

/// Where a video's quality window starts, and whether it must open.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MediaStart {
    /// The quality picked, saved as the saved choices say.
    pub selection: MediaSelection,
    /// The saved resolution, when the video does not offer it ("1080p").
    pub missing: Option<String>,
    /// The window must open: the person asks for each video, or the video does not offer the
    /// saved resolution and the fallback is to ask.
    pub asks: bool,
}

impl MediaDefaults {
    /// Where the quality window starts for a video offering `options` (best first, as a
    /// [`MediaChoice`](crate::app::MediaChoice) lists them), `subtitles`, and chapters when
    /// `chapters`. With no options, the selection names none.
    #[must_use]
    pub fn start(
        &self,
        options: &[MediaOption],
        subtitles: &[MediaSubtitle],
        chapters: bool,
    ) -> MediaStart {
        let (index, missing) = self.pick(options);
        let selection = match options.get(index) {
            Some(option) => {
                let saved = if option.audio_only {
                    &self.audio
                } else {
                    &self.video
                };
                saved.selection(option, subtitles, chapters)
            }
            None => OutputDefaults::default().selection_of(String::new(), &[], false),
        };
        let asks =
            self.ask_each_time || (missing.is_some() && self.fallback == QualityFallback::Ask);
        MediaStart {
            selection,
            missing,
            asks,
        }
    }

    /// The option the saved resolution picks, as its place in `options`, and the saved
    /// resolution when the video does not offer it. Heights are compared only among videos
    /// that say theirs; a video with none, or audio alone, starts on its best option.
    fn pick(&self, options: &[MediaOption]) -> (usize, Option<String>) {
        let videos = || {
            options
                .iter()
                .enumerate()
                .filter(|(_, o)| !o.audio_only)
                .filter_map(|(i, o)| o.height.map(|h| (i, h)))
        };
        match self.quality {
            VideoQuality::Best => (0, None),
            VideoQuality::AudioOnly => match options.iter().position(|o| o.audio_only) {
                Some(index) => (index, None),
                None => (0, Some("Audio only".to_owned())),
            },
            VideoQuality::Height(wanted) => {
                if videos().next().is_none() {
                    return (0, None);
                }
                if let Some((index, _)) = videos().find(|&(_, h)| h == wanted) {
                    return (index, None);
                }
                let lower = || {
                    videos()
                        .filter(|&(_, h)| h < wanted)
                        .max_by_key(|&(_, h)| h)
                        .or_else(|| videos().min_by_key(|&(_, h)| h))
                };
                let picked = match self.fallback {
                    QualityFallback::Higher => videos()
                        .filter(|&(_, h)| h > wanted)
                        .min_by_key(|&(_, h)| h)
                        .or_else(|| videos().max_by_key(|&(_, h)| h)),
                    QualityFallback::Lower | QualityFallback::Ask => lower(),
                };
                (picked.map_or(0, |(i, _)| i), Some(format!("{wanted}p")))
            }
        }
    }
}

impl OutputDefaults {
    /// `option` saved as these choices say, as far as it and the video offer them.
    fn selection(
        &self,
        option: &MediaOption,
        subtitles: &[MediaSubtitle],
        chapters: bool,
    ) -> MediaSelection {
        let fit = option.outputs.iter().find(|f| f.format == self.format);
        let format = fit.map_or(OutputFormat::Original, |f| f.format);
        let audio = fit
            .or_else(|| option.outputs.iter().find(|f| f.format == format))
            .map(|f| f.audio);
        let mut selection = self.selection_of(option.id.clone(), subtitles, chapters);
        selection.output = format;
        selection.audio_kbps = audio_kbps(format, audio, self.audio_kbps);
        selection
    }

    /// A selection of the option `option_id` in its original format, with the subtitles and
    /// chapters these choices take from what the video offers.
    fn selection_of(
        &self,
        option_id: String,
        subtitles: &[MediaSubtitle],
        chapters: bool,
    ) -> MediaSelection {
        MediaSelection {
            option_id,
            folder: None,
            file_name: None,
            queue: None,
            later: false,
            output: OutputFormat::Original,
            audio_kbps: None,
            subtitles: preselected_subtitles(
                subtitles,
                &self.subtitle_languages,
                self.auto_subtitles,
            ),
            subtitle_files: self.subtitle_files,
            chapters: chapters && self.chapters,
            every_video: None,
        }
    }
}

/// The audio quality to start from for `format`, whose audio is `audio`: none for a format
/// without a quality to pick; the offered quality nearest the saved one; none when the
/// downloaded audio fits as it is; otherwise the format's usual quality.
fn audio_kbps(
    format: OutputFormat,
    audio: Option<StreamAction>,
    saved: Option<u32>,
) -> Option<u32> {
    let choices = format.kbps_choices();
    if choices.is_empty() {
        return None;
    }
    if let Some(wanted) = saved {
        return choices.iter().copied().min_by_key(|c| c.abs_diff(wanted));
    }
    if audio.is_some_and(|a| a != StreamAction::Encode) {
        return None;
    }
    format.default_kbps().or_else(|| choices.first().copied())
}

/// The subtitles to pick for the saved `languages`, each once: for each language its subtitles
/// written by people, or, when `auto` and it has none, its auto-generated ones. A language the
/// video does not have is skipped, never replaced by another.
#[must_use]
pub fn preselected_subtitles(
    offered: &[MediaSubtitle],
    languages: &[String],
    auto: bool,
) -> Vec<String> {
    let mut picked: Vec<String> = Vec::new();
    for language in languages {
        let mut of_language = offered
            .iter()
            .filter(|s| same_language(&s.language, language));
        let found = of_language.clone().find(|s| !s.auto).or_else(|| {
            if auto {
                of_language.find(|s| s.auto)
            } else {
                None
            }
        });
        if let Some(found) = found
            && !picked.contains(&found.id)
        {
            picked.push(found.id.clone());
        }
    }
    picked
}

/// Whether `language` (a site's code, `en-GB`) is the language `wanted` (`en`, `en-gb`).
fn same_language(language: &str, wanted: &str) -> bool {
    let have = language.to_ascii_lowercase();
    let want = wanted.to_ascii_lowercase();
    have == want || have.split(['-', '_']).next() == Some(want.as_str())
}

/// Saved choices and the videos they meet; `pnpm protocol:generate` writes each with where
/// [`MediaDefaults::start`] starts it, for the TypeScript port to be tested against.
#[cfg(test)]
pub(crate) fn start_cases() -> Vec<(MediaDefaults, Vec<MediaOption>, Vec<MediaSubtitle>, bool)> {
    let heights = [Some(2160), Some(1080), Some(720), Some(360)];
    let videos = || -> Vec<MediaOption> {
        heights
            .iter()
            .map(|h| test_option(*h, false))
            .chain([test_option(None, true)])
            .collect()
    };
    let subtitles = test_subtitles();
    let saved = |quality, fallback, ask_each_time| MediaDefaults {
        ask_each_time,
        quality,
        fallback,
        ..MediaDefaults::default()
    };
    let mut cases = Vec::new();
    for quality in [
        VideoQuality::Best,
        VideoQuality::AudioOnly,
        VideoQuality::Height(1080),
        VideoQuality::Height(1440),
        VideoQuality::Height(4320),
        VideoQuality::Height(240),
    ] {
        for fallback in [
            QualityFallback::Lower,
            QualityFallback::Higher,
            QualityFallback::Ask,
        ] {
            for ask in [true, false] {
                cases.push((saved(quality, fallback, ask), videos(), Vec::new(), false));
            }
        }
    }
    // Only a video without audio alone, a stream that does not say its heights, audio alone.
    let only_videos: Vec<MediaOption> = videos().into_iter().filter(|o| !o.audio_only).collect();
    let no_heights = vec![test_option(None, false), test_option(None, true)];
    let audio_alone = vec![test_option(None, true)];
    for options in [only_videos, no_heights, audio_alone] {
        for quality in [VideoQuality::AudioOnly, VideoQuality::Height(1080)] {
            cases.push((
                saved(quality, QualityFallback::Ask, false),
                options.clone(),
                Vec::new(),
                false,
            ));
        }
    }
    // The saved formats, audio qualities, subtitles and chapters.
    let mut formats = MediaDefaults::default();
    formats.video.format = OutputFormat::Mkv;
    formats.video.subtitle_languages = vec!["bn".into(), "en".into(), "fr".into()];
    formats.video.chapters = true;
    formats.video.subtitle_files = true;
    formats.audio.format = OutputFormat::Mp3;
    formats.audio.audio_kbps = Some(200);
    formats.audio.subtitle_languages = vec!["pt".into()];
    formats.audio.auto_subtitles = false;
    for quality in [VideoQuality::Height(1080), VideoQuality::AudioOnly] {
        formats.quality = quality;
        cases.push((formats.clone(), videos(), subtitles.clone(), true));
        cases.push((formats.clone(), videos(), subtitles.clone(), false));
    }
    let mut webm = formats.clone();
    webm.video.format = OutputFormat::Webm;
    webm.video.auto_subtitles = false;
    webm.quality = VideoQuality::Height(720);
    cases.push((webm, videos(), subtitles.clone(), true));
    let mut usual = formats.clone();
    usual.audio.audio_kbps = None;
    usual.audio.format = OutputFormat::Opus;
    usual.quality = VideoQuality::AudioOnly;
    cases.push((usual, videos(), subtitles, false));
    cases
}

/// A quality of `height`, or audio alone, offered as the original and as MKV (audio: MP3 and
/// Opus, the MP3 converted).
#[cfg(test)]
fn test_option(height: Option<u32>, audio_only: bool) -> MediaOption {
    use crate::app::OutputFit;
    let fit = |format: OutputFormat, audio: StreamAction| OutputFit {
        format,
        ext: "x".into(),
        video: if audio_only {
            StreamAction::None
        } else {
            StreamAction::Copy
        },
        audio,
        video_codec: None,
        audio_codec: None,
        audio_kbps: Some(160),
        lossless_source: false,
        size: None,
        embeds_subtitles: true,
        chapters: true,
    };
    let outputs = if audio_only {
        vec![
            fit(OutputFormat::Original, StreamAction::Copy),
            fit(OutputFormat::Mp3, StreamAction::Encode),
            fit(OutputFormat::Opus, StreamAction::Copy),
        ]
    } else {
        vec![
            fit(OutputFormat::Original, StreamAction::Copy),
            fit(OutputFormat::Mkv, StreamAction::Copy),
        ]
    };
    let quality = match (audio_only, height) {
        (true, _) => "Audio only".to_owned(),
        (false, Some(h)) => format!("{h}p"),
        (false, None) => "Best quality".to_owned(),
    };
    MediaOption {
        id: format!("q-{quality}"),
        label: quality.clone(),
        height,
        audio_only,
        size: None,
        needs_ffmpeg: false,
        quality,
        outputs,
    }
}

/// Subtitles as a page lists them: written by people first, then auto-generated ones.
#[cfg(test)]
fn test_subtitles() -> Vec<MediaSubtitle> {
    let subtitle = |id: &str, language: &str, auto: bool| MediaSubtitle {
        id: id.into(),
        language: language.into(),
        name: language.into(),
        auto,
    };
    vec![
        subtitle("en", "en-GB", false),
        subtitle("pt", "pt-BR", false),
        subtitle("auto:en", "en", true),
        subtitle("auto:bn", "bn", true),
        subtitle("auto:pt", "pt", true),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn start(saved: &MediaDefaults, options: &[MediaOption]) -> (String, Option<String>, bool) {
        let start = saved.start(options, &[], false);
        (start.selection.option_id, start.missing, start.asks)
    }

    fn saved(quality: VideoQuality, fallback: QualityFallback) -> MediaDefaults {
        MediaDefaults {
            quality,
            fallback,
            ask_each_time: false,
            ..MediaDefaults::default()
        }
    }

    fn offered(heights: &[u32]) -> Vec<MediaOption> {
        heights
            .iter()
            .map(|h| test_option(Some(*h), false))
            .chain([test_option(None, true)])
            .collect()
    }

    fn picks(id: &str, missing: Option<&str>, asks: bool) -> (String, Option<String>, bool) {
        (id.to_owned(), missing.map(str::to_owned), asks)
    }

    /// The saved resolution when offered; otherwise the closest lower, else the lowest.
    #[test]
    fn next_lower_takes_the_closest_lower_or_else_the_lowest() {
        let s = saved(VideoQuality::Height(1080), QualityFallback::Lower);
        assert_eq!(
            start(&s, &offered(&[2160, 1080, 720])),
            picks("q-1080p", None, false)
        );
        assert_eq!(
            start(&s, &offered(&[2160, 720, 480])),
            picks("q-720p", Some("1080p"), false)
        );
        assert_eq!(
            start(&s, &offered(&[2160, 1440])),
            picks("q-1440p", Some("1080p"), false)
        );
    }

    /// The closest higher, else the best: the user picked 1080p, the video goes up to 720p.
    #[test]
    fn next_higher_takes_the_closest_higher_or_else_the_best() {
        let s = saved(VideoQuality::Height(1080), QualityFallback::Higher);
        assert_eq!(
            start(&s, &offered(&[2160, 1440, 720])),
            picks("q-1440p", Some("1080p"), false)
        );
        assert_eq!(
            start(&s, &offered(&[720, 480])),
            picks("q-720p", Some("1080p"), false)
        );
    }

    /// Ask opens the window on the closest lower, and only when the resolution is missing.
    #[test]
    fn ask_opens_the_window_only_when_the_resolution_is_missing() {
        let s = saved(VideoQuality::Height(1080), QualityFallback::Ask);
        assert_eq!(
            start(&s, &offered(&[2160, 1080])),
            picks("q-1080p", None, false)
        );
        assert_eq!(
            start(&s, &offered(&[2160, 720])),
            picks("q-720p", Some("1080p"), true)
        );
        let each_time = MediaDefaults {
            ask_each_time: true,
            ..s
        };
        assert_eq!(
            start(&each_time, &offered(&[2160, 1080])),
            picks("q-1080p", None, true)
        );
    }

    /// Best and audio only never need a fallback, nor does a video that does not say its
    /// heights or that is audio alone.
    #[test]
    fn best_audio_and_heightless_videos_need_no_fallback() {
        let ask = |q| saved(q, QualityFallback::Ask);
        let options = offered(&[1080, 720]);
        assert_eq!(
            start(&ask(VideoQuality::Best), &options),
            picks("q-1080p", None, false)
        );
        assert_eq!(
            start(&ask(VideoQuality::AudioOnly), &options),
            picks("q-Audio only", None, false)
        );
        let heightless = vec![test_option(None, false), test_option(None, true)];
        assert_eq!(
            start(&ask(VideoQuality::Height(1080)), &heightless),
            picks("q-Best quality", None, false)
        );
        // A stream with no audio alone: audio only is missing, and asks.
        let videos = vec![test_option(Some(720), false)];
        assert_eq!(
            start(&ask(VideoQuality::AudioOnly), &videos),
            picks("q-720p", Some("Audio only"), true)
        );
    }

    /// Bengali: written by people first, else auto-generated, else nothing; never another
    /// language in its place.
    #[test]
    fn each_saved_language_takes_its_own_subtitles_or_none() {
        let offered = test_subtitles();
        let languages = |l: &[&str]| l.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>();
        assert_eq!(
            preselected_subtitles(&offered, &languages(&["bn", "en", "fr"]), true),
            ["auto:bn", "en"]
        );
        assert_eq!(
            preselected_subtitles(&offered, &languages(&["bn", "en", "fr"]), false),
            ["en"]
        );
        assert_eq!(
            preselected_subtitles(&offered, &languages(&["pt-br", "pt"]), true),
            ["pt"]
        );
    }

    /// The saved format when the quality offers it, else the original; the nearest audio
    /// quality; chapters only when the video has them.
    #[test]
    fn the_saved_format_and_audio_quality_are_kept_when_offered() {
        let mut s = MediaDefaults::default();
        s.video.format = OutputFormat::Mp4;
        s.video.chapters = true;
        s.audio.format = OutputFormat::Mp3;
        s.audio.audio_kbps = Some(200);
        let options = offered(&[1080]);
        let video = s.start(&options, &[], true).selection;
        assert_eq!(
            (video.output, video.chapters),
            (OutputFormat::Original, true)
        );
        assert!(!s.start(&options, &[], false).selection.chapters);
        s.quality = VideoQuality::AudioOnly;
        let audio = s.start(&options, &[], false).selection;
        assert_eq!(
            (audio.output, audio.audio_kbps),
            (OutputFormat::Mp3, Some(192))
        );
        // Opus copies the audio as it is, so no quality is picked unless one was saved.
        s.audio.format = OutputFormat::Opus;
        s.audio.audio_kbps = None;
        assert_eq!(s.start(&options, &[], false).selection.audio_kbps, None);
    }
}
