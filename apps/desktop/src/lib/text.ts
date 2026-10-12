// Small helpers for text shown to the person, kept apart from what the text says.

/**
 * U+2068 FIRST STRONG ISOLATE: what follows, up to the matching pop, takes its direction from its
 * own first strong character and cannot reorder the sentence around it.
 */
export const FIRST_STRONG_ISOLATE = '\u2068';

/** U+2069 POP DIRECTIONAL ISOLATE: ends the isolate opened before it. */
export const POP_DIRECTIONAL_ISOLATE = '\u2069';

/**
 * `text` wrapped in a first strong isolate and its pop, for a name inside a sentence, such as a
 * file name in a title, where right-to-left or control characters in it could otherwise reorder
 * the words around it. The two marks are written as escapes above, so no invisible character
 * sits in the source.
 */
export function isolate(text: string): string {
  return `${FIRST_STRONG_ISOLATE}${text}${POP_DIRECTIONAL_ISOLATE}`;
}

const ORDINAL_RULES = new Intl.PluralRules('en', { type: 'ordinal' });

const ORDINAL_SUFFIX: Record<Intl.LDMLPluralRule, string> = {
  zero: 'th',
  one: 'st',
  two: 'nd',
  few: 'rd',
  many: 'th',
  other: 'th',
};

/** 1st, 2nd, 3rd, 4th, 11th, 21st. */
export function ordinal(n: number): string {
  return `${n}${ORDINAL_SUFFIX[ORDINAL_RULES.select(n)]}`;
}
