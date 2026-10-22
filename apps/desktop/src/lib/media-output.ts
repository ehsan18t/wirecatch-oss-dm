// The "Save as" part of a video's quality window (D24): which formats a quality offers, what
// each means (copied in seconds, or converted), the audio qualities, where the window starts
// and the notes it shows. The core decides what is offered and where the window starts (D50);
// this only presents it.

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

/** Each format's name by its extension, Original aside. */
const NAME_BY_EXTENSION: ReadonlyMap<string, string> = new Map(
  Object.entries(FORMAT_LABEL).filter(([format]) => format !== 'original'),
);

/** A file format by its extension, named as Save as names it ("WebM"), as the core names it in a quality's label; one the app has no name for in capitals. */
export function extensionName(ext: string): string {
  return NAME_BY_EXTENSION.get(ext.toLowerCase()) ?? ext.toUpperCase();
}

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

/** What the person has chosen in the Save as part of the window. */
export interface OutputChoice {
  format: OutputFormat;
  kbps: number | null;
  subtitles: string[];
  subtitleFiles: boolean;
  chapters: boolean;
}

/** Where the window starts: the core's proposal from the saved choices (D50). */
export function proposedChoice(start: MediaSelection): OutputChoice {
  return {
    format: start.output,
    kbps: start.audio_kbps,
    subtitles: [...start.subtitles],
    subtitleFiles: start.subtitle_files,
    chapters: start.chapters,
  };
}

/**
 * Switching between a video and audio only: the last format, audio quality and chapters for
 * the kind of `option`, as far as it and the video offer them (Original when nothing was chosen
 * before), with the subtitles ticked so far, which are the video's whatever is saved.
 */
export function switchedChoice(
  option: MediaOption,
  choice: MediaChoice,
  defaults: OutputDefaults | null | undefined,
  now: OutputChoice,
): OutputChoice {
  const format = defaults && fitFor(option, defaults.format) ? defaults.format : 'original';
  return {
    format,
    kbps: kbpsFor(fitFor(option, format), format, defaults?.audio_kbps ?? null),
    subtitles: now.subtitles,
    subtitleFiles: defaults?.subtitle_files ?? false,
    chapters: choice.chapters && (defaults?.chapters ?? false),
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
    return `This quality has no ${name} version, so Wirecatch converts the video to ${fit.video_codec} after downloading, which takes longer.`;
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

/** Each subtitle's name as the quality window shows it: auto-generated ones marked, and two that would read alike told apart by their language code (`English (auto, en-orig)`). */
export function subtitleLabels(subtitles: readonly MediaSubtitle[]): Map<string, string> {
  const base = (s: MediaSubtitle) => (s.auto ? `${s.name} (auto)` : s.name);
  const seen = new Map<string, number>();
  for (const s of subtitles) seen.set(base(s), (seen.get(base(s)) ?? 0) + 1);
  return new Map(
    subtitles.map((s) => {
      const label = base(s);
      if ((seen.get(label) ?? 0) < 2) return [s.id, label];
      return [s.id, s.auto ? `${s.name} (auto, ${s.language})` : `${s.name} (${s.language})`];
    }),
  );
}
