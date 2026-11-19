/**
 * The popup's options as the app describes them (D280). Each option is read on its own: one this
 * version cannot read is left out, and one of a kind it does not know shows as its label and an
 * "Open in Wirecatch" button, so an app newer than the extension never breaks the popup.
 */
import {
  BROWSER_ROWS,
  MAX_OPTION_CHOICES,
  MAX_OPTION_GROUPS,
  MAX_OPTION_TEXT_CHARS,
  MAX_OPTIONS_PER_GROUP,
  OPTION_KINDS,
  type OptionList,
} from '@wirecatch/protocol';

/** A value an option holds or offers. */
export type OptionValue = string | number | boolean;

export interface OptionChoice {
  value: OptionValue;
  label: string;
  keywords?: string;
}

/** What every option has. */
interface Common {
  id: string;
  label: string;
  help: string | null;
  /** Why it cannot be changed now; null when it can. */
  disabled: string | null;
}

/** How a number is typed: in `unit`, as the value divided by `scale`. */
export interface NumberField {
  unit: string;
  scale: number;
  min: number;
  max: number | null;
  step: number;
  decimals: number;
}

export type ParsedOption =
  | (Common & { kind: 'toggle'; value: boolean })
  | (Common & { kind: 'choice'; value: OptionValue; choices: OptionChoice[] })
  | (Common & {
      kind: 'choices';
      value: OptionValue[];
      choices: OptionChoice[];
      placeholder: string | null;
      searchPlaceholder: string | null;
    })
  | (Common & { kind: 'number'; value: number; field: NumberField })
  | (Common & { kind: 'limit'; value: number | null; onValue: number; field: NumberField })
  | (Common & { kind: 'action' })
  | (Common & { kind: 'note' })
  | { kind: 'browser'; id: string; row: 'shortcut' | 'page_site' }
  /** A kind this version does not know: its label, and a way to the app's settings. */
  | { kind: 'unknown'; id: string; label: string };

export interface ParsedGroup {
  id: string;
  /** Null for a card that does not fold. */
  title: string | null;
  summary: string | null;
  folded: boolean;
  options: ParsedOption[];
}

type Raw = Record<string, unknown>;

const isRaw = (value: unknown): value is Raw =>
  typeof value === 'object' && value !== null && !Array.isArray(value);

/** `value` as text the popup shows, cut to the longest it shows; null for anything else. */
function text(value: unknown): string | null {
  return typeof value === 'string' ? value.slice(0, MAX_OPTION_TEXT_CHARS) : null;
}

function isOptionValue(value: unknown): value is OptionValue {
  return (
    typeof value === 'string' ||
    typeof value === 'boolean' ||
    (typeof value === 'number' && Number.isFinite(value))
  );
}

function finite(value: unknown): number | null {
  return typeof value === 'number' && Number.isFinite(value) ? value : null;
}

function choicesOf(value: unknown): OptionChoice[] | null {
  if (!Array.isArray(value)) return null;
  const choices: OptionChoice[] = [];
  for (const raw of value.slice(0, MAX_OPTION_CHOICES)) {
    if (!isRaw(raw) || !isOptionValue(raw.value)) continue;
    const label = text(raw.label);
    if (label === null) continue;
    const keywords = text(raw.keywords);
    choices.push(
      keywords === null ? { value: raw.value, label } : { value: raw.value, label, keywords },
    );
  }
  return choices;
}

/** The most decimals a number field shows. */
const MAX_DECIMALS = 6;
/** Numbers are shown in base ten. */
const DECIMAL_BASE = 10;

function numberField(raw: Raw): NumberField | null {
  const scale = finite(raw.scale);
  const min = finite(raw.min);
  const step = finite(raw.step);
  const decimals = finite(raw.decimals);
  if (scale === null || scale <= 0 || min === null || step === null || step <= 0) return null;
  if (decimals === null || !Number.isInteger(decimals) || decimals < 0) return null;
  if (decimals > MAX_DECIMALS) return null;
  return { unit: text(raw.unit) ?? '', scale, min, max: finite(raw.max), step, decimals };
}

