<script lang="ts">
  // The app's text button: the design system's button recipe (one look for every button), a type that never submits a form by accident, and an optional leading icon at the button icon size.
  import type { LucideIcon } from '@lucide/svelte';
  import type { HTMLButtonAttributes } from 'svelte/elements';
  import { button, type ButtonVariant } from '@wirecatch/ui';
  import Icon from './Icon.svelte';

  const VARIANT: Record<'default' | 'primary' | 'danger' | 'ghost', ButtonVariant> = {
    default: 'secondary',
    primary: 'primary',
    danger: 'danger',
    ghost: 'ghost',
  };

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
  class={button({
    variant: VARIANT[variant],
    size: small ? 'sm' : 'md',
    class: typeof extra === 'string' ? extra : undefined,
  })}
  {...rest}
>
  {#if icon}<Icon {icon} size="sm" />{/if}
  {@render children?.()}
</button>
