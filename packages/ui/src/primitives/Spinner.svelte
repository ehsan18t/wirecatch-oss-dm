<script lang="ts">
  // A ring that turns, drawn like Windows 11's progress ring. Decorative unless it has a label or text beside it.
  import type { Snippet } from 'svelte';
  import { cn } from '../lib/cn';
  import { ICON_SIZES, type IconSize } from './icon';

  interface Props {
    size?: IconSize;
    /** `current` takes the surrounding text colour, as inside a button. */
    tone?: 'accent' | 'current';
    /** Names the work for screen readers; with neither this nor `children` the spinner is decoration. */
    label?: string | undefined;
    class?: string | undefined;
    /** Visible status text beside the ring. */
    children?: Snippet;
  }

  let { size = 'md', tone = 'accent', label, class: className, children }: Props = $props();

  const isStatus = $derived(label !== undefined || children !== undefined);
</script>

<span
  role={isStatus ? 'status' : undefined}
  aria-label={label}
  aria-hidden={isStatus ? undefined : 'true'}
  class={cn('inline-flex items-center gap-2', className)}
>
  <svg
    viewBox="0 0 16 16"
    fill="none"
    class={cn(
      'animate-spin',
      ICON_SIZES[size],
      tone === 'accent' ? 'text-accent' : 'text-current',
      'forced-colors:text-sys-text',
    )}
    aria-hidden="true"
  >
    <circle cx="8" cy="8" r="6.5" stroke="currentColor" stroke-opacity="0.2" stroke-width="2" />
    <path
      d="M8 1.5a6.5 6.5 0 0 1 6.5 6.5"
      stroke="currentColor"
      stroke-width="2"
      stroke-linecap="round"
    />
  </svg>
  {@render children?.()}
</span>
