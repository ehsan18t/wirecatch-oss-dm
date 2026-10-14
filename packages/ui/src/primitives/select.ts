import type { IconComponent } from './icon';

export interface SelectOption<T extends string | number = string> {
  value: T;
  label: string;
  /** A second line under the label. */
  description?: string | undefined;
  icon?: IconComponent | undefined;
  disabled?: boolean | undefined;
}
