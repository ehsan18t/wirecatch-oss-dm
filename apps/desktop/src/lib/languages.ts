// The subtitle languages a person can save for videos and audio (D50): every two-letter ISO 639-1 code, named in English by the system's own language data, so this list holds no names of its own. Codes the person saved earlier (`pt-br`) are offered too.

import type { SelectOption } from '@wirecatch/ui';

// prettier-ignore
const CODES = [
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

const NAMES = new Intl.DisplayNames(['en'], { type: 'language', fallback: 'code' });

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
    return NAMES.of(code) ?? code;
  } catch {
    // A code the system cannot read (it was typed before this list existed).
    return code;
  }
}

/** Every language to pick from, by name, each with its code so either can be searched: "Bangla (bn)", found by "Bengali" too. */
export function languageOptions(saved: readonly string[]): SelectOption[] {
  const codes = [...new Set([...CODES, ...saved])];
  return codes
    .map((code) => ({
      value: code,
      label: `${languageName(code)} (${code})`,
      keywords: ALSO_CALLED[code],
    }))
    .sort((a, b) => a.label.localeCompare(b.label));
}
