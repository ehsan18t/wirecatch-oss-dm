// The "Save as" part of a video's quality window (D24): which formats a quality offers, what
// each means (copied in seconds, or converted), the audio qualities, the subtitles to tick first
// and the notes the window shows. The core decides what is offered; this only presents it.

import {
  AUDIO_DEFAULT_KBPS,
  AUDIO_KBPS_CHOICES,
  LOSSLESS_OUTPUT_FORMATS,
  type MediaChoice,
  type MediaOption,
  type MediaSelection,
  type MediaSubtitle,
  type OutputDefaults,
  type OutputFit,
  type OutputFormat,
} from '@wirecatch/protocol';
import { bytesAtBitRate } from '@wirecatch/units';

/** Each format's name for a person. */
export const FORMAT_LABEL: Record<OutputFormat, string> = {
  original: 'Original',
  mp4: 'MP4',
  mkv: 'MKV',
  webm: 'WebM',
  mp3: 'MP3',
  m4a: 'M4A',
  opus: 'Opus',
  flac: 'FLAC',
  wav: 'WAV',
};

/** What a quality means when saved as `format`, if it offers that format. */
export function fitFor(option: MediaOption | undefined, format: OutputFormat): OutputFit | null {
  return option?.outputs.find((f) => f.format === format) ?? null;
}

/** A format in the Save as list: "Original (WebM)", "MP4". */
export function formatChoiceLabel(fit: OutputFit): string {
  if (fit.format !== 'original') return FORMAT_LABEL[fit.format];
  const ext = fit.ext.toLowerCase();
  const named = ext === 'webm' ? 'WebM' : ext === 'opus' ? 'Opus' : ext.toUpperCase();
  return `Original (${named})`;
}

/** The offered quality nearest to `wanted` (the lower one on a tie). */
export function nearestKbps(choices: readonly number[], wanted: number): number {
  let best = choices[0] ?? wanted;
  for (const c of choices) {
    if (Math.abs(c - wanted) < Math.abs(best - wanted)) best = c;
  }
  return best;
}

/**
 * The audio quality to start from for `format`: `null` keeps the downloaded audio when it fits
 * as it is (and nothing else was wanted), otherwise the wanted quality, or the format's usual one.
 * `null` too for formats without a quality to pick.
 */
export function kbpsFor(
  fit: OutputFit | null,
  format: OutputFormat,
  wanted: number | null,
): number | null {
  const choices = AUDIO_KBPS_CHOICES[format];
  if (!choices) return null;
  if (wanted !== null) return nearestKbps(choices, wanted);
  if (fit && fit.audio !== 'encode') return null;
  return AUDIO_DEFAULT_KBPS[format] ?? choices[0] ?? null;
}

/** Whether `language` (a site's code, `en-GB`) is the language `wanted` (`en`, `en-gb`). */
function sameLanguage(language: string, wanted: string): boolean {
  const have = language.toLowerCase();
  const want = wanted.toLowerCase();
  return have === want || have.split(/[-_]/)[0] === want;
}

/**
 * The subtitles to tick first: for each remembered language its human-written subtitles, or,
 * when `auto` and it has none, its auto-generated ones.
 */
export function preselectSubtitles(
  offered: readonly MediaSubtitle[],
  languages: readonly string[],
  auto: boolean,
): string[] {
  const picked: string[] = [];
  for (const language of languages) {
    const human = offered.find((s) => !s.auto && sameLanguage(s.language, language));
    const found =
      human ?? (auto ? offered.find((s) => s.auto && sameLanguage(s.language, language)) : null);
    if (found && !picked.includes(found.id)) picked.push(found.id);
  }
  return picked;
}

/** What the person has chosen in the Save as part of the window. */
export interface OutputChoice {
  format: OutputFormat;
  kbps: number | null;
  subtitles: string[];
  subtitleFiles: boolean;
  chapters: boolean;
}

/** What the window starts from when nothing was chosen before: the download as it comes. */
const NOTHING_CHOSEN: OutputDefaults = {
  format: 'original',
  audio_kbps: null,
  subtitle_languages: [],
  auto_subtitles: false,
  subtitle_files: false,
  chapters: false,
};

/**
 * Where the window starts for `option`: the last choices for its kind (videos or audio only),
 * as far as this quality and video offer them. Original when nothing was chosen before.
 */
