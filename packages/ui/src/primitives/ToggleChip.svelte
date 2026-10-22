<script lang="ts">
  // One choice of several that can each be on or off, as a chip that stays pressed: the days a queue runs, the subtitles to save. A row of them wraps; it never scrolls inside the page.
  import { cn } from '../lib/cn';
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
    'h-control-sm min-w-11 rounded-sm border px-2.5 text-ui transition-colors duration-fast ease-out disabled:opacity-disabled',
    pressed
      ? 'border-accent bg-accent-soft font-medium text-accent'
      : 'border-line bg-control text-ink-muted enabled:hover:bg-control-hover enabled:hover:text-ink',
    FOCUS_RING,
    className,
  )}
  onclick={() => onchange(!pressed)}>{label}</button
>
