// The choices a person has for how videos and audio download (D50), as the app's settings and the extension's popup both offer them (D61): one list for both, so the two never offer different things.

import { AUDIO_KBPS_CHOICES, SUBTITLE_LANGUAGES } from './generated/constants';
import type { OutputFormat } from './generated/OutputFormat';
import type { QualityFallback } from './generated/QualityFallback';
import type { UnembeddedSubtitles } from './generated/UnembeddedSubtitles';
import type { VideoQuality } from './generated/VideoQuality';

/** One entry of a menu: what it saves, what it reads, and other words that find it in a search. */
export interface Choice<T> {
  value: T;
  label: string;
  keywords?: string;
}

/** Each format's name for a person. */
export const FORMAT_LABEL: Readonly<Record<OutputFormat, string>> = {
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

/** The formats of `list` as menu entries. */
export function formatChoices(list: readonly OutputFormat[]): Choice<OutputFormat>[] {
  return list.map((format) => ({ value: format, label: FORMAT_LABEL[format] }));
}

/** The offered quality nearest to `wanted` (the lower one on a tie). */
export function nearestKbps(choices: readonly number[], wanted: number): number {
  let best = choices[0] ?? wanted;
  for (const c of choices) {
    if (Math.abs(c - wanted) < Math.abs(best - wanted)) best = c;
  }
  return best;
}

/** The audio quality to keep when the audio format becomes `format`: the one nearest `kbps` when the format has a choice, else the format's usual one (`null`). */
export function kbpsForFormat(format: OutputFormat, kbps: number | null): number | null {
  const list = AUDIO_KBPS_CHOICES[format];
  return list && kbps !== null ? nearestKbps(list, kbps) : null;
}

/** The audio qualities `format` offers, after its usual one; none for a format without a choice. */
export function kbpsChoices(format: OutputFormat): Choice<number | 'usual'>[] {
  const list = AUDIO_KBPS_CHOICES[format];
  if (!list) return [];
  return [
    { value: 'usual', label: 'Usual quality' },
    ...list.map((kbps) => ({ value: kbps, label: `${String(kbps)} kbps` })),
  ];
}

/** The resolutions offered, tallest first, between Best available and Audio only. */
export const QUALITY_HEIGHTS: readonly number[] = [2160, 1440, 1080, 720, 480, 360];

/** A resolution as a menu holds it: a height, or a word. */
export type QualityValue = number | 'best' | 'audio_only';

export function qualityValue(quality: VideoQuality): QualityValue {
  return typeof quality === 'string' ? quality : quality.height;
}

export function videoQuality(value: QualityValue): VideoQuality {
  return typeof value === 'number' ? { height: value } : value;
}

/** Best available, the usual heights and the saved one if it is another (a pick from the quality window), Audio only. */
export function qualityChoices(saved: VideoQuality): Choice<QualityValue>[] {
  const heights = [...QUALITY_HEIGHTS];
  if (typeof saved !== 'string' && !heights.includes(saved.height)) {
    heights.push(saved.height);
    heights.sort((a, b) => b - a);
  }
  return [
    { value: 'best', label: 'Best available' },
    ...heights.map((h) => ({ value: h, label: `${String(h)}p` })),
    { value: 'audio_only', label: 'Audio only' },
  ];
}

/** What is picked when a video does not offer the saved resolution. */
export const FALLBACK_CHOICES: readonly Choice<QualityFallback>[] = [
  { value: 'lower', label: 'Next lower' },
  { value: 'higher', label: 'Next higher' },
  { value: 'ask', label: 'Ask me' },
];

/** Where a video's subtitles go: `true` is `OutputDefaults.subtitle_files`. */
export const SUBTITLE_PLACEMENT_CHOICES: readonly Choice<'inside' | 'files'>[] = [
  { value: 'inside', label: 'Inside the video' },
  { value: 'files', label: 'As separate files' },
];

/** What happens to subtitles a format cannot hold (D93). */
export const UNEMBEDDED_CHOICES: readonly Choice<UnembeddedSubtitles>[] = [
  { value: 'files', label: 'Save them as separate files' },
  { value: 'skip', label: 'Leave them out' },
];

/** Every subtitle language to pick from (`SUBTITLE_LANGUAGES`), by name, each with its code so either can be searched: "Bangla (bn)", found by "Bengali" too; a code saved earlier that the list lacks (`pt-br`) is kept as it is. */
export function languageChoices(saved: readonly string[]): Choice<string>[] {
  const known = new Set(SUBTITLE_LANGUAGES.map((language) => language.code));
  const choices: Choice<string>[] = SUBTITLE_LANGUAGES.map(({ code, name, also_called }) =>
    also_called === null
      ? { value: code, label: `${name} (${code})` }
      : { value: code, label: `${name} (${code})`, keywords: also_called },
  );
  for (const code of saved) {
    if (!known.has(code)) choices.push({ value: code, label: `${code} (${code})` });
  }
  return choices.sort((a, b) => a.label.localeCompare(b.label));
}
