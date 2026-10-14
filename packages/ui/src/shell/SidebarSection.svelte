<script lang="ts">
  // A group of sidebar items under a heading; in the rail the heading gives way to a line. A collapsible group's heading is a button that folds its items away, so a long navigation fits a short window; in the rail that button stands in for the heading, under the line.
  import ChevronDown from '~icons/lucide/chevron-down';
  import ChevronRight from '~icons/lucide/chevron-right';
  import type { Snippet } from 'svelte';
  import { tooltip } from '../attachments/tooltip.svelte';
  import { cn } from '../lib/cn';
  import Icon from '../primitives/Icon.svelte';
  import { FOCUS_RING_INSET } from '../recipes/control';
  import { getSidebarContext } from './sidebar-state.svelte';

  interface Props {
    title: string;
    /** An action beside the heading, such as "New queue". */
    action?: Snippet;
    /** The heading folds the items away and shows them again. */
    collapsible?: boolean;
    /** Whether the items show, for a collapsible section. */
    expanded?: boolean;
    /** The person folded or opened the section; the owner may keep the choice. */
    ontoggle?: (expanded: boolean) => void;
    children: Snippet;
  }

  let {
    title,
    action,
    collapsible = false,
    expanded = $bindable(true),
    ontoggle,
    children,
  }: Props = $props();

  const sidebar = getSidebarContext();
  const id = $props.id();
  const open = $derived(!collapsible || expanded);

  function toggle() {
    expanded = !expanded;
    ontoggle?.(expanded);
  }
</script>

<div
  role="group"
  aria-labelledby={id}
  class={cn('flex flex-col gap-0.5 first:mt-0', sidebar.showLabels ? 'mt-3' : 'mt-2')}
>
  {#if sidebar.showLabels}
    <div class="flex h-control-sm items-center justify-between pr-1 pl-3">
      {#if collapsible}
        <button
          type="button"
          {id}
          data-nav-stop
          aria-expanded={expanded}
          aria-controls="{id}-items"
          class={cn(
            '-ml-1.5 flex items-center gap-1 rounded-xs pr-1 pl-1.5 text-caption font-semibold tracking-caps text-ink-subtle uppercase hover:text-ink',
            FOCUS_RING_INSET,
          )}
          onclick={toggle}
          >{title}<Icon icon={expanded ? ChevronDown : ChevronRight} size="xs" /></button
        >
      {:else}
        <span {id} class="text-caption font-semibold tracking-caps text-ink-subtle uppercase"
          >{title}</span
        >
      {/if}
      {#if action}
        <span data-nav-section-action class="flex">{@render action()}</span>
      {/if}
    </div>
  {:else}
    <span id={collapsible ? undefined : id} class="sr-only">{title}</span>
    <div class="mx-2 mb-1 h-px bg-line" aria-hidden="true"></div>
    {#if collapsible}
      <button
        type="button"
        {id}
        data-nav-stop
        aria-label={title}
        aria-expanded={expanded}
        aria-controls="{id}-items"
        class={cn(
          'flex h-control-xs w-full shrink-0 items-center justify-center rounded-sm text-ink-subtle hover:bg-wash hover:text-ink',
          FOCUS_RING_INSET,
        )}
        {@attach tooltip(() => `${expanded ? 'Hide' : 'Show'} ${title.toLocaleLowerCase()}`)}
        onclick={toggle}
      >
        <Icon icon={expanded ? ChevronDown : ChevronRight} size="sm" />
      </button>
    {/if}
  {/if}
  {#if open}
    <div id="{id}-items" class="flex flex-col gap-0.5">{@render children()}</div>
  {/if}
</div>
