//! The Save as part of a video's quality window (D24): what a quality offers, what each format
//! does to it, and where the window starts. The core decides what is offered (D50); this presents
//! it.

use wirecatch_protocol::app::{
    MediaChoice, MediaOption, MediaSelection, MediaSubtitle, OutputDefaults, OutputFit,
    OutputFormat, StreamAction,
};
use wirecatch_shell::host;

use crate::text::format_name;

const BITS_PER_BYTE: u64 = 8;

/// What the person has chosen in the Save as part of the window.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OutputChoice {
    pub format: OutputFormat,
    pub kbps: Option<u32>,
    pub subtitles: Vec<String>,
    pub subtitle_files: bool,
    pub chapters: bool,
}

impl OutputChoice {
    /// Where the window starts: the core's proposal from the saved choices (D50).
    pub fn proposed(start: &MediaSelection) -> Self {
        Self {
            format: start.output,
            kbps: start.audio_kbps,
            subtitles: start.subtitles.clone(),
            subtitle_files: start.subtitle_files,
            chapters: start.chapters,
        }
    }

    /// Switching between a video and audio only: the last choices for `option`'s kind, as far as
    /// it and the video offer them, keeping the subtitles ticked, which are this video's.
    pub fn switched(
        &self,
        option: &MediaOption,
        choice: &MediaChoice,
        defaults: Option<&OutputDefaults>,
    ) -> Self {
        let format = defaults
            .map(|saved| saved.format)
            .filter(|format| fit_for(option, *format).is_some())
            .unwrap_or_default();
        Self {
            format,
            kbps: kbps_for(
                fit_for(option, format),
                format,
                defaults.and_then(|saved| saved.audio_kbps),
            ),
            subtitles: self.subtitles.clone(),
            subtitle_files: defaults.is_some_and(|saved| saved.subtitle_files),
            chapters: choice.chapters && defaults.is_some_and(|saved| saved.chapters),
        }
    }

    /// Writes this into `selection`, leaving out what the quality or the video does not offer.
    pub fn apply(
        &self,
        option: &MediaOption,
        choice: &MediaChoice,
        selection: &mut MediaSelection,
    ) {
        let fit = fit_for(option, self.format);
        selection.output = if fit.is_some() {
            self.format
        } else {
            OutputFormat::Original
        };
        selection.audio_kbps = self
            .kbps
            .filter(|_| fit.is_some() && !self.format.kbps_choices().is_empty());
        selection.subtitles = self
            .subtitles
            .iter()
            .filter(|id| choice.subtitles.iter().any(|offered| offered.id == **id))
            .cloned()
            .collect();
        selection.subtitle_files = self.subtitle_files;
        selection.chapters =
            self.chapters && choice.chapters && fit.is_some_and(|fit| fit.chapters);
    }
}

/// What a quality means when saved as `format`, if it offers that format.
pub fn fit_for(option: &MediaOption, format: OutputFormat) -> Option<&OutputFit> {
    option.outputs.iter().find(|fit| fit.format == format)
}

/// A format in the Save as list: "Original (WebM)", "MP4".
pub fn format_label(fit: &OutputFit) -> String {
    if fit.format != OutputFormat::Original {
        return format_name(fit.format).to_owned();
    }
    let ext = fit.ext.to_lowercase();
    let named = match ext.as_str() {
        "webm" => "WebM".to_owned(),
        "opus" => "Opus".to_owned(),
        _ => ext.to_uppercase(),
    };
    format!("Original ({named})")
}

/// The bit rate `format` offers nearest `wanted`.
pub fn nearest_kbps(format: OutputFormat, wanted: u32) -> Option<u32> {
    format
        .kbps_choices()
        .iter()
        .copied()
        .min_by_key(|choice| (choice.abs_diff(wanted), *choice))
}

/// The audio quality to start from for `format`: `None` keeps the downloaded audio when it fits as
/// it is and nothing else was wanted, else the wanted quality, or the format's usual one.
pub fn kbps_for(fit: Option<&OutputFit>, format: OutputFormat, wanted: Option<u32>) -> Option<u32> {
    if let Some(wanted) = wanted {
        return nearest_kbps(format, wanted);
    }
    if fit.is_some_and(|fit| fit.audio != StreamAction::Encode) {
        return None;
    }
    format.default_kbps()
}

