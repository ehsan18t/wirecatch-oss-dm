<script lang="ts">
  // A button that shows only an icon. Its label is required, because the icon alone says
  // nothing to a screen reader.
  import type { LucideIcon } from '@lucide/svelte';
  import type { HTMLButtonAttributes } from 'svelte/elements';
  import Icon from './Icon.svelte';
  import type { IconSize } from './scale';

  let {
    icon,
    label,
    size = 'sm',
    class: extra,
    ...rest
  }: Omit<HTMLButtonAttributes, 'type' | 'children' | 'aria-label'> & {
    icon: LucideIcon;
    label: string;
    size?: IconSize;
  } = $props();
</script>

<button type="button" class={['icon-btn', extra]} aria-label={label} {...rest}>
  <Icon {icon} {size} />
</button>
