// Committing what a person typed in a number field that edits a setting.

/** How one number field reads, shows and saves its setting. */
export interface NumberSetting<T> {
  /** The value to keep for what was typed, or `null` to keep `current`. */
  read: (typed: string) => T | null;
  /** The setting as it is now. */
  current: T;
  /** `value` as the field shows it. */
  show: (value: T) => string;
  save: (value: T) => void;
}

/**
 * Saves what `field` reads as, when it changes the setting, and shows the kept value in the field either way. Svelte writes a field's value only when the setting changes, so text that is refused, clamped or rounded to the value already saved would otherwise stay in the field.
 */
export function commitNumber<T>(field: HTMLInputElement, setting: NumberSetting<T>): void {
  const kept = setting.read(field.value) ?? setting.current;
  field.value = setting.show(kept);
  if (kept !== setting.current) setting.save(kept);
}