/// One sentence on what saving as `format` does to this quality, before the person starts.
pub fn conversion_note(fit: Option<&OutputFit>, kbps: Option<u32>) -> Option<String> {
    let fit = fit.filter(|fit| fit.format != OutputFormat::Original)?;
    let name = format_name(fit.format);
    Some(match fit.video {
        StreamAction::None if fit.audio == StreamAction::Encode || kbps.is_some() => {
            let at = kbps.map_or_else(String::new, |kbps| format!(" at {kbps} kbps"));
            format!("The audio is converted to {name}{at} after it is downloaded, which is quick.")
        }
        StreamAction::None => format!("No conversion: the audio is copied into {name} as it is."),
        StreamAction::Encode => format!(
            "This quality has no {name} version, so {} converts the video to {} after downloading, which takes longer.",
            host::CHANNEL.name(),
            fit.video_codec.as_deref().unwrap_or(name),
        ),
        StreamAction::Copy if fit.audio == StreamAction::Encode => {
            let from = fit
                .audio_codec
                .as_ref()
                .map_or_else(String::new, |codec| format!(" ({codec})"));
            format!(
                "The video is copied as it is; only the audio{from} is converted for {name}, which is quick."
            )
        }
        StreamAction::Copy => format!(
            "No conversion: the video and audio are copied into {name} as they are, which takes seconds."
        ),
    })
}

/// Notes on quality: a bit rate above the download's, or a lossless format from lossy audio.
pub fn quality_notes(fit: Option<&OutputFit>, kbps: Option<u32>) -> Vec<String> {
    let Some(fit) = fit.filter(|fit| fit.format != OutputFormat::Original) else {
        return Vec::new();
    };
    let mut notes = Vec::new();
    if let (Some(kbps), Some(source)) = (kbps, fit.audio_kbps)
        && kbps > source
    {
        notes.push(format!(
            "{kbps} kbps is more than the {source} kbps of the download, so it does not add quality; the file only gets bigger."
        ));
    }
    if matches!(fit.format, OutputFormat::Flac | OutputFormat::Wav) && !fit.lossless_source {
        let codec = fit.audio_codec.as_deref().unwrap_or("compressed audio");
        notes.push(format!(
            "The download's audio is {codec}, which already left out some detail. {} keeps all that is there but cannot bring it back.",
            format_name(fit.format),
        ));
    }
    notes
}

/// The estimated size of the file, for the chosen format and audio quality.
pub fn size_for(
    option: &MediaOption,
    format: OutputFormat,
    kbps: Option<u32>,
    duration_ms: Option<u64>,
) -> Option<u64> {
    let Some(fit) = fit_for(option, format) else {
        return option.size;
    };
    if let (true, Some(kbps), Some(duration)) = (option.audio_only, kbps, duration_ms) {
        return Some(u64::from(kbps).saturating_mul(duration) / BITS_PER_BYTE);
    }
    fit.size
}

