<script lang="ts">
  // The open menu itself (WAI-ARIA APG menu): focus moves onto the items, arrows, Home and End move it, typing a letter finds an item, Enter or Space chooses, Escape and Tab close. It renders in the top layer; Menu and ContextMenu place it.
  import Check from '~icons/lucide/check';
  import { onMount } from 'svelte';
  import { cn } from '../lib/cn';
  import { findElements } from '../lib/dom';
  import { topLayer } from '../lib/layer';
  import {
    isTypeaheadKey,
    listKeyIndex,
    nextEnabledIndex,
    Typeahead,
    typeaheadIndex,
  } from '../lib/list-nav';
  import { pop } from '../motion/motion';
  import Icon from '../primitives/Icon.svelte';
  import Kbd from '../primitives/Kbd.svelte';
  import { FLYOUT_SURFACE, TOP_LAYER } from '../recipes/surface';
  import {
    ariaKeyShortcut,
    isSeparator,
    navigable,
    type MenuCloseReason,
    type MenuEntry,
  } from './menu';

  interface Props {
    entries: readonly MenuEntry[];
    /** The menu's name for screen readers. */
    label: string;
    /** Where focus starts: the first item, or the last (opened with the up arrow). */
    start?: 'first' | 'last';
    id?: string | undefined;
    style?: string | undefined;
    origin?: 'top' | 'bottom';
    onclose: (reason: MenuCloseReason) => void;
    element?: HTMLElement | null;
  }

  let {
    entries,
    label,
    start = 'first',
    id,
    style,
    origin = 'top',
    onclose,
    element = $bindable(null),
  }: Props = $props();

  const items = $derived(navigable(entries));
  const typeahead = new Typeahead();

  const rows = () => findElements(element, '[data-menu-row]');

  function focusRow(index: number) {
    if (index >= 0) rows()[index]?.focus();
  }

  onMount(() => {
    focusRow(
      nextEnabledIndex(items, start === 'first' ? -1 : items.length, start === 'first' ? 1 : -1),
    );
  });

  function choose(entry: MenuEntry) {
    if (isSeparator(entry) || entry.disabled === true) return;
    onclose('select');
    entry.onselect();
  }

  function onkeydown(event: KeyboardEvent) {
    const current = rows().findIndex((row) => row === document.activeElement);
    if (event.key === 'Escape') {
      event.preventDefault();
      onclose('escape');
      return;
    }
    if (event.key === 'Tab') {
      // Not prevented: focus goes back to where the menu was opened, and the browser's Tab moves on from there.
      onclose('tab');
      return;
    }
    const next = listKeyIndex(items, current, event.key, 'vertical');
    if (next !== null) {
      event.preventDefault();
      focusRow(next);
    } else if (isTypeaheadKey(event)) {
      focusRow(typeaheadIndex(items, current, typeahead.push(event.key)));
    }
  }
</script>

<div
  bind:this={element}
  {id}
  role="menu"
  aria-label={label}
  popover="manual"
  tabindex="-1"
  data-layer
  class={cn(
    TOP_LAYER,
    FLYOUT_SURFACE,
    'z-popover flex min-w-menu flex-col p-1 outline-none',
    origin === 'top' ? 'origin-top' : 'origin-bottom',
  )}
  {style}
  {@attach topLayer}
  transition:pop
  {onkeydown}
>
  {#each entries as entry (entry.id)}
    {#if isSeparator(entry)}
      <!-- A row too, so rows line up with entries; never focused, as the keys skip it. -->
      <div role="separator" data-menu-row class="mx-2 my-1 h-px bg-line"></div>
    {:else}
      {@const isCheckbox = entry.kind === 'checkbox'}
      {@const danger = entry.kind !== 'checkbox' && entry.danger === true}
      <div
        role={isCheckbox ? 'menuitemcheckbox' : 'menuitem'}
        aria-checked={entry.kind === 'checkbox' ? entry.checked : undefined}
        aria-disabled={entry.disabled === true || undefined}
        aria-keyshortcuts={entry.kind !== 'checkbox' && entry.shortcut
          ? ariaKeyShortcut(entry.shortcut)
          : undefined}
        tabindex="-1"
        data-menu-row
        class={cn(
          'flex h-control cursor-default items-center gap-2.5 rounded-sm px-2.5 text-ui outline-none',
          danger ? 'text-danger' : 'text-ink',
          entry.disabled === true
            ? 'opacity-disabled'
            : 'hover:bg-wash-strong focus:bg-wash-strong forced-colors:focus:bg-sys-highlight forced-colors:focus:text-sys-highlight-text',
        )}
        onclick={() => choose(entry)}
        onkeydown={(event) => {
          if (event.key === 'Enter' || event.key === ' ') {
            event.preventDefault();
            choose(entry);
          }
        }}
        onpointermove={(event) => {
          if (entry.disabled !== true) event.currentTarget.focus();
        }}
      >
        {#if entry.kind === 'checkbox'}
          <Icon icon={Check} size="sm" class={cn('text-accent', !entry.checked && 'invisible')} />
        {:else if entry.icon}
          <Icon icon={entry.icon} size="sm" class={danger ? undefined : 'text-ink-muted'} />
        {/if}
        <span class="min-w-0 flex-1 truncate">{entry.label}</span>
        {#if entry.kind !== 'checkbox' && entry.shortcut}
          <!-- Read through aria-keyshortcuts, not as part of the item's name. -->
          <span class="ml-4 flex" aria-hidden="true"><Kbd keys={entry.shortcut} /></span>
        {/if}
      </div>
    {/if}
  {/each}
</div>
