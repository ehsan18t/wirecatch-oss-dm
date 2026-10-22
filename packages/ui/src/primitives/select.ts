import type { IconComponent } from './icon';

export interface SelectOption<T extends string | number = string> {
  value: T;
  label: string;
  /** A second line under the label. */
  description?: string | undefined;
  icon?: IconComponent | undefined;
  disabled?: boolean | undefined;
  /** Other words a search finds it by, never shown: an older name, a code. */
  keywords?: string | undefined;
}

/** The options a search for `query` finds, by their label, description and keywords; all of them for an empty query. */
export function searched<T extends string | number>(
  options: readonly SelectOption<T>[],
  query: string,
): readonly SelectOption<T>[] {
  const words = query.trim().toLowerCase();
  if (!words) return options;
  return options.filter((option) =>
    [option.label, option.description, option.keywords].join(' ').toLowerCase().includes(words),
  );
}
