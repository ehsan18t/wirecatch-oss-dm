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
    /** A second line under the label; a snippet where part of it is marked, such as a name in bold. */
    description?: string | Snippet | undefined;
    /** The id of help shown elsewhere (a settings row's), in place of `description`. */
    describedby?: string | undefined;
    /** Waiting for the change to land: the thumb shows a spinner and the switch takes no input. */
    loading?: boolean;
    disabled?: boolean;
    /** The label sits after the switch (`end`), or, as in a settings row (`start`), on a line of the row's width with the switch at its end and the description under both, the label in medium weight. */
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
    labelSide === 'start'
      ? 'flex w-full flex-wrap items-center gap-x-3 gap-y-0.5'
      : 'inline-flex items-center gap-3',
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
      labelSide === 'start' && 'order-1',
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
  <!-- In a settings row its parts sit in the row itself: the label on the switch's line, taking the room the switch leaves, and the rest under both. -->
  <span
    class={cn(
      'flex min-w-0 flex-col',
      labelSide === 'start' && 'contents',
      hideLabel && 'sr-only flex',
    )}
  >
    <span
      id={labelId}
      class={cn(
        'text-ui wrap-break-word text-ink',
        labelSide === 'start' && 'min-w-0 flex-1 font-medium',
        labelSide === 'start' && disabled && 'text-ink-muted',
      )}>{label}</span
    >
    {@render children?.()}
    {#if description}
      <span
        id={hintId}
        class={cn(
          'text-small wrap-break-word text-ink-muted',
          labelSide === 'start' && 'order-2 basis-full',
        )}
      >
        {#if typeof description === 'string'}{description}{:else}{@render description()}{/if}
      </span>
    {/if}
  </span>
</div>
