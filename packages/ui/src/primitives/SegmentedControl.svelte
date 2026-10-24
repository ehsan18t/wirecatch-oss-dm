<script lang="ts" generics="T extends string | number">
  // A small set of exclusive choices side by side (a radiogroup): one tab stop, arrow keys choose, and the selection pill glides to the new segment.
  import { cn } from '../lib/cn';
  import { findElements } from '../lib/dom';
  import { listKeyIndex, nextEnabledIndex } from '../lib/list-nav';
  import { glide } from '../motion/motion';
  import { CHROME, FOCUS_RING_INSET } from '../recipes/control';
  import Icon from './Icon.svelte';
  import type { SelectOption } from './select';

  interface Props {
    value: T | null;
    options: readonly SelectOption<T>[];
    /** The group's name for screen readers. */
    label: string;
    size?: 'sm' | 'md';
    /** Segments share the width of the container. */
    block?: boolean;
    disabled?: boolean;
    class?: string | undefined;
    onchange?: (value: T) => void;
  }

  let {
    value,
    options,
    label,
    size = 'md',
    block = false,
    disabled = false,
    class: className,
    onchange,
  }: Props = $props();

  let group = $state<HTMLElement | null>(null);
  const selectedIndex = $derived(options.findIndex((option) => option.value === value));
  // With nothing selected the group still needs one tab stop.
  const tabStop = $derived(selectedIndex >= 0 ? selectedIndex : nextEnabledIndex(options, -1, 1));

  const segments = () => findElements(group, '[role="radio"]');

  // The pill is the selected segment's ::before; a change glides it over from the segment that had it.
  let previous = -1;
  $effect(() => {
    const from = previous;
    previous = selectedIndex;
    if (from < 0 || selectedIndex < 0 || from === selectedIndex) return;
    const all = segments();
    const start = all[from];
    const end = all[selectedIndex];
    if (start && end) glide(start, end);
  });

  function choose(index: number) {
    const option = options[index];
    if (!disabled && option && option.disabled !== true && option.value !== value) {
      onchange?.(option.value);
    }
  }

  function onkeydown(event: KeyboardEvent) {
    if (disabled) return;
    const all = segments();
    const focused = all.findIndex((segment) => segment === document.activeElement);
    const next = listKeyIndex(options, focused === -1 ? selectedIndex : focused, event.key);
    if (next === null) return;
    event.preventDefault();
    if (next < 0) return;
    all[next]?.focus();
    choose(next);
  }
</script>

<div
  bind:this={group}
  role="radiogroup"
  aria-label={label}
  aria-disabled={disabled || undefined}
  tabindex="-1"
  class={cn(
    'relative isolate inline-flex max-w-full items-center gap-0.5 rounded-sm border border-line bg-wash p-0.5',
    block && 'flex w-full',
    disabled && 'opacity-disabled',
    CHROME,
    className,
  )}
  {onkeydown}
>
  {#each options as option, index (option.value)}
    {const isSelected = $derived(index === selectedIndex)}
    <button
      type="button"
      role="radio"
      aria-checked={isSelected}
      tabindex={index === tabStop ? 0 : -1}
      disabled={disabled || option.disabled}
      class={cn(
        'relative inline-flex min-w-0 items-center justify-center gap-1.5 rounded-xs px-3 font-medium whitespace-nowrap',
        size === 'sm' ? 'h-control-xs text-small' : 'h-control-sm text-ui',
        block && 'flex-1',
        FOCUS_RING_INSET,
        'disabled:cursor-not-allowed',
        isSelected
          ? 'text-ink before:absolute before:inset-0 before:-z-1 before:origin-left before:rounded-xs before:border before:border-line before:bg-raised before:shadow-raised forced-colors:bg-sys-highlight forced-colors:text-sys-highlight-text forced-colors:before:hidden'
          : 'text-ink-muted enabled:hover:text-ink',
      )}
      onclick={() => choose(index)}
    >
      {#if option.icon}<Icon icon={option.icon} size="sm" />{/if}
      <span class="truncate">{option.label}</span>
    </button>
  {/each}
</div>
