const LETTERS = 'abcdefghijklmnopqrstuvwxyz';

/** Letters on each side of the hyphen. Six and six is the shape of none of the names the HTML standard reserves (`font-face`, `missing-glyph`). */
const PART_LENGTH = 6;

/** How many values a random byte takes. */
const BYTE_VALUES = 256;

/** Bytes from this value up are dropped, so every letter is equally likely (256 is not a multiple of 26). */
const UNBIASED_LIMIT = LETTERS.length * Math.floor(BYTE_VALUES / LETTERS.length);

/**
 * A custom element name for the button's host, new for every copy of the content
 * script (`kqzvhe-pwrtam`). A page cannot register it ahead of time to stop the
 * button being built, or find the button by a fixed string. The host's presence
 * stays detectable: it is an element the page did not add, at the root of the
 * document, and its name always has this shape.
 */
export function randomHostName(): string {
  let letters = '';
  while (letters.length < 2 * PART_LENGTH) {
    for (const byte of crypto.getRandomValues(new Uint8Array(2 * PART_LENGTH))) {
      if (byte < UNBIASED_LIMIT && letters.length < 2 * PART_LENGTH)
        letters += LETTERS.charAt(byte % LETTERS.length);
    }
  }
  return `${letters.slice(0, PART_LENGTH)}-${letters.slice(PART_LENGTH)}`;
}
