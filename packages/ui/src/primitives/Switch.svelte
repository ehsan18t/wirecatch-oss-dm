<script lang="ts">
  // An on/off switch (role switch) on a native button, whose own click answers Enter and Space; acting on keydown as well would toggle twice wherever the browser still clicks after it (Firefox, for the extension).
  import type { Snippet } from 'svelte';
  import { cn } from '../lib/cn';
  import { CHROME, DISABLED, FOCUS_RING } from '../recipes/control';
  import Spinner from './Spinner.svelte';

  interface Props {
    checked: boolean;
    /** The switch's name: shown beside it, or only to screen readers with `hideLabel`. */
    label: string;
    hideLabel?: boolean;
    /** A second line under the label. */
    description?: string | undefined;
    /** The id of help shown elsewhere (a settings row's), in place of `description`. */
    describedby?: string | undefined;
    /** Waiting for the change to land: the thumb shows a spinner and the switch takes no input. */
    loading?: boolean;
    disabled?: boolean;
    /** The label sits after the switch (`end`) or before it, stretched to the row's width (`start`), as in a settings row. */
    labelSide?: 'start' | 'end';
    id?: string | undefined;
    class?: string | undefined;
    children?: Snippet;
    onchange?: (checked: boolean) => void;
  }

  let {
    checked,
    label,
    hideLabel = false,
    description,
    describedby,
    loading = false,
    disabled = false,
    labelSide = 'end',
    id,
    class: className,
    children,
    onchange,
  }: Props = $props();

  const uid = $props.id();
  const labelId = `${uid}-label`;
  const hintId = `${uid}-hint`;

  function toggle() {
    if (!disabled && !loading) onchange?.(!checked);
  }
</script>

<div
  class={cn(
    'inline-flex items-center gap-3',
    labelSide === 'start' && 'w-full flex-row-reverse justify-between',
    className,
  )}
>
  <button
    {id}
    type="button"
    role="switch"
    aria-checked={checked}
    aria-labelledby={labelId}
    aria-describedby={describedby ?? (description ? hintId : undefined)}
    aria-busy={loading || undefined}
    {disabled}
    class={cn(
      'group relative inline-flex h-5 w-10 shrink-0 items-center rounded-full border p-0.5 transition',
      checked
        ? 'border-accent bg-accent enabled:hover:border-accent-hover enabled:hover:bg-accent-hover'
        : 'border-line-strong bg-field enabled:hover:bg-wash-strong',
      'forced-colors:forced-color-adjust-none forced-colors:border-sys-text',
      checked ? 'forced-colors:bg-sys-highlight' : 'forced-colors:bg-sys-canvas',
      FOCUS_RING,
      DISABLED,
      CHROME,
    )}
    onclick={toggle}
  >
    <!-- The thumb grows a little under the pointer and stretches while pressed, as Windows 11's does. -->
    <span
      class={cn(
        'flex size-3.5 items-center justify-center rounded-full shadow-thumb transition-all duration-normal ease-spring',
        'group-enabled:group-hover:size-4 group-enabled:group-active:w-5',
        checked
          ? 'translate-x-5 bg-accent-fg group-enabled:group-active:translate-x-3.5 forced-colors:bg-sys-highlight-text'
          : 'translate-x-0 bg-ink-muted forced-colors:bg-sys-text',
      )}
    >
      {#if loading}<Spinner size="xs" tone="current" class="text-accent" />{/if}
    </span>
  </button>
  <span class={cn('flex min-w-0 flex-col', hideLabel && 'sr-only')}>
    <span id={labelId} class="text-ui text-ink">{label}</span>
    {@render children?.()}
    {#if description}
      <span id={hintId} class="text-small text-ink-muted">{description}</span>
    {/if}
  </span>
</div>