type BrowserRow = Extract<ParsedOption, { kind: 'browser' }>['row'];
const KNOWN_ROWS = new Set<string>(BROWSER_ROWS);
const isBrowserRow = (row: string): row is BrowserRow => KNOWN_ROWS.has(row);

/** Reads the fields of one kind of option; null when they do not fit it. */
type KindReader = (common: Common, raw: Raw) => ParsedOption | null;

const READERS: Readonly<Record<string, KindReader>> = {
  [OPTION_KINDS.toggle]: (common, { value }) =>
    typeof value === 'boolean' ? { ...common, kind: 'toggle', value } : null,
  [OPTION_KINDS.choice]: (common, raw) => {
    const choices = choicesOf(raw.choices);
    return choices !== null && isOptionValue(raw.value)
      ? { ...common, kind: 'choice', value: raw.value, choices }
      : null;
  },
  [OPTION_KINDS.choices]: (common, raw) => {
    const choices = choicesOf(raw.choices);
    const { value } = raw;
    if (choices === null || !Array.isArray(value) || !value.every(isOptionValue)) return null;
    return {
      ...common,
      kind: 'choices',
      value,
      choices,
      placeholder: text(raw.placeholder),
      searchPlaceholder: text(raw.search_placeholder),
    };
  },
  [OPTION_KINDS.number]: (common, raw) => {
    const field = numberField(raw);
    const value = finite(raw.value);
    return field !== null && value !== null ? { ...common, kind: 'number', value, field } : null;
  },
  [OPTION_KINDS.limit]: (common, raw) => {
    const field = numberField(raw);
    const onValue = finite(raw.on_value);
    const value = raw.value === null ? null : finite(raw.value);
    if (field === null || onValue === null || (raw.value !== null && value === null)) return null;
    return { ...common, kind: 'limit', value, onValue, field };
  },
  [OPTION_KINDS.action]: (common) => ({ ...common, kind: 'action' }),
  [OPTION_KINDS.note]: (common) => ({ ...common, kind: 'note' }),
};

/** One option, or null when it cannot be read at all. */
export function parseOption(raw: unknown): ParsedOption | null {
  if (!isRaw(raw)) return null;
  const id = text(raw.id);
  const kind = text(raw.kind);
  if (id === null || id === '' || kind === null) return null;
  if (kind === OPTION_KINDS.browser) {
    const row = text(raw.browser);
    return row !== null && isBrowserRow(row) ? { kind: 'browser', id, row } : null;
  }
  const label = text(raw.label);
  if (label === null) return null;
  // An own key only: an inherited one (`constructor`, `toString`) is no reader.
  const read = Object.hasOwn(READERS, kind) ? READERS[kind] : undefined;
  if (read === undefined) return { kind: 'unknown', id, label };
  return read({ id, label, help: text(raw.help), disabled: text(raw.disabled) }, raw);
}

/** The groups the popup shows, each with the options this version can show. */
export function parseOptions(list: OptionList): ParsedGroup[] {
  return list.groups.slice(0, MAX_OPTION_GROUPS).map((group) => ({
    id: group.id,
    title: group.title === null ? null : text(group.title),
    summary: group.summary === null ? null : text(group.summary),
    folded: group.folded,
    options: group.options
      .slice(0, MAX_OPTIONS_PER_GROUP)
      .map(parseOption)
      .filter((option): option is ParsedOption => option !== null),
  }));
}

/** A value a menu holds: text or a number. */
export type MenuValue = string | number;

/** The choices a menu can hold, as the design system's menus take them. */
export function menuOf(
  choices: readonly OptionChoice[],
): { value: MenuValue; label: string; keywords?: string }[] {
  return choices.flatMap((c) =>
    typeof c.value === 'boolean'
      ? []
      : [
          c.keywords === undefined
            ? { value: c.value, label: c.label }
            : { value: c.value, label: c.label, keywords: c.keywords },
        ],
  );
}

/** `value`, as a field in `field`'s unit shows it. */
export function shownNumber(value: number, field: NumberField): number {
  const factor = DECIMAL_BASE ** field.decimals;
  return Math.round((value / field.scale) * factor) / factor;
}

/** What a number typed in `field`'s unit stands for: a whole number, as the app keeps it. */
export function storedNumber(shown: number, field: NumberField): number {
  return Math.round(shown * field.scale);
}
