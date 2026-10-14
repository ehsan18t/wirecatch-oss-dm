<script lang="ts">
  // One place in the navigation. In the rail its label shows as a tooltip and stays its name for screen readers. With labels showing, it may carry a second line (a queue's schedule and limit) and buttons of its own beside it (edit, pause), which show while the pointer or the keyboard is on the item.
  import type { Snippet } from 'svelte';
  import { tooltip } from '../attachments/tooltip.svelte';
  import { cn } from '../lib/cn';
  import Icon from '../primitives/Icon.svelte';
  import type { IconComponent } from '../primitives/icon';
  import { FOCUS_RING_INSET, TONE_FILL, TONE_TEXT, type Tone } from '../recipes/control';
  import { getSidebarContext } from './sidebar-state.svelte';

  interface Props {
    label: string;
    icon: IconComponent;
    /** This is the page being shown. */
    active?: boolean;
    /** A number after the label: how many downloads are in this view. */
    count?: number | undefined;
    /** The count's colour, such as `danger` for failures. */
    countTone?: Tone;
    /** A second line under the label, with labels showing: what this place is doing. It is part of the item's name. */
    description?: string | undefined;
    /** Something else after the label, in place of a count. */
    trailing?: Snippet;
    /** Buttons beside the item (never inside it, so each keeps its own name), with labels showing. */
    actions?: Snippet;
    onclick?: () => void;
  }

  let {
    label,
    icon,
    active = false,
    count,
    countTone = 'neutral',
    description,
    trailing,
    actions,
    onclick,
  }: Props = $props();

  const sidebar = getSidebarContext();
  let item = $state<HTMLElement | null>(null);
  const described = $derived(description !== undefined && sidebar.showLabels);
  const withActions = $derived(actions !== undefined && sidebar.showLabels);

  $effect(() => {
    if (active && item) sidebar.activated(item);
  });
</script>

<div class="group/item relative shrink-0" data-nav-entry>
  <button
    bind:this={item}
    type="button"
    data-nav-stop
    aria-current={active ? 'page' : undefined}
    class={cn(
      'group relative flex w-full shrink-0 items-center gap-3 rounded-sm px-3 text-ui',
      // The rail is denser still, so a 600px window shows every place without scrolling.
      sidebar.showLabels ? (described ? 'min-h-nav-item py-1' : 'h-nav-item') : 'h-control',
      FOCUS_RING_INSET,
      active
        ? 'bg-wash-strong font-medium text-ink before:absolute before:top-1/2 before:left-0 before:h-4 before:w-0.75 before:-translate-y-1/2 before:rounded-full before:bg-accent forced-colors:before:bg-sys-highlight'
        : 'text-ink-muted hover:bg-wash hover:text-ink active:bg-wash-strong',
    )}
    {@attach tooltip(() => (sidebar.showLabels ? null : label))}
    onclick={() => {
      onclick?.();
      sidebar.chose();
    }}
  >
    <Icon {icon} size="lg" class={cn(active && 'text-accent')} />
    <!-- In the rail the count shows as a dot at most, so screen readers get it with the name ("Failed, 3"). -->
    <span class={cn('flex min-w-0 flex-1 flex-col text-left', !sidebar.showLabels && 'sr-only')}>
      <span class="truncate"
        >{label}{#if !sidebar.showLabels && count !== undefined && count > 0}, {count}{/if}</span
      >
      {#if described}
        <span class="truncate text-small font-normal text-ink-subtle">{description}</span>
      {/if}
    </span>
    {#if sidebar.showLabels}
      <span
        class={cn(
          'flex shrink-0 items-center',
          withActions && 'group-hover/item:opacity-0 group-focus-within/item:opacity-0',
        )}
      >
        {#if trailing}
          {@render trailing()}
        {:else if count !== undefined && count > 0}
          <span class={cn('text-small tabular-nums', TONE_TEXT[countTone])}>{count}</span>
        {/if}
      </span>
    {:else if count !== undefined && count > 0 && countTone !== 'neutral'}
      <!-- In the rail only a count that needs attention shows, as a dot. -->
      <span
        class={cn('absolute top-2 right-2 size-1.5 rounded-full', TONE_FILL[countTone])}
        aria-hidden="true"
      ></span>
    {/if}
  </button>
  {#if withActions && actions}
    <!-- Out of the tab order: the right arrow on the item reaches them (nav-keys.ts). -->
    <div
      data-nav-actions
      class="absolute top-1/2 right-1.5 flex -translate-y-1/2 items-center gap-0.5 opacity-0 group-focus-within/item:opacity-100 group-hover/item:opacity-100"
    >
      {@render actions()}
    </div>
  {/if}
</div>
