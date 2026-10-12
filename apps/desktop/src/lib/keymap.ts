// Every keyboard shortcut of the main window, in one table. The window's and the list's key
// handlers look keys up here, the shortcuts dialog lists them, and the command palette and the
// toolbar show them, so a shortcut is changed in one place.

/** Shortcuts that work anywhere in the main window. */
export type WindowKey = 'palette' | 'paste' | 'add' | 'search' | 'settings' | 'escape' | 'help';

/** Shortcuts of the download list, while it has the keyboard. */
export type ListKey =
  | 'down'
  | 'up'
  | 'extend-down'
  | 'extend-up'
  | 'page-down'
  | 'page-up'
  | 'first'
  | 'last'
  | 'select-all'
  | 'pause-resume'
  | 'open'
  | 'remove'
  | 'nudge-up'
  | 'nudge-down'
  | 'menu';

export type KeyCommand = WindowKey | ListKey;

interface Keys {
  /** `KeyboardEvent.key`; letters match in either case. */
  key: string;
  /**
   * Each modifier must be down (`true`) or up (`false`); left out, it does not matter. Ctrl also
   * means Command on a Mac.
   */
  ctrl?: boolean;
  shift?: boolean;
  alt?: boolean;
  /** Not taken from a text field, where the key already means something. */
  outsideFields?: boolean;
  /** What the shortcuts dialog says; keys with the same text share one line. */
  help?: string;
}

/** A key of the whole window, or of the download list while it has the keyboard. */
export type KeyBinding =
  (Keys & { scope: 'window'; command: WindowKey }) | (Keys & { scope: 'list'; command: ListKey });

export type KeyScope = KeyBinding['scope'];

const MOVE = 'Move through the list';
const EXTEND = 'Select several downloads';
const NUDGE = 'Move the selection up or down in its queue';

/**
 * Every shortcut, in the order the shortcuts dialog lists them. In each scope no key press
 * matches two lines (a test holds this), so the order never decides what a key does.
 */
export const KEYMAP: readonly KeyBinding[] = [
  { scope: 'window', command: 'palette', key: 'k', ctrl: true, help: 'Find any action or setting' },
  // Ctrl+V is the browser's paste, which the window reads as a `paste` event, so pasting from a
  // menu works too. Listed for the dialog; `keyCommand` never returns it.
  { scope: 'window', command: 'paste', key: 'v', ctrl: true, help: 'Download the link you copied' },
  { scope: 'window', command: 'add', key: 'n', ctrl: true, help: 'Add a download' },
  { scope: 'window', command: 'search', key: 'f', ctrl: true, help: 'Search the list' },
  { scope: 'window', command: 'settings', key: ',', ctrl: true, help: 'Open settings' },
  { scope: 'list', command: 'up', key: 'ArrowUp', shift: false, alt: false, help: MOVE },
  { scope: 'list', command: 'down', key: 'ArrowDown', shift: false, alt: false, help: MOVE },
  { scope: 'list', command: 'extend-up', key: 'ArrowUp', shift: true, alt: false, help: EXTEND },
  {
    scope: 'list',
    command: 'extend-down',
    key: 'ArrowDown',
    shift: true,
    alt: false,
    help: EXTEND,
  },
  {
    scope: 'list',
    command: 'select-all',
    key: 'a',
    ctrl: true,
    help: 'Select every download in the list',
  },
  {
    scope: 'list',
    command: 'pause-resume',
    key: ' ',
    help: 'Pause or resume the selected downloads',
  },
  {
    scope: 'list',
    command: 'open',
    key: 'Enter',
    help: 'Open the selected file when it is finished',
  },
  { scope: 'list', command: 'remove', key: 'Delete', help: 'Remove the selected downloads' },
  { scope: 'list', command: 'nudge-up', key: 'ArrowUp', alt: true, help: NUDGE },
  { scope: 'list', command: 'nudge-down', key: 'ArrowDown', alt: true, help: NUDGE },
  {
    scope: 'window',
    command: 'escape',
    key: 'Escape',
    outsideFields: true,
    help: 'Close a dialog or clear the selection',
  },
  { scope: 'window', command: 'help', key: 'F1', help: 'Show these shortcuts' },
  // Shift on the paging keys extends the selection, as it does on the arrows.
  { scope: 'list', command: 'page-up', key: 'PageUp' },
  { scope: 'list', command: 'page-down', key: 'PageDown' },
  { scope: 'list', command: 'first', key: 'Home' },
  { scope: 'list', command: 'last', key: 'End' },
  { scope: 'list', command: 'menu', key: 'ContextMenu' },
  { scope: 'list', command: 'menu', key: 'F10', shift: true },
];

