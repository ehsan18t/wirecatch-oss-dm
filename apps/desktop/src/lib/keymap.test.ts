import { describe, expect, it } from 'vitest';
import {
  ariaShortcut,
  KEYMAP,
  keyLabel,
  listCommand,
  shortcutHelp,
  shortcutLabel,
  windowCommand,
  type KeyBinding,
  type KeyCommand,
} from './keymap';

function press(
  key: string,
  mods: Partial<KeyboardEventInit> = {},
  target?: Element,
): KeyboardEvent {
  const e = new KeyboardEvent('keydown', { key, bubbles: true, ...mods });
  if (target) target.dispatchEvent(e);
  return e;
}

/** Every shortcut name; the type makes this list whole. */
const EVERY_COMMAND: Record<KeyCommand, true> = {
  palette: true,
  paste: true,
  add: true,
  search: true,
  settings: true,
  escape: true,
  help: true,
  'next-region': true,
  'previous-region': true,
  down: true,
  up: true,
  'extend-down': true,
  'extend-up': true,
  'page-down': true,
  'page-up': true,
  first: true,
  last: true,
  'select-all': true,
  'pause-resume': true,
  open: true,
  details: true,
  remove: true,
  'nudge-up': true,
  'nudge-down': true,
  menu: true,
};

/** Two bindings of one scope that some key press would match both. */
function overlap(a: KeyBinding, b: KeyBinding): boolean {
  if (a.scope !== b.scope || a.key.toLowerCase() !== b.key.toLowerCase()) return false;
  const differ = (x: boolean | undefined, y: boolean | undefined) =>
    x !== undefined && y !== undefined && x !== y;
  return !(differ(a.ctrl, b.ctrl) || differ(a.shift, b.shift) || differ(a.alt, b.alt));
}

describe('keymap', () => {
  it('never lets one key press mean two things in a scope', () => {
    const clashes = KEYMAP.flatMap((a, i) =>
      KEYMAP.slice(i + 1)
        .filter((b) => overlap(a, b))
        .map((b) => `${keyLabel(a)} is ${a.command} and ${b.command}`),
    );
    expect(clashes).toEqual([]);
  });

  it('has a key for every shortcut', () => {
    const missing = Object.keys(EVERY_COMMAND).filter((c) => !KEYMAP.some((b) => b.command === c));
    expect(missing).toEqual([]);
  });

  it('reads window shortcuts, with Ctrl or Command, in either case', () => {
    expect(windowCommand(press('P', { ctrlKey: true, shiftKey: true }))).toBe('palette');
    expect(windowCommand(press('p', { ctrlKey: true }))).toBeNull();
    expect(windowCommand(press('N', { ctrlKey: true, shiftKey: true }))).toBe('add');
    expect(windowCommand(press(',', { metaKey: true }))).toBe('settings');
    expect(windowCommand(press('k', { ctrlKey: true }))).toBe('search');
    expect(windowCommand(press('F1'))).toBe('help');
    expect(windowCommand(press('n'))).toBeNull();
  });

  it('leaves Ctrl+V to the paste event', () => {
    expect(windowCommand(press('v', { ctrlKey: true }))).toBeNull();
  });

  it('takes Escape only outside text fields', () => {
    const field = document.createElement('input');
    const box = document.createElement('input');
    box.type = 'checkbox';
    document.body.append(field, box);
    expect(windowCommand(press('Escape', {}, field))).toBeNull();
    expect(windowCommand(press('Escape', {}, box))).toBe('escape');
    expect(windowCommand(press('Escape', {}, document.body))).toBe('escape');
    field.remove();
    box.remove();
  });

  it('tells moving, extending and reordering apart in the list', () => {
    expect(listCommand(press('ArrowDown'))).toBe('down');
    expect(listCommand(press('ArrowDown', { ctrlKey: true }))).toBe('down');
    expect(listCommand(press('ArrowUp', { shiftKey: true }))).toBe('extend-up');
    expect(listCommand(press('ArrowUp', { altKey: true }))).toBe('nudge-up');
    expect(listCommand(press('ArrowDown', { altKey: true, shiftKey: true }))).toBe('nudge-down');
    expect(listCommand(press('a'))).toBeNull();
    expect(listCommand(press('A', { ctrlKey: true }))).toBe('select-all');
    expect(listCommand(press('F10'))).toBeNull();
    expect(listCommand(press('F10', { shiftKey: true }))).toBe('menu');
    expect(listCommand(press('ContextMenu'))).toBe('menu');
    expect(listCommand(press(' '))).toBe('pause-resume');
    expect(listCommand(press('x'))).toBeNull();
  });

  it('writes keys the way people read them', () => {
    expect(shortcutLabel('settings')).toBe('Ctrl+,');
    expect(shortcutLabel('nudge-down')).toBe('Alt+Down');
    expect(shortcutLabel('pause-resume')).toBe('Space');
    expect(ariaShortcut('add')).toBe('Control+N');
    expect(ariaShortcut('search')).toBe('Control+K');
    expect(shortcutLabel('palette')).toBe('Ctrl+Shift+P');
  });

  it('lists each shortcut once in the dialog, keys with one meaning on one line', () => {
    const lines = shortcutHelp().map((l) => [l.keys.map((k) => k.join('+')).join(', '), l.what]);
    expect(lines).toEqual([
      ['Ctrl+Shift+P', 'Find any action or setting'],
      ['Ctrl+V', 'Download the link you copied'],
      ['Ctrl+N', 'Add a download'],
      ['Ctrl+K', 'Search the list'],
      ['Ctrl+,', 'Open settings'],
      ['Up, Down', 'Move through the list'],
      ['Shift+Up, Shift+Down', 'Select several downloads'],
      ['Ctrl+A', 'Select every download in the list'],
      ['Space', 'Pause or resume the selected downloads'],
      ['Enter', 'Open the selected file when it is finished'],
      ['Alt+Enter', 'Show the details of the selected download'],
      ['Delete', 'Remove the selected downloads'],
      ['Alt+Up, Alt+Down', 'Move the selection up or down in its queue'],
      ['Escape', 'Close a dialog, or clear the selection'],
      ['F1', 'Show these shortcuts'],
      ['F6, Shift+F6', 'Move between the title bar, the navigation and the list'],
    ]);
  });
});
