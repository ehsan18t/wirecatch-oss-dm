<script lang="ts" generics="T extends string | number">
  // Tabs (WAI-ARIA APG, automatic activation): one tab stop, arrow keys move and show, Home and End jump. A disabled tab stays focusable (aria-disabled), as the APG advises, so screen reader users learn it is there; it is never shown. The underline glides to the chosen tab.
  import type { Snippet } from 'svelte';
  import { cn } from '../lib/cn';
  import { findElements } from '../lib/dom';
  import { listKeyIndex, nextEnabledIndex } from '../lib/list-nav';
  import { glide } from '../motion/motion';
  import { CHROME, DISABLED, FOCUS_RING, FOCUS_RING_INSET } from '../recipes/control';
  import Icon from './Icon.svelte';
  import type { SelectOption } from './select';

  interface Props {
    value: T;
    tabs: readonly (SelectOption<T> & { count?: number | undefined })[];
    /** The tab list's name for screen readers. */
    label: string;
    class?: string | undefined;
    onchange?: (value: T) => void;
    /** The chosen tab's content, shown in its tab panel. */
    panel?: Snippet<[T]>;
  }

  let { value, tabs, label, class: className, onchange, panel }: Props = $props();

  const uid = $props.id();
  const tabId = (index: number) => `${uid}-tab-${String(index)}`;
  const panelId = `${uid}-panel`;
  let list = $state<HTMLElement | null>(null);
  const selectedIndex = $derived(tabs.findIndex((tab) => tab.value === value));
  // With no tab chosen (a value that matches none), the list still needs one tab stop.
  const tabStop = $derived(selectedIndex >= 0 ? selectedIndex : nextEnabledIndex(tabs, -1, 1));
  // Arrow keys land on every tab, disabled ones included; choosing skips those.
  const stops = $derived(tabs.map(() => ({ disabled: false })));

  const tabElements = () => findElements(list, '[role="tab"]');

  let previous = -1;
  $effect(() => {
    const from = previous;
    previous = selectedIndex;
    if (from < 0 || selectedIndex < 0 || from === selectedIndex) return;
    const all = tabElements();
    const start = all[from];
    const end = all[selectedIndex];
    if (start && end) glide(start, end, { pseudoElement: '::after' });
  });

  function choose(index: number) {
    const tab = tabs[index];
    if (tab && tab.disabled !== true && tab.value !== value) onchange?.(tab.value);
  }

  function onkeydown(event: KeyboardEvent) {
    const all = tabElements();
    const focused = all.findIndex((tab) => tab === document.activeElement);
    const next = listKeyIndex(stops, focused, event.key, 'horizontal');
    if (next === null) return;
    event.preventDefault();
    if (next < 0) return;
    all[next]?.focus();
    choose(next);
  }
</script>

<div class={cn('flex min-w-0 flex-col', className)}>
  <div
    bind:this={list}
    role="tablist"
    aria-label={label}
    tabindex="-1"
    class={cn('flex items-end gap-1 border-b border-line', CHROME)}
    {onkeydown}
  >
    {#each tabs as tab, index (tab.value)}
      {@const isSelected = index === selectedIndex}
      <button
        type="button"
        role="tab"
        id={tabId(index)}
        aria-selected={isSelected}
        aria-controls={panel ? panelId : undefined}
        tabindex={index === tabStop ? 0 : -1}
        aria-disabled={tab.disabled === true || undefined}
        class={cn(
          'relative -mb-px inline-flex h-control items-center gap-2 rounded-t-sm px-3 text-ui font-medium',
          FOCUS_RING_INSET,
          DISABLED,
          isSelected
            ? 'text-ink after:absolute after:inset-x-2 after:bottom-0 after:h-0.75 after:origin-left after:rounded-full after:bg-accent forced-colors:after:bg-sys-highlight'
            : 'text-ink-muted enabled:hover:bg-wash enabled:hover:text-ink',
        )}
        onclick={() => choose(index)}
      >
        {#if tab.icon}<Icon icon={tab.icon} size="sm" />{/if}
        {tab.label}
        {#if tab.count !== undefined}
          <span class="rounded-full bg-wash-strong px-1.5 text-caption tabular-nums text-ink-muted">
            {tab.count}
          </span>
        {/if}
      </button>
    {/each}
  </div>
  {#if panel && selectedIndex >= 0}
    <div
      id={panelId}
      role="tabpanel"
      aria-labelledby={tabId(selectedIndex)}
      tabindex="0"
      class={cn('min-h-0 flex-1 animate-fade-in pt-4', FOCUS_RING)}
    >
      {@render panel(value)}
    </div>
  {/if}
</div>