export function startingChoice(
  option: MediaOption | undefined,
  choice: MediaChoice,
  defaults: OutputDefaults | null | undefined,
): OutputChoice {
  const last = defaults ?? NOTHING_CHOSEN;
  const format = fitFor(option, last.format) ? last.format : 'original';
  return {
    format,
    kbps: kbpsFor(fitFor(option, format), format, last.audio_kbps),
    subtitles: preselectSubtitles(choice.subtitles, last.subtitle_languages, last.auto_subtitles),
    subtitleFiles: last.subtitle_files,
    chapters: choice.chapters && last.chapters,
  };
}

/** Whether a quality saved as `format` is converted (slow) or copied (seconds); `null` for Original. */
export function rowTag(
  option: MediaOption,
  format: OutputFormat,
  kbps: number | null,
): 'converts' | 'copied' | null {
  if (format === 'original') return null;
  const fit = fitFor(option, format);
  if (!fit) return null;
  if (fit.video === 'encode') return 'converts';
  if (option.audio_only && (fit.audio === 'encode' || kbps !== null)) return 'converts';
  return 'copied';
}

/** One sentence on what saving as `format` does to this quality, before the person starts. */
export function conversionNote(
  fit: OutputFit | null,
  format: OutputFormat,
  kbps: number | null,
): string | null {
  if (!fit || format === 'original') return null;
  const name = FORMAT_LABEL[format];
  if (fit.video === 'none') {
    if (fit.audio === 'encode' || kbps !== null) {
      const at = kbps !== null ? ` at ${kbps} kbps` : '';
      return `The audio is converted to ${name}${at} after it is downloaded, which is quick.`;
    }
    return `No conversion: the audio is copied into ${name} as it is.`;
  }
  if (fit.video === 'encode') {
    return `This quality has no ${name} version, so Wirecatch converts the video to ${fit.video_codec} after downloading it. That takes longer and keeps the processor or graphics card busy.`;
  }
  if (fit.audio === 'encode') {
    const from = fit.audio_codec ? ` (${fit.audio_codec})` : '';
    return `The video is copied as it is; only the audio${from} is converted for ${name}, which is quick.`;
  }
  return `No conversion: the video and audio are copied into ${name} as they are, which takes seconds.`;
}

/** Notes on quality: a bit rate above the download's, or a lossless format from lossy audio. */
export function qualityNotes(
  fit: OutputFit | null,
  format: OutputFormat,
  kbps: number | null,
): string[] {
  if (!fit || format === 'original') return [];
  const notes: string[] = [];
  if (kbps !== null && fit.audio_kbps !== null && kbps > fit.audio_kbps) {
    notes.push(
      `${kbps} kbps is more than the ${fit.audio_kbps} kbps of the download, so it does not add quality; the file only gets bigger.`,
    );
  }
  if (LOSSLESS_OUTPUT_FORMATS.includes(format) && !fit.lossless_source) {
    const codec = fit.audio_codec ?? 'compressed audio';
    notes.push(
      `The download's audio is ${codec}, which already left out some detail. ${FORMAT_LABEL[format]} keeps all that is there but cannot bring that back, so the file only gets bigger.`,
    );
  }
  return notes;
}

/** The estimated size of the file, for the chosen format and audio quality. */
export function sizeFor(
  option: MediaOption,
  format: OutputFormat,
  kbps: number | null,
  durationMs: number | null,
): number | null {
  const fit = fitFor(option, format);
  if (!fit) return option.size;
  if (option.audio_only && kbps !== null && durationMs !== null) {
    return bytesAtBitRate(kbps, durationMs);
  }
  return fit.size;
}

/** The Save as part of a selection, as the core takes it. */
export function outputSelection(
  option: MediaOption,
  choice: MediaChoice,
  picked: OutputChoice,
): Pick<MediaSelection, 'output' | 'audio_kbps' | 'subtitles' | 'subtitle_files' | 'chapters'> {
  const fit = fitFor(option, picked.format);
  const offered = new Set(choice.subtitles.map((s) => s.id));
  return {
    output: fit ? picked.format : 'original',
    audio_kbps: fit && AUDIO_KBPS_CHOICES[picked.format] ? picked.kbps : null,
    subtitles: picked.subtitles.filter((id) => offered.has(id)),
    subtitle_files: picked.subtitleFiles,
    chapters: picked.chapters && choice.chapters && (fit?.chapters ?? false),
  };
}
