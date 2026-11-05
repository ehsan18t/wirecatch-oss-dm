//! The Save as part of a video's quality window (D24): what a quality offers, what each format
//! does to it, and where the window starts. The core decides what is offered (D50); this presents
//! it.

use gpui_kit::component::button::ToggleGroup;
use gpui_kit::{App, ElementId, Window};
use wirecatch_protocol::app::{
    LanguageSource, MediaChoice, MediaOption, MediaSelection, MediaSubtitle, OutputDefaults,
    OutputFit, OutputFormat, StreamAction, SubtitleChoice, UnembeddedSubtitles,
};
use wirecatch_protocol::languages::LANGUAGES;
use wirecatch_shell::host;

use crate::text::format_name;
use crate::ui::choice;

const BITS_PER_BYTE: u64 = 8;
const SUBTITLE_CHOICES: [(SubtitleChoice, &str); 3] = [
    (SubtitleChoice::Off, "Off"),
    (SubtitleChoice::Inside, "In the video"),
    (SubtitleChoice::Files, "Separate files"),
];

/// What the person has chosen in the Save as part of the window.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OutputChoice {
    pub format: OutputFormat,
    pub kbps: Option<u32>,
    /// Kept while off, so switching on again finds the languages just picked.
    pub subtitles: Vec<String>,
    /// What the person asked for; [`Self::subtitles_shown`] is what the format makes of it.
    pub wanted_subtitles: SubtitleChoice,
    /// Whether the person chose the languages, not the window while switching kind (D195.5).
    pub languages: LanguageSource,
    pub unembedded: UnembeddedSubtitles,
    pub chapters: bool,
}

impl OutputChoice {
    /// Where the window starts: the core's proposal from the saved choices (D50).
    pub fn proposed(start: &MediaSelection, offered: &[MediaSubtitle]) -> Self {
        Self {
            format: start.output,
            kbps: start.audio_kbps,
            subtitles: listed_subtitles(offered, &start.subtitles),
            wanted_subtitles: start.subtitle_choice(),
            languages: LanguageSource::Kept,
            unembedded: start.unembedded_subtitles,
            chapters: start.chapters,
        }
    }

    /// What the choice comes to in `fit`, the one rule the download follows too; nothing selected
    /// is off, as a video with no subtitles is whatever the saved choice wants.
    pub fn subtitles_shown(&self, fit: Option<&OutputFit>) -> SubtitleChoice {
        if self.subtitles.is_empty() {
            return SubtitleChoice::Off;
        }
        self.wanted_subtitles
            .in_format(holds_subtitles(fit), self.unembedded)
    }

    /// The person pressed `pick`. Switching on from off stands by the languages it shows.
    pub fn choose_subtitles(
        &mut self,
        pick: SubtitleChoice,
        offered: &[MediaSubtitle],
        saved: &OutputDefaults,
    ) {
        if self.wanted_subtitles == SubtitleChoice::Off && pick != SubtitleChoice::Off {
            self.languages = LanguageSource::Picked;
        }
        self.take_subtitles(pick, offered, saved);
    }

    /// On from nothing selected takes what `saved` says, so on always has a language.
    fn take_subtitles(
        &mut self,
        pick: SubtitleChoice,
        offered: &[MediaSubtitle],
        saved: &OutputDefaults,
    ) {
        self.wanted_subtitles = pick;
        if pick != SubtitleChoice::Off && self.subtitles.is_empty() {
            self.subtitles = listed_subtitles(offered, &saved.subtitles_when_switched_on(offered));
        }
    }

