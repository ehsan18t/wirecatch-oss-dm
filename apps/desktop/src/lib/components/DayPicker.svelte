<script lang="ts">
  // The days a schedule runs on: one joined row of seven toggle buttons, Monday first (D84.6).
  import type { Weekday } from '@wirecatch/protocol';
  import { cn, FOCUS_RING_INSET } from '@wirecatch/ui';
  import { WEEKDAY_SHORT } from '$lib/format';
  import { WEEKDAYS } from '$lib/time';

  let {
    days,
    invalid = false,
    describedby,
    onchange,
  }: {
    days: readonly Weekday[];
    invalid?: boolean;
    describedby?: string | undefined;
    onchange: (day: Weekday) => void;
  } = $props();
</script>

<div
  role="group"
  aria-label="Days"
  aria-describedby={describedby}
  class={cn(
    'grid w-full max-w-md grid-cols-7 gap-0.5 rounded-sm border bg-wash p-0.5',
    invalid ? 'border-danger' : 'border-line',
  )}
>
  {#each WEEKDAYS as day (day)}
    {const pressed = $derived(days.includes(day))}
    <button
      type="button"
      aria-pressed={pressed}
      class={cn(
        'h-control-xs rounded-xs text-small font-medium select-none',
        FOCUS_RING_INSET,
        pressed
          ? 'bg-accent-soft font-semibold text-accent forced-colors:bg-sys-highlight forced-colors:text-sys-highlight-text'
          : 'text-ink-muted hover:bg-wash hover:text-ink',
      )}
      onclick={() => onchange(day)}>{WEEKDAY_SHORT[day]}</button
    >
  {/each}
</div>
