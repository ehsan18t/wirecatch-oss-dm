// The choices a person has for how videos and audio download (D50), as the app's settings and the extension's popup both offer them (D61): one list for both, so the two never offer different things.

import { AUDIO_KBPS_CHOICES } from './generated/constants';
import type { OutputFormat } from './generated/OutputFormat';
import type { QualityFallback } from './generated/QualityFallback';
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

// Every two-letter ISO 639-1 code, named in English by the system's own language data, so this list holds no names of its own.
// prettier-ignore
const LANGUAGE_CODES = [
  'aa', 'ab', 'af', 'ak', 'am', 'an', 'ar', 'as', 'av', 'ay', 'az', 'ba', 'be', 'bg', 'bi', 'bm',
  'bn', 'bo', 'br', 'bs', 'ca', 'ce', 'ch', 'co', 'cr', 'cs', 'cu', 'cv', 'cy', 'da', 'de', 'dv',
  'dz', 'ee', 'el', 'en', 'eo', 'es', 'et', 'eu', 'fa', 'ff', 'fi', 'fj', 'fo', 'fr', 'fy', 'ga',
  'gd', 'gl', 'gn', 'gu', 'gv', 'ha', 'he', 'hi', 'ho', 'hr', 'ht', 'hu', 'hy', 'hz', 'ia', 'id',
  'ie', 'ig', 'ii', 'ik', 'io', 'is', 'it', 'iu', 'ja', 'jv', 'ka', 'kg', 'ki', 'kj', 'kk', 'kl',
  'km', 'kn', 'ko', 'kr', 'ks', 'ku', 'kv', 'kw', 'ky', 'la', 'lb', 'lg', 'li', 'ln', 'lo', 'lt',
  'lu', 'lv', 'mg', 'mh', 'mi', 'mk', 'ml', 'mn', 'mr', 'ms', 'mt', 'my', 'na', 'nb', 'nd', 'ne',
  'ng', 'nl', 'nn', 'no', 'nr', 'nv', 'ny', 'oc', 'oj', 'om', 'or', 'os', 'pa', 'pi', 'pl', 'ps',
  'pt', 'qu', 'rm', 'rn', 'ro', 'ru', 'rw', 'sa', 'sc', 'sd', 'se', 'sg', 'si', 'sk', 'sl', 'sm',
  'sn', 'so', 'sq', 'sr', 'ss', 'st', 'su', 'sv', 'sw', 'ta', 'te', 'tg', 'th', 'ti', 'tk', 'tl',
  'tn', 'to', 'tr', 'ts', 'tt', 'tw', 'ty', 'ug', 'uk', 'ur', 'uz', 've', 'vi', 'vo', 'wa', 'wo',
  'xh', 'yi', 'yo', 'za', 'zh', 'zu',
];

const LANGUAGE_NAMES = new Intl.DisplayNames(['en'], { type: 'language', fallback: 'code' });

/** Names many people still search by, for languages the system now names otherwise ("Bangla"). */
const ALSO_CALLED: Readonly<Record<string, string>> = {
  bn: 'Bengali',
  dv: 'Dhivehi Maldivian',
  ii: 'Nuosu',
  kl: 'Greenlandic',
  ky: 'Kirghiz',
  ny: 'Chichewa Chewa',
  or: 'Oriya',
  os: 'Ossetic',
  pa: 'Panjabi',
  ug: 'Uighur',
};

/** A language code in words ("Bengali"), or the code itself when the system has no name for it. */
function languageName(code: string): string {
  try {
    return LANGUAGE_NAMES.of(code) ?? code;
  } catch {
    // A code the system cannot read (it was typed before this list existed).
    return code;
  }
}

/** Every subtitle language to pick from, by name, each with its code so either can be searched: "Bangla (bn)", found by "Bengali" too. A language the system has no name for is left out, unless it was saved earlier (`pt-br`): a bare code says nothing to most people. */
export function languageChoices(saved: readonly string[]): Choice<string>[] {
  const named = LANGUAGE_CODES.filter((code) => languageName(code) !== code);
  const codes = [...new Set([...named, ...saved])];
  return codes
    .map((code) => {
      const keywords = ALSO_CALLED[code];
      const label = `${languageName(code)} (${code})`;
      return keywords === undefined ? { value: code, label } : { value: code, label, keywords };
    })
    .sort((a, b) => a.label.localeCompare(b.label));
}
