// How a key is drawn: arrows as arrows, everything else by its name.

const GLYPH: Readonly<Record<string, string>> = {
  Up: '↑',
  Down: '↓',
  Left: '←',
  Right: '→',
  ArrowUp: '↑',
  ArrowDown: '↓',
  ArrowLeft: '←',
  ArrowRight: '→',
  Escape: 'Esc',
};

/** The key as it is drawn on a key cap or in a menu: `Up` is `↑`. */
export function keyGlyph(key: string): string {
  return GLYPH[key] ?? key;
}

/** A shortcut as plain text, the way a menu shows it: `Alt+↑`. */
export function shortcutText(keys: readonly string[]): string {
  return keys.map(keyGlyph).join('+');
}
