<script lang="ts">
  // The app's text button: the `.btn` styles in app.css, a type that never submits a form by
  // accident, and an optional leading icon at the button icon size.
  import type { LucideIcon } from '@lucide/svelte';
  import type { HTMLButtonAttributes } from 'svelte/elements';
  import Icon from './Icon.svelte';

  let {
    variant = 'default',
    small = false,
    icon,
    type = 'button',
    element = $bindable(),
    class: extra,
    children,
    ...rest
  }: Omit<HTMLButtonAttributes, 'type'> & {
    variant?: 'default' | 'primary' | 'danger' | 'ghost';
    small?: boolean;
    icon?: LucideIcon;
    /** `submit` only for the button that sends its form. */
    type?: 'button' | 'submit';
    element?: HTMLButtonElement;
  } = $props();
</script>

<button
  bind:this={element}
  {type}
  class={['btn', variant !== 'default' && variant, small && 'small', extra]}
  {...rest}
>
  {#if icon}<Icon {icon} size="sm" />{/if}
  {@render children?.()}
</button>