/// Each subtitle's name as the window shows it: auto-generated ones marked, and two that would
/// read alike told apart by their language code.
pub fn subtitle_labels(subtitles: &[MediaSubtitle]) -> Vec<String> {
    let base = |subtitle: &MediaSubtitle| {
        if subtitle.auto {
            format!("{} (auto)", subtitle.name)
        } else {
            subtitle.name.clone()
        }
    };
    subtitles
        .iter()
        .map(|subtitle| {
            let label = base(subtitle);
            let alike = subtitles
                .iter()
                .filter(|other| base(other) == label)
                .count();
            match (alike > 1, subtitle.auto) {
                (false, _) => label,
                (true, true) => format!("{} (auto, {})", subtitle.name, subtitle.language),
                (true, false) => format!("{} ({})", subtitle.name, subtitle.language),
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fit(format: OutputFormat, video: StreamAction, audio: StreamAction) -> OutputFit {
        OutputFit {
            format,
            ext: "webm".to_owned(),
            video,
            audio,
            video_codec: Some("H.264".to_owned()),
            audio_codec: Some("Opus".to_owned()),
            audio_kbps: Some(160),
            lossless_source: false,
            size: Some(1000),
            embeds_subtitles: true,
            chapters: true,
        }
    }

    fn option(audio_only: bool, outputs: Vec<OutputFit>) -> MediaOption {
        MediaOption {
            id: "best".to_owned(),
            label: "1080p, WebM".to_owned(),
            height: None,
            audio_only,
            size: Some(500),
            needs_ffmpeg: false,
            quality: "1080p".to_owned(),
            outputs,
        }
    }

    fn subtitle(id: &str, language: &str, name: &str, auto: bool) -> MediaSubtitle {
        MediaSubtitle {
            id: id.to_owned(),
            language: language.to_owned(),
            name: name.to_owned(),
            auto,
        }
    }

    #[test]
    fn the_original_names_the_format_the_site_sends() {
        let original = fit(
            OutputFormat::Original,
            StreamAction::Copy,
            StreamAction::Copy,
        );
        assert_eq!(format_label(&original), "Original (WebM)");
        let mp4 = fit(OutputFormat::Mp4, StreamAction::Copy, StreamAction::Copy);
        assert_eq!(format_label(&mp4), "MP4");
    }

    #[test]
    fn a_format_starts_from_the_wanted_bit_rate_or_the_download_or_its_usual_one() {
        let copied = fit(OutputFormat::M4a, StreamAction::None, StreamAction::Copy);
        let encoded = fit(OutputFormat::Mp3, StreamAction::None, StreamAction::Encode);
        assert_eq!(
            kbps_for(Some(&encoded), OutputFormat::Mp3, Some(200)),
            Some(192)
        );
        assert_eq!(kbps_for(Some(&copied), OutputFormat::M4a, None), None);
        assert_eq!(kbps_for(Some(&encoded), OutputFormat::Mp3, None), Some(192));
        assert_eq!(kbps_for(None, OutputFormat::Flac, None), None);
    }

    #[test]
    fn the_note_says_what_happens_to_the_file() {
        let copied = fit(OutputFormat::Mp4, StreamAction::Copy, StreamAction::Copy);
        assert!(
            conversion_note(Some(&copied), None)
                .unwrap()
                .starts_with("No conversion")
        );
        let encoded = fit(OutputFormat::Mp4, StreamAction::Encode, StreamAction::Copy);
        assert!(
            conversion_note(Some(&encoded), None)
                .unwrap()
                .contains("takes longer")
        );
        let audio = fit(OutputFormat::Mp3, StreamAction::None, StreamAction::Encode);
        assert!(
            conversion_note(Some(&audio), Some(192))
                .unwrap()
                .contains("at 192 kbps")
        );
        let original = fit(
            OutputFormat::Original,
            StreamAction::Copy,
            StreamAction::Copy,
        );
        assert_eq!(conversion_note(Some(&original), None), None);
    }

    #[test]
    fn a_higher_bit_rate_or_lossless_format_says_it_adds_no_quality() {
        let mp3 = fit(OutputFormat::Mp3, StreamAction::None, StreamAction::Encode);
        assert_eq!(quality_notes(Some(&mp3), Some(128)).len(), 0);
        assert_eq!(quality_notes(Some(&mp3), Some(320)).len(), 1);
        let flac = fit(OutputFormat::Flac, StreamAction::None, StreamAction::Encode);
        assert!(quality_notes(Some(&flac), None)[0].contains("cannot bring it back"));
    }

    #[test]
    fn audio_only_at_a_picked_bit_rate_sizes_by_its_length() {
        let mp3 = option(
            true,
            vec![fit(
                OutputFormat::Mp3,
                StreamAction::None,
                StreamAction::Encode,
            )],
        );
        assert_eq!(
            size_for(&mp3, OutputFormat::Mp3, Some(128), Some(1000)),
            Some(16_000)
        );
        assert_eq!(
            size_for(&mp3, OutputFormat::Mp3, None, Some(1000)),
            Some(1000)
        );
        assert_eq!(size_for(&mp3, OutputFormat::Wav, None, None), Some(500));
    }

    #[test]
    fn subtitles_that_read_alike_are_told_apart() {
        let labels = subtitle_labels(&[
            subtitle("a", "en", "English", false),
            subtitle("b", "en", "English", true),
            subtitle("c", "pt-BR", "Portuguese", false),
            subtitle("d", "pt-PT", "Portuguese", false),
        ]);
        assert_eq!(
            labels,
            [
                "English",
                "English (auto)",
                "Portuguese (pt-BR)",
                "Portuguese (pt-PT)"
            ]
        );
    }
}
