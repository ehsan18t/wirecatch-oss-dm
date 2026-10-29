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

export { default as VisuallyHidden } from './VisuallyHidden.svelte';
export { visuallyHidden, type VisuallyHiddenVariants } from './visually-hidden';

export { default as Badge } from './primitives/Badge.svelte';
export { default as Button } from './primitives/Button.svelte';
export { default as Callout } from './primitives/Callout.svelte';
export { default as Card } from './primitives/Card.svelte';
export { default as Checkbox } from './primitives/Checkbox.svelte';
export { default as Disclosure } from './primitives/Disclosure.svelte';
export { default as EmptyState } from './primitives/EmptyState.svelte';
export { default as Field } from './primitives/Field.svelte';
export type { FieldControl } from './primitives/field-control';
export { default as Icon } from './primitives/Icon.svelte';
export { default as IconButton } from './primitives/IconButton.svelte';
export { default as InfoTip } from './primitives/InfoTip.svelte';
export { default as Input } from './primitives/Input.svelte';
export { default as Kbd } from './primitives/Kbd.svelte';
export { default as MultiSelect } from './primitives/MultiSelect.svelte';
export { default as NumberInput } from './primitives/NumberInput.svelte';
export { default as ProgressBar } from './primitives/ProgressBar.svelte';
export type { ProgressState } from './primitives/progress';
export { default as RadioGroup } from './primitives/RadioGroup.svelte';
export { default as SegmentedControl } from './primitives/SegmentedControl.svelte';
export { default as Select } from './primitives/Select.svelte';
export { default as Separator } from './primitives/Separator.svelte';
export { default as Skeleton } from './primitives/Skeleton.svelte';
export { default as Slider } from './primitives/Slider.svelte';
export { default as Spinner } from './primitives/Spinner.svelte';
export { default as Switch } from './primitives/Switch.svelte';
export { default as TagInput } from './primitives/TagInput.svelte';
export { default as ToggleChip } from './primitives/ToggleChip.svelte';
export { default as Tabs } from './primitives/Tabs.svelte';
export { default as TextArea } from './primitives/TextArea.svelte';
export { ICON_SIZES, type IconComponent, type IconSize } from './primitives/icon';
export type { SelectOption } from './primitives/select';

export { default as SettingRow } from './settings/SettingRow.svelte';
export { default as SettingsSection } from './settings/SettingsSection.svelte';
export { default as SettingSwitch } from './settings/SettingSwitch.svelte';
export { matchesSearch, searchWords, type Searchable } from './settings/search';

export { default as AppMark } from './shell/AppMark.svelte';
export { default as ContextMenu } from './shell/ContextMenu.svelte';
export { default as Dialog } from './shell/Dialog.svelte';
export type { DialogSize } from './shell/dialog';
export { default as Menu } from './shell/Menu.svelte';
export { default as Popover } from './shell/Popover.svelte';
export type { PopoverTriggerProps } from './shell/popover';
export { default as Sidebar } from './shell/Sidebar.svelte';
export { default as SidebarItem } from './shell/SidebarItem.svelte';
export { default as SidebarSection } from './shell/SidebarSection.svelte';
export { default as SlideOver } from './shell/SlideOver.svelte';
export { default as TitleBar } from './shell/TitleBar.svelte';
export { default as ToastRegion } from './shell/ToastRegion.svelte';
export type {
  MenuAction,
  MenuCheckbox,
  MenuCloseReason,
  MenuEntry,
  MenuSeparator,
  MenuTriggerProps,
} from './shell/menu';
export { SidebarState, type SidebarMode, type SidebarOptions } from './shell/sidebar-state.svelte';
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
export type {
  ButtonRect,
  CaptionAction,
  MaximizeState,
  WindowControls,
} from './shell/window-controls';
