<script lang="ts">
  // One choice of several that can each be on or off, as a chip that stays pressed: the days a queue runs, the subtitles to save. A row of them wraps; it never scrolls inside the page.
  import Check from '~icons/lucide/check';
  import { cn } from '../lib/cn';
  import Icon from './Icon.svelte';
  import { FOCUS_RING } from '../recipes/control';

  interface Props {
    pressed: boolean;
    label: string;
    disabled?: boolean;
    class?: string | undefined;
    onchange: (pressed: boolean) => void;
  }

  let { pressed, label, disabled = false, class: className, onchange }: Props = $props();
</script>

<button
  type="button"
  aria-pressed={pressed}
  {disabled}
  class={cn(
    'inline-flex h-control-sm min-w-11 items-center justify-center gap-1 rounded-sm border px-2.5 text-ui transition-colors duration-fast ease-out disabled:opacity-disabled',
    pressed
      ? 'border-accent bg-accent-soft font-medium text-accent'
      : 'border-line-strong/quiet bg-transparent text-ink-muted enabled:hover:bg-wash enabled:hover:text-ink',
    FOCUS_RING,
    className,
  )}
  onclick={() => onchange(!pressed)}
  >{#if pressed}<Icon icon={Check} size="xs" />{/if}{label}</button
>
