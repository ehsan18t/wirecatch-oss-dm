// Wirecatch's design system. Styles: import `@wirecatch/ui/tokens.css` (and, on a redesigned page, `@wirecatch/ui/base.css`) once per page.

export * from './appearance';

export { cn, tv, type ClassValue, type VariantProps } from './lib/cn';
export { token } from './lib/css-token';
export { keyGlyph, shortcutText } from './lib/keys';
export {
  textIfCut,
  tooltip,
  type TooltipOptions,
  type TooltipText,
} from './attachments/tooltip.svelte';
export {
  delay,
  duration,
  easing,
  expand,
  fade,
  glide,
  pop,
  reducedMotion,
  reflow,
  rise,
  settle,
  shift,
  type Curve,
  type Delay,
  type Distance,
  type Side,
  type Speed,
} from './motion/motion';
export { button, type ButtonVariant } from './recipes/button';
export { FOCUS_RING, FOCUS_RING_INSET, TONES, type Tone } from './recipes/control';
export { DIALOG_SURFACE } from './recipes/surface';

export { default as Badge } from './primitives/Badge.svelte';
export { default as Button } from './primitives/Button.svelte';
export { default as Callout } from './primitives/Callout.svelte';
export { default as Card } from './primitives/Card.svelte';
export { default as Disclosure } from './primitives/Disclosure.svelte';
export type { FieldControl } from './primitives/field-control';
export { default as Icon } from './primitives/Icon.svelte';
export { default as IconButton } from './primitives/IconButton.svelte';
export { default as InfoTip } from './primitives/InfoTip.svelte';
export { default as Kbd } from './primitives/Kbd.svelte';
export { default as MultiSelect } from './primitives/MultiSelect.svelte';
export { default as NumberInput } from './primitives/NumberInput.svelte';
export { default as Select } from './primitives/Select.svelte';
export { default as Spinner } from './primitives/Spinner.svelte';
export { default as Switch } from './primitives/Switch.svelte';
export { ICON_SIZES, type IconComponent, type IconSize } from './primitives/icon';
export type { SelectOption } from './primitives/select';

export { default as SettingRow } from './settings/SettingRow.svelte';
export { default as SettingSwitch } from './settings/SettingSwitch.svelte';

export { default as AppMark } from './shell/AppMark.svelte';
export { default as ToastRegion } from './shell/ToastRegion.svelte';
export {
  MAX_TOASTS,
  provideToaster,
  TOAST_DURATION,
  Toaster,
  type Toast,
  type ToastAction,
  type ToastOptions,
  type ToastTone,
} from './shell/toaster.svelte';