    /// The person changed the languages; with the last one gone, subtitles are off.
    pub fn pick_languages(&mut self, ids: &[String]) {
        self.languages = LanguageSource::Picked;
        self.subtitles = ids.to_vec();
        if ids.is_empty() {
            self.wanted_subtitles = SubtitleChoice::Off;
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
        let mut switched = Self {
            format,
            kbps: kbps_for(
                fit_for(option, format),
                format,
                defaults.and_then(|saved| saved.audio_kbps),
            ),
            subtitles: self.subtitles.clone(),
            wanted_subtitles: self.wanted_subtitles,
            languages: self.languages,
            unembedded: defaults.map_or(self.unembedded, |saved| saved.unembedded_subtitles),
            chapters: choice.chapters && defaults.is_some_and(|saved| saved.chapters),
        };
        if let Some(saved) = defaults {
            switched.take_subtitles(saved.subtitles, &choice.subtitles, saved);
        }
        switched
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
        selection.subtitles = if self.wanted_subtitles == SubtitleChoice::Off {
            Vec::new()
        } else {
            self.subtitles
                .iter()
                .filter(|id| choice.subtitles.iter().any(|offered| offered.id == **id))
                .cloned()
                .collect()
        };
        selection.subtitle_files = self.wanted_subtitles == SubtitleChoice::Files;
        selection.languages = self.languages;
        selection.unembedded_subtitles = self.unembedded;
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
    if fit.format == OutputFormat::Original {
        format!("Original ({})", file_format(fit))
    } else {
        file_format(fit)
    }
}

/// The format of the file itself: "WebM", "MP4".
fn file_format(fit: &OutputFit) -> String {
    if fit.format != OutputFormat::Original {
        return format_name(fit.format).to_owned();
    }
    let ext = fit.ext.to_lowercase();
    match ext.as_str() {
        "webm" => "WebM".to_owned(),
        "opus" => "Opus".to_owned(),
        _ => ext.to_uppercase(),
    }
}

/// Whether the file can hold a subtitle track. A quality that does not offer the format holds none.
pub fn holds_subtitles(fit: Option<&OutputFit>) -> bool {
    fit.is_some_and(|fit| fit.embeds_subtitles)
}

/// The three-part subtitle control, of the window and of Settings. `shown` is never "In the video"
/// when the format holds no subtitles, since that option is then not pressable.
pub fn subtitle_toggle(
    id: impl Into<ElementId>,
    shown: SubtitleChoice,
    holds: bool,
    pick: impl Fn(&SubtitleChoice, &mut Window, &mut App) + 'static,
    cx: &App,
) -> ToggleGroup {
    let labels = SUBTITLE_CHOICES.map(|(_, label)| label);
    let index_of = |wanted| {
        SUBTITLE_CHOICES
            .iter()
            .position(|(choice, _)| *choice == wanted)
    };
    choice::one_of_except(
        id,
        &labels,
        index_of(shown).unwrap_or_default(),
        index_of(SubtitleChoice::Inside).filter(|_| !holds),
        move |index, window, cx| {
            if let Some((choice, _)) = SUBTITLE_CHOICES.get(*index) {
                pick(choice, window, cx);
            }
        },
        cx,
    )
}

/// Why "In the video" cannot be pressed, and what became of the choice when it had been.
pub fn subtitle_reason(fit: &OutputFit, choice: &OutputChoice) -> Option<String> {
    if fit.embeds_subtitles {
        return None;
    }
    let cannot = format!("{} cannot hold subtitles", file_format(fit));
    Some(match choice.subtitles_shown(Some(fit)) {
        shown if choice.wanted_subtitles == shown => format!("{cannot}."),
        SubtitleChoice::Files => format!("{cannot}, so they are saved as separate files."),
        _ => format!("{cannot}, so they are left out."),
    })
}

/// Subtitles inside a file mean writing it after the download, even when nothing is converted;
/// where something is converted anyway, [`conversion_note`] says what that takes.
pub fn subtitles_added_note(fit: &OutputFit, shown: SubtitleChoice) -> Option<&'static str> {
    let converts = fit.video == StreamAction::Encode || fit.audio == StreamAction::Encode;
    (shown == SubtitleChoice::Inside && !converts)
        .then_some("Subtitles are added after downloading, which takes seconds.")
}

/// `ids` as the window lists them: a language's repeat, which [`shown_subtitles`] leaves out, is
/// that language's listed one.
fn listed_subtitles(offered: &[MediaSubtitle], ids: &[String]) -> Vec<String> {
    let mut listed: Vec<String> = Vec::new();
    for id in ids {
        let first = offered.iter().find(|s| s.id == *id).and_then(|picked| {
            offered
                .iter()
                .find(|s| s.language == picked.language && s.auto == picked.auto)
        });
        if let Some(first) = first
            && !listed.contains(&first.id)
        {
            listed.push(first.id.clone());
        }
    }
    listed
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
/// it is and nothing else was wanted, else the wanted quality or the format's usual one, at or
/// below what the download has, since a higher one only makes the file bigger.
pub fn kbps_for(fit: Option<&OutputFit>, format: OutputFormat, wanted: Option<u32>) -> Option<u32> {
    if wanted.is_none() && fit.is_some_and(|fit| fit.audio != StreamAction::Encode) {
        return None;
    }
    let wanted = wanted.or_else(|| format.default_kbps())?;
    let limit = fit
        .and_then(|fit| fit.audio_kbps)
        .map_or(wanted, |source| source.min(wanted));
    format
        .kbps_choices()
        .iter()
        .copied()
        .filter(|choice| *choice <= limit)
        .max()
        .or_else(|| nearest_kbps(format, limit))
}

/// A video's length: "3:33", "1:02:03".
pub fn video_length(milliseconds: u64) -> String {
    let seconds = milliseconds / 1000;
    let (hours, minutes, seconds) = (seconds / 3600, seconds / 60 % 60, seconds % 60);
    if hours > 0 {
        format!("{hours}:{minutes:02}:{seconds:02}")
    } else {
        format!("{minutes}:{seconds:02}")
    }
}

/// One sentence on what converting to the format does to this quality, before the person starts; `None` when nothing is converted.
pub fn conversion_note(fit: Option<&OutputFit>, kbps: Option<u32>) -> Option<String> {
    let fit = fit.filter(|fit| fit.format != OutputFormat::Original)?;
    let name = format_name(fit.format);
    match fit.video {
        StreamAction::None if fit.audio == StreamAction::Encode || kbps.is_some() => {
            let at = kbps.map_or_else(String::new, |kbps| format!(" at {kbps} kbps"));
            Some(format!(
                "Converted to {name}{at} after downloading, which is quick."
            ))
        }
        StreamAction::Encode => Some(format!(
            "This quality has no {name} version, so {} converts the video to {} after downloading, which takes longer.",
            host::CHANNEL.name(),
            fit.video_codec.as_deref().unwrap_or(name),
        )),
        StreamAction::Copy if fit.audio == StreamAction::Encode => {
            let from = fit
                .audio_codec
                .as_ref()
                .map_or_else(String::new, |codec| format!(" ({codec})"));
            Some(format!(
                "The video is copied as it is; only the audio{from} is converted for {name}, which is quick."
            ))
        }
        StreamAction::None | StreamAction::Copy => None,
    }
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

/// The subtitles the window lists, as id and name: a repeat of a language left out, a site's code
/// given its language's name, auto-generated ones marked, and two that read alike told apart by
/// their code.
pub fn shown_subtitles(subtitles: &[MediaSubtitle]) -> Vec<(String, String)> {
    let mut seen: Vec<(&str, bool)> = Vec::new();
    let kept: Vec<&MediaSubtitle> = subtitles
        .iter()
        .filter(|subtitle| {
            let key = (subtitle.language.as_str(), subtitle.auto);
            let new = !seen.contains(&key);
            seen.push(key);
            new
        })
        .collect();
    let base = |subtitle: &MediaSubtitle| {
        let name = language_name(subtitle);
        if subtitle.auto {
            format!("{name} (auto-generated)")
        } else {
            name
        }
    };
    kept.iter()
        .map(|subtitle| {
            let label = base(subtitle);
            let alike = kept.iter().filter(|other| base(other) == label).count();
            let shown = if alike > 1 {
                let name = language_name(subtitle);
                let code = &subtitle.language;
                if subtitle.auto {
                    format!("{name} (auto-generated, {code})")
                } else {
                    format!("{name} ({code})")
                }
            } else {
                label
            };
            (subtitle.id.clone(), shown)
        })
        .collect()
}

/// The subtitle's name, or its language's when the site gave only a code.
fn language_name(subtitle: &MediaSubtitle) -> String {
    if !subtitle.name.eq_ignore_ascii_case(&subtitle.language) {
        return subtitle.name.clone();
    }
    let code = subtitle.language.split('-').next().unwrap_or_default();
    LANGUAGES
        .iter()
        .find(|language| language.code.eq_ignore_ascii_case(code))
        .map_or_else(
            || subtitle.name.clone(),
            |language| language.name.to_owned(),
        )
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
    fn a_format_starts_at_or_below_what_the_download_has() {
        let copied = fit(OutputFormat::M4a, StreamAction::None, StreamAction::Copy);
        let encoded = fit(OutputFormat::Mp3, StreamAction::None, StreamAction::Encode);
        assert_eq!(
            kbps_for(Some(&encoded), OutputFormat::Mp3, Some(320)),
            Some(128)
        );
        assert_eq!(kbps_for(Some(&encoded), OutputFormat::Mp3, None), Some(128));
        assert_eq!(kbps_for(Some(&copied), OutputFormat::M4a, None), None);
        assert_eq!(kbps_for(None, OutputFormat::Mp3, None), Some(192));
        assert_eq!(kbps_for(None, OutputFormat::Flac, None), None);
    }

    #[test]
    fn a_source_below_every_choice_starts_at_the_lowest() {
        let mut low = fit(OutputFormat::Mp3, StreamAction::None, StreamAction::Encode);
        low.audio_kbps = Some(64);
        assert_eq!(kbps_for(Some(&low), OutputFormat::Mp3, None), Some(128));
    }

    #[test]
    fn a_length_shows_hours_only_when_it_has_them() {
        assert_eq!(video_length(213_000), "3:33");
        assert_eq!(video_length(3_723_000), "1:02:03");
        assert_eq!(video_length(0), "0:00");
    }

    #[test]
    fn only_a_conversion_gets_a_note() {
        let copied = fit(OutputFormat::Mkv, StreamAction::Copy, StreamAction::Copy);
        assert_eq!(conversion_note(Some(&copied), None), None);
        let audio_copied = fit(OutputFormat::M4a, StreamAction::None, StreamAction::Copy);
        assert_eq!(conversion_note(Some(&audio_copied), None), None);
        let audio_for_video = fit(OutputFormat::Mp4, StreamAction::Copy, StreamAction::Encode);
        assert!(
            conversion_note(Some(&audio_for_video), None)
                .unwrap()
                .contains("only the audio (Opus) is converted")
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

    fn choice_of(wanted: SubtitleChoice, unembedded: UnembeddedSubtitles) -> OutputChoice {
        OutputChoice {
            format: OutputFormat::Mp4,
            kbps: None,
            subtitles: vec!["a".to_owned()],
            wanted_subtitles: wanted,
            languages: LanguageSource::Kept,
            unembedded,
            chapters: false,
        }
    }

    #[test]
    fn in_the_video_moves_aside_for_a_format_that_cannot_hold_it_and_comes_back() {
        let holds = fit(OutputFormat::Mp4, StreamAction::Copy, StreamAction::Copy);
        let mut mp3 = fit(OutputFormat::Mp3, StreamAction::None, StreamAction::Encode);
        mp3.embeds_subtitles = false;
        let files = choice_of(SubtitleChoice::Inside, UnembeddedSubtitles::Files);
        assert_eq!(files.subtitles_shown(Some(&holds)), SubtitleChoice::Inside);
        assert_eq!(files.subtitles_shown(Some(&mp3)), SubtitleChoice::Files);
        assert_eq!(files.wanted_subtitles, SubtitleChoice::Inside);
        let skip = choice_of(SubtitleChoice::Inside, UnembeddedSubtitles::Skip);
        assert_eq!(skip.subtitles_shown(Some(&mp3)), SubtitleChoice::Off);
    }

    #[test]
    fn the_reason_names_the_format_and_says_what_moved() {
        let mut mp3 = fit(OutputFormat::Mp3, StreamAction::None, StreamAction::Encode);
        mp3.embeds_subtitles = false;
        let reason = |wanted, unembedded| subtitle_reason(&mp3, &choice_of(wanted, unembedded));
        let files = UnembeddedSubtitles::Files;
        assert_eq!(
            reason(SubtitleChoice::Off, files).as_deref(),
            Some("MP3 cannot hold subtitles.")
        );
        assert_eq!(
            reason(SubtitleChoice::Files, files).as_deref(),
            Some("MP3 cannot hold subtitles.")
        );
        assert_eq!(
            reason(SubtitleChoice::Inside, files).as_deref(),
            Some("MP3 cannot hold subtitles, so they are saved as separate files.")
        );
        assert_eq!(
            reason(SubtitleChoice::Inside, UnembeddedSubtitles::Skip).as_deref(),
            Some("MP3 cannot hold subtitles, so they are left out.")
        );
        let mut original = fit(
            OutputFormat::Original,
            StreamAction::Copy,
            StreamAction::Copy,
        );
        original.ext = "flv".to_owned();
        original.embeds_subtitles = false;
        assert_eq!(
            subtitle_reason(&original, &choice_of(SubtitleChoice::Off, files)).as_deref(),
            Some("FLV cannot hold subtitles.")
        );
        let holds = fit(OutputFormat::Mp4, StreamAction::Copy, StreamAction::Copy);
        assert_eq!(
            subtitle_reason(&holds, &choice_of(SubtitleChoice::Inside, files)),
            None
        );
    }

    #[test]
    fn switching_on_finds_a_language_and_clearing_the_last_one_switches_off() {
        let offered = [
            subtitle("auto:en", "en", "English", true),
            subtitle("auto:en-orig", "en", "English", true),
            subtitle("auto:bn", "bn", "Bangla", true),
        ];
        let mut choice = choice_of(SubtitleChoice::Off, UnembeddedSubtitles::Files);
        choice.subtitles = Vec::new();

        choice.choose_subtitles(SubtitleChoice::Files, &offered, &OutputDefaults::default());
        assert_eq!(choice.wanted_subtitles, SubtitleChoice::Files);
        assert_eq!(
            choice.subtitles,
            ["auto:en"],
            "the video's own language, as the list names it"
        );

        choice.choose_subtitles(SubtitleChoice::Off, &offered, &OutputDefaults::default());
        assert_eq!(choice.subtitles, ["auto:en"]);
        let saved = OutputDefaults {
            subtitle_languages: vec!["bn".to_owned()],
            ..OutputDefaults::default()
        };
        choice.choose_subtitles(SubtitleChoice::Inside, &offered, &saved);
        assert_eq!(choice.subtitles, ["auto:en"], "the languages just picked");

        choice.pick_languages(&[]);
        assert_eq!(choice.wanted_subtitles, SubtitleChoice::Off);
        choice.choose_subtitles(SubtitleChoice::Inside, &offered, &saved);
        assert_eq!(choice.subtitles, ["auto:bn"], "the saved language");
    }

    #[test]
    fn only_the_person_picks_languages() {
        let offered = [subtitle("sub:en", "en", "English", false)];
        let picked_after = |act: &dyn Fn(&mut OutputChoice)| {
            let mut choice = choice_of(SubtitleChoice::Inside, UnembeddedSubtitles::Files);
            act(&mut choice);
            choice.languages == LanguageSource::Picked
        };
        let saved = OutputDefaults::default();
        assert!(picked_after(&|c| c.pick_languages(&["sub:en".to_owned()])));
        assert!(!picked_after(&|c| c.choose_subtitles(
            SubtitleChoice::Files,
            &offered,
            &saved
        )));
        assert!(picked_after(&|c| {
            c.choose_subtitles(SubtitleChoice::Off, &offered, &saved);
            c.choose_subtitles(SubtitleChoice::Inside, &offered, &saved);
        }));

        let (_, page) = page_with_every_kind();
        let option = page.options.first().unwrap().clone();
        let on = OutputDefaults {
            subtitles: SubtitleChoice::Files,
            ..OutputDefaults::default()
        };
        let off = choice_of(SubtitleChoice::Off, UnembeddedSubtitles::Files);
        let switched = off.switched(&option, &page, Some(&on));
        assert_eq!(switched.wanted_subtitles, SubtitleChoice::Files);
        assert_eq!(
            switched.languages,
            LanguageSource::Kept,
            "the window switched it on"
        );
    }

    #[test]
    fn only_subtitles_written_into_a_file_that_converts_nothing_get_their_own_note() {
        let copied = fit(OutputFormat::Mp4, StreamAction::Copy, StreamAction::Copy);
        let encoded = fit(OutputFormat::Mp4, StreamAction::Encode, StreamAction::Copy);
        assert!(subtitles_added_note(&copied, SubtitleChoice::Inside).is_some());
        assert!(subtitles_added_note(&copied, SubtitleChoice::Files).is_none());
        assert!(subtitles_added_note(&copied, SubtitleChoice::Off).is_none());
        assert!(subtitles_added_note(&encoded, SubtitleChoice::Inside).is_none());
    }

    /// A page with every kind of quality, read as the core reads it, and its window's question.
    fn page_with_every_kind() -> (Vec<wirecatch_media::choice::Offer>, MediaChoice) {
        let info: wirecatch_media::ytdlp::Info = serde_json::from_str(
            r#"{"title":"T","duration":2.0,
            "subtitles":{"en":[{"ext":"vtt","url":"https://example.com/en.vtt"}]},
            "formats":[
            {"format_id":"251","ext":"webm","vcodec":"none","acodec":"opus","abr":160,"url":"u"},
            {"format_id":"140","ext":"m4a","vcodec":"none","acodec":"mp4a.40.2","abr":128,"url":"u"},
            {"format_id":"313","ext":"webm","vcodec":"vp9","acodec":"none","height":2160,"tbr":12000,"url":"u"},
            {"format_id":"137","ext":"mp4","vcodec":"avc1.640028","acodec":"none","height":1080,"tbr":4000,"url":"u"},
            {"format_id":"18","ext":"mp4","vcodec":"avc1","acodec":"mp4a","height":360,"tbr":500,"url":"u"},
            {"format_id":"w","ext":"wmv","vcodec":"wmv3","acodec":"wmav2","height":240,"url":"u"}
        ]}"#,
        )
        .unwrap();
        let offers = wirecatch_media::choice::from_page(&info).unwrap();
        let offered = [subtitle("sub:en", "en", "English", false)];
        let options: Vec<MediaOption> = offers.iter().map(|(option, _)| option.clone()).collect();
        let start = wirecatch_protocol::app::MediaDefaults::default()
            .start(&options, &offered, true)
            .selection;
        let window = MediaChoice {
            id: wirecatch_protocol::QuestionId(1),
            title: "T".to_owned(),
            page_url: None,
            options: options.clone(),
            folder: "/v".to_owned(),
            type_folder: None,
            queue: wirecatch_protocol::QueueId(1),
            queue_reason: wirecatch_protocol::rules::QueueReason::BrowserQueue,
            subtitles: offered.to_vec(),
            chapters: false,
            duration_ms: Some(2_000),
            start,
            missing_quality: None,
        };
        (offers, window)
    }

    /// What the window shows, for every format of every kind of quality, is what the core does.
    #[test]
    fn the_window_and_the_download_agree_on_every_format() {
        let (offers, window) = page_with_every_kind();
        let start = window.start.clone();
        let mut none_offered = window.clone();
        none_offered.subtitles = Vec::new();
        let mut formats_seen = Vec::new();
        for (option, plan) in &offers {
            for fit in &option.outputs {
                formats_seen.push(fit.embeds_subtitles);
                for wanted in SUBTITLE_CHOICES.map(|(choice, _)| choice) {
                    for unembedded in [UnembeddedSubtitles::Files, UnembeddedSubtitles::Skip] {
                        let mut output = choice_of(wanted, unembedded);
                        output.format = fit.format;
                        output.subtitles = vec!["sub:en".to_owned()];
                        let shown = output.subtitles_shown(Some(fit));
                        let mut selection = start.clone();
                        selection.option_id.clone_from(&option.id);
                        output.apply(option, &window, &mut selection);
                        let job = plan.resolve(&window, &selection).unwrap().job;
                        let done = match &job {
                            wirecatch_media::MediaJob::Page {
                                output: Some(job), ..
                            } if job.embedded_subtitles() => SubtitleChoice::Inside,
                            wirecatch_media::MediaJob::Page {
                                output: Some(job), ..
                            } if job.sidecar_subtitles() => SubtitleChoice::Files,
                            _ => SubtitleChoice::Off,
                        };
                        let case = (&option.quality, fit.format, wanted, unembedded);
                        assert_eq!(shown, done, "{case:?}");

                        // A video with no subtitles, on whatever the saved choice says.
                        output.subtitles = Vec::new();
                        assert_eq!(
                            output.subtitles_shown(Some(fit)),
                            SubtitleChoice::Off,
                            "{case:?}"
                        );
                        selection = start.clone();
                        selection.option_id.clone_from(&option.id);
                        output.apply(option, &none_offered, &mut selection);
                        assert!(
                            selection.subtitles.is_empty()
                                && selection.subtitle_choice() == SubtitleChoice::Off,
                            "{case:?}"
                        );
                        output.subtitles = vec!["sub:en".to_owned()];
                        assert_eq!(
                            subtitle_reason(fit, &output).is_some(),
                            !fit.embeds_subtitles,
                            "{case:?}"
                        );
                    }
                }
            }
        }
        assert!(formats_seen.contains(&true) && formats_seen.contains(&false));
    }

    #[test]
    fn subtitles_are_named_for_people() {
        let shown = shown_subtitles(&[
            subtitle("a", "en", "English", false),
            subtitle("b", "en", "English", true),
            subtitle("b2", "en", "English", true),
            subtitle("c", "pt-BR", "Portuguese", false),
            subtitle("d", "pt-PT", "Portuguese", false),
            subtitle("e", "de-DE", "de-DE", true),
            subtitle("f", "xx-YY", "xx-YY", false),
        ]);
        let labels: Vec<&str> = shown.iter().map(|(_, label)| label.as_str()).collect();
        assert_eq!(
            labels,
            [
                "English",
                "English (auto-generated)",
                "Portuguese (pt-BR)",
                "Portuguese (pt-PT)",
                "German (auto-generated)",
                "xx-YY",
            ]
        );
    }
}
