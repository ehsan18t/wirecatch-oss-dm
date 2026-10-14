import type { IconComponent } from '../primitives/icon';

interface Common {
  /** Stable within the menu: the key of its row. */
  id: string;
  label: string;
  disabled?: boolean | undefined;
}

export interface MenuAction extends Common {
  kind?: 'action';
  icon?: IconComponent | undefined;
  /** The keys that do the same, shown at the end of the row. */
  shortcut?: readonly string[] | undefined;
  /** A destructive action: shown in the danger colour. */
  danger?: boolean | undefined;
  onselect: () => void;
}

export interface MenuCheckbox extends Common {
  kind: 'checkbox';
  checked: boolean;
  onselect: () => void;
}

export interface MenuSeparator {
  kind: 'separator';
  id: string;
}

export type MenuEntry = MenuAction | MenuCheckbox | MenuSeparator;

/** How a menu was closed: by choosing or by Escape (focus goes back to where it was opened), by Tab (focus goes back there too, and the browser's Tab then moves on from it, as the APG menu does), or by a click elsewhere (focus stays where the click put it). */
export type MenuCloseReason = 'select' | 'escape' | 'tab' | 'away';

/** Key names as a shortcut is shown, mapped to the names `aria-keyshortcuts` takes. */
const ARIA_KEY_NAMES: Record<string, string> = {
  Ctrl: 'Control',
  Cmd: 'Meta',
  Win: 'Meta',
  Esc: 'Escape',
  Del: 'Delete',
  Ins: 'Insert',
};

/** A shown shortcut (`['Ctrl', 'C']`) as `aria-keyshortcuts` writes it (`Control+C`). */
export const ariaKeyShortcut = (keys: readonly string[]): string =>
  keys.map((key) => ARIA_KEY_NAMES[key] ?? key).join('+');

export const isSeparator = (entry: MenuEntry): entry is MenuSeparator => entry.kind === 'separator';

/** Entries as the list keys see them: separators are never landed on. */
export const navigable = (entries: readonly MenuEntry[]) =>
  entries.map((entry) => ({
    label: isSeparator(entry) ? '' : entry.label,
    disabled: isSeparator(entry) || entry.disabled === true,
  }));

/** What the trigger must carry: spread it onto the button that opens the menu. */
export interface MenuTriggerProps {
  'aria-haspopup': 'menu';
  'aria-expanded': boolean;
  'aria-controls': string;
  onclick: () => void;
  onkeydown: (event: KeyboardEvent) => void;
  [key: symbol]: (node: HTMLElement) => void;
}
