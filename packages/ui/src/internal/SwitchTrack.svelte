<script lang="ts">
  // The track and thumb of Switch, so Switch can place it before or after its label in reading order.
  import { cn } from '../lib/cn';
  import { CHROME, DISABLED, FOCUS_RING } from '../recipes/control';
  import Spinner from '../primitives/Spinner.svelte';

  interface Props {
    id: string | undefined;
    checked: boolean;
    loading: boolean;
    disabled: boolean;
    labelId: string;
    describedby: string | undefined;
    onclick: () => void;
  }

  let { id, checked, loading, disabled, labelId, describedby, onclick }: Props = $props();
</script>

<button
  {id}
  type="button"
  role="switch"
  aria-checked={checked}
  aria-labelledby={labelId}
  aria-describedby={describedby}
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
  {onclick}
>
  <!-- The thumb grows a little under the pointer and stretches while pressed, as Windows 11's does. -->
  <span
    class={cn(
      'flex size-3.5 items-center justify-center rounded-full shadow-thumb transition-all duration-normal ease-spring',
      'group-enabled:group-hover:size-4 group-enabled:group-active:w-5',
      checked
        ? 'translate-x-5 bg-accent-fg group-enabled:group-active:translate-x-3.5 forced-colors:bg-sys-highlight-text'
        : 'translate-x-0 bg-line-strong forced-colors:bg-sys-text',
    )}
  >
    {#if loading}<Spinner size="xs" tone="current" class="text-accent" />{/if}
  </span>
</button>