/** How a key is written for people, where `KeyboardEvent.key` differs. */
const KEY_LABEL: Readonly<Record<string, string>> = {
  ArrowUp: 'Up',
  ArrowDown: 'Down',
  ' ': 'Space',
};

/** How each modifier is written for people and for `aria-keyshortcuts`, in that order. */
const MODIFIERS = [
  { flag: 'ctrl', label: 'Ctrl', aria: 'Control' },
  { flag: 'alt', label: 'Alt', aria: 'Alt' },
  { flag: 'shift', label: 'Shift', aria: 'Shift' },
] as const;

/** A field that takes typing, where keys have their own meaning. */
export function isEditable(el: EventTarget | null): boolean {
  if (!(el instanceof HTMLElement)) return false;
  if (el.isContentEditable) return true;
  if (el instanceof HTMLTextAreaElement) return true;
  if (el instanceof HTMLInputElement) {
    return !['checkbox', 'radio', 'button', 'submit', 'range'].includes(el.type);
  }
  return el instanceof HTMLSelectElement;
}

function modifierMatches(wanted: boolean | undefined, down: boolean): boolean {
  return wanted === undefined || wanted === down;
}

/** Whether a key press is this binding's key. */
export function matches(b: KeyBinding, e: KeyboardEvent): boolean {
  return (
    b.key.toLowerCase() === e.key.toLowerCase() &&
    modifierMatches(b.ctrl, e.ctrlKey || e.metaKey) &&
    modifierMatches(b.shift, e.shiftKey) &&
    modifierMatches(b.alt, e.altKey) &&
    !(b.outsideFields === true && isEditable(e.target))
  );
}

/** The shortcut a key press means anywhere in the window, if any. */
export function windowCommand(e: KeyboardEvent): Exclude<WindowKey, 'paste'> | null {
  for (const b of KEYMAP) {
    if (b.scope === 'window' && b.command !== 'paste' && matches(b, e)) return b.command;
  }
  return null;
}

/** The shortcut a key press means in the download list, if any. */
export function listCommand(e: KeyboardEvent): ListKey | null {
  for (const b of KEYMAP) if (b.scope === 'list' && matches(b, e)) return b.command;
  return null;
}

/** The keys pressed together, as people read them: `['Ctrl', 'N']`. */
export function keyParts(b: KeyBinding): string[] {
  const mods = MODIFIERS.filter((m) => b[m.flag] === true).map((m) => m.label);
  const key = KEY_LABEL[b.key] ?? (b.key.length === 1 ? b.key.toUpperCase() : b.key);
  return [...mods, key];
}

/** A key as people read it: `Ctrl+N`, `Shift+Up`. */
export function keyLabel(b: KeyBinding): string {
  return keyParts(b).join('+');
}

/** The first key of a shortcut as people read it, for the command palette. */
export function shortcutLabel(command: KeyCommand): string | undefined {
  const b = KEYMAP.find((x) => x.command === command);
  return b ? keyLabel(b) : undefined;
}

/** The first key of a shortcut in the form `aria-keyshortcuts` takes (`Control+N`). */
export function ariaShortcut(command: KeyCommand): string | undefined {
  const b = KEYMAP.find((x) => x.command === command);
  if (!b) return undefined;
  const mods = MODIFIERS.filter((m) => b[m.flag] === true).map((m) => m.aria);
  return [...mods, b.key.length === 1 ? b.key.toUpperCase() : b.key].join('+');
}

/** One line of the shortcuts dialog: its keys, and what they do. */
export interface ShortcutHelp {
  /** Each key that does it, as the keys pressed together. */
  keys: string[][];
  what: string;
}

/** The shortcuts dialog's lines: keys with the same help text together, in table order. */
export function shortcutHelp(): ShortcutHelp[] {
  const lines: ShortcutHelp[] = [];
  for (const b of KEYMAP) {
    if (b.help === undefined) continue;
    const parts = keyParts(b);
    const line = lines.find((l) => l.what === b.help);
    if (line) line.keys.push(parts);
    else lines.push({ keys: [parts], what: b.help });
  }
  return lines;
}
