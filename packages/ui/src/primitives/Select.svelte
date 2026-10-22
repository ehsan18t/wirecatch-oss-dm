<script lang="ts" module>
  import type { Placement } from '../lib/flyout';

  const PLACEMENT: Placement = { side: 'below', align: 'start', gap: 4, inset: 8 };
</script>

<script lang="ts" generics="T extends string | number">
  // A select-only combobox (WAI-ARIA APG): a button that opens a listbox, the highlight followed through aria-activedescendant so focus never leaves the button. `searchable` puts a search field over a long list, which then keeps the focus instead; `aside` ends each option with what sets it apart (badges, a size), the chosen one's on the field too.
  import Check from '~icons/lucide/check';
  import ChevronDown from '~icons/lucide/chevron-down';
  import Search from '~icons/lucide/search';
  import { tick, type Snippet } from 'svelte';
  import { Anchored, dismissOnOutside } from '../lib/anchored.svelte';
  import { cn } from '../lib/cn';
  import { topLayer } from '../lib/layer';
  import {
    isTypeaheadKey,
    listKeyIndex,
    nextEnabledIndex,
    Typeahead,
    typeaheadIndex,
  } from '../lib/list-nav';
  import { pop } from '../motion/motion';
  import { field, FIELD_INPUT, type FieldSize } from '../recipes/field';
  import { FLYOUT_SURFACE, TOP_LAYER } from '../recipes/surface';
  import Icon from './Icon.svelte';
  import type { SelectOption } from './select';

  interface Props {
    value: T | null;
    options: readonly SelectOption<T>[];
    /** The trigger's id, for a `<label for>` (Field passes it). */
    id?: string | undefined;
    /** The name when no Field labels it. */
    label?: string | undefined;
    'aria-describedby'?: string | undefined;
    placeholder?: string;
    size?: FieldSize;
    invalid?: boolean;
    disabled?: boolean;
    /** A search field over the list, for one long enough to need it. */
    searchable?: boolean;
    /** What each option shows at its end, the chosen one's on the field too: badges, a size. */
    aside?: Snippet<[SelectOption<T>]>;
    class?: string | undefined;
    onchange?: (value: T) => void;
  }

  let {
    value,
    options,
    id,
    label,
    'aria-describedby': describedBy,
    placeholder = 'Choose…',
    size,
    invalid = false,
    disabled = false,
    searchable = false,
    aside,
    class: className,
    onchange,
  }: Props = $props();

  const uid = $props.id();
  const listboxId = `${uid}-listbox`;
  // Index, not value: values may hold characters an id reference cannot.
  const optionId = (index: number) => `${uid}-option-${String(index)}`;

  let open = $state(false);
  /** The highlighted option, as its place in `shown`. */
  let highlighted = $state(-1);
  let query = $state('');
  let trigger = $state<HTMLButtonElement | null>(null);
  let panel = $state<HTMLElement | null>(null);
  let listbox = $state<HTMLElement | null>(null);
  let search = $state<HTMLInputElement | null>(null);
  const anchored = new Anchored();
  const typeahead = new Typeahead();

  const selected = $derived(options.find((option) => option.value === value));
  /** The options the list shows: all of them, or those the search finds. */
  const shown = $derived.by(() => {
    const words = query.trim().toLowerCase();
    if (!words) return options;
    return options.filter((option) =>
      `${option.label} ${option.description ?? ''}`.toLowerCase().includes(words),
    );
  });

  async function show() {
    if (disabled || open) return;
    query = '';
    open = true;
    const index = options.findIndex((option) => option.value === value);
    highlighted = index >= 0 ? index : nextEnabledIndex(options, -1, 1);
    await tick();
    if (trigger && panel) anchored.place(trigger.getBoundingClientRect(), panel, PLACEMENT);
    search?.focus();
  }

  function close() {
    open = false;
    highlighted = -1;
  }

  function choose(index: number) {
    const option = shown[index];
    if (!option || option.disabled === true) return;
    // Controlled: the owner may refuse the change, so the trigger shows only what comes back.
    if (option.value !== value) onchange?.(option.value);
    close();
    trigger?.focus();
  }

  /** Arrows, Home and End: move the highlight while open, open the list while closed. */
  function navigate(event: KeyboardEvent): boolean {
    if (!open) {
      if (event.key !== 'ArrowDown' && event.key !== 'ArrowUp') return false;
      event.preventDefault();
      void show();
      return true;
    }
    const next = listKeyIndex(shown, highlighted, event.key, 'vertical');
    if (next === null) return false;
    event.preventDefault();
    if (next >= 0) highlighted = next;
    return true;
  }

  /** Typing letters finds an option, searching on from the highlight while open and from the chosen option while closed: highlights it while open, chooses it while closed, as a native select does. */
  function find(event: KeyboardEvent) {
    if (!isTypeaheadKey(event)) return;
    const from = open ? highlighted : options.findIndex((option) => option.value === value);
    const found = typeaheadIndex(options, from, typeahead.push(event.key));
    if (found < 0) return;
    if (open) highlighted = found;
    else choose(found);
  }

  function onkeydown(event: KeyboardEvent) {
    if (disabled) return;
    if (event.key === 'Escape' && open) {
      event.preventDefault();
      close();
    } else if (event.key === 'Tab') {
      close();
    } else if (event.key === 'Enter' || event.key === ' ') {
      event.preventDefault();
      if (open) choose(highlighted);
      else void show();
    } else if (!navigate(event)) {
      find(event);
    }
  }

  /** The search field's keys: arrows move through what it finds, Enter chooses, Escape goes back to the field. */
  function onSearchKey(event: KeyboardEvent) {
    if (event.key === 'Enter') {
      event.preventDefault();
      // Enter chooses here; it never reaches a form around the field and submits it.
      event.stopPropagation();
      choose(highlighted);
    } else if (event.key === 'Escape') {
      event.preventDefault();
      event.stopPropagation();
      close();
      trigger?.focus();
    } else if (event.key === 'Tab') {
      close();
    } else {
      navigate(event);
    }
  }

  $effect(() => {
    if (!open || highlighted < 0 || !listbox) return;
    listbox
      .querySelector(`#${CSS.escape(optionId(highlighted))}`)
      ?.scrollIntoView({ block: 'nearest' });
  });

  $effect(() => {
    if (!open) return;
    return dismissOnOutside(() => [trigger, panel], trigger, close);
  });
</script>

<button
  bind:this={trigger}
  {id}
  type="button"
  role="combobox"
  class={field({
    size,
    invalid,
    class: cn('cursor-default justify-between gap-2 text-left', open && 'border-accent', className),
  })}
  aria-label={label}
  aria-describedby={describedBy}
  aria-haspopup="listbox"
  aria-expanded={open}
  aria-controls={listboxId}
  aria-activedescendant={!searchable && open && highlighted >= 0
    ? optionId(highlighted)
    : undefined}
  aria-invalid={invalid || undefined}
  {disabled}
  onclick={() => {
    if (open) close();
    else void show();
  }}
  {onkeydown}
>
  <span class={cn('flex min-w-0 items-center gap-2', !selected && 'text-ink-subtle')}>
    {#if selected?.icon}<Icon icon={selected.icon} size="sm" class="text-ink-muted" />{/if}
    <span class="truncate">{selected?.label ?? placeholder}</span>
  </span>
  <span class="flex shrink-0 items-center gap-1.5">
    {#if aside && selected}{@render aside(selected)}{/if}
    <Icon
      icon={ChevronDown}
      size="sm"
      class={cn('text-ink-muted transition-transform duration-normal', open && 'rotate-180')}
    />
  </span>
</button>

{#if open}
  <div
    bind:this={panel}
    popover="manual"
    class={cn(
      TOP_LAYER,
      FLYOUT_SURFACE,
      'z-popover flex max-h-80 min-w-menu flex-col overflow-hidden',
      anchored.above ? 'origin-bottom' : 'origin-top',
    )}
    style="{anchored.style} min-width: {anchored.anchorWidth}px;"
    {@attach topLayer}
    transition:pop
  >
    {#if searchable}
      <div class="flex items-center gap-2 border-b border-line px-3">
        <Icon icon={Search} size="sm" class="shrink-0 text-ink-subtle" />
        <input
          bind:this={search}
          bind:value={query}
          class={cn(FIELD_INPUT, 'h-control')}
          role="combobox"
          aria-label={label ? `Search ${label.toLowerCase()}` : 'Search'}
          aria-expanded="true"
          aria-controls={listboxId}
          aria-autocomplete="list"
          aria-activedescendant={highlighted >= 0 && shown.length > 0
            ? optionId(highlighted)
            : undefined}
          placeholder="Search"
          autocomplete="off"
          spellcheck="false"
          oninput={() => (highlighted = nextEnabledIndex(shown, -1, 1))}
          onkeydown={onSearchKey}
        />
      </div>
    {/if}
    <div
      bind:this={listbox}
      id={listboxId}
      role="listbox"
      tabindex="-1"
      aria-label={label}
      class="min-h-0 flex-1 overflow-y-auto p-1"
    >
      {#each shown as option, index (option.value)}
        {@const isSelected = option.value === value}
        <!-- Options take no focus: the trigger or the search keeps it and points here with aria-activedescendant. -->
        <div
          id={optionId(index)}
          role="option"
          aria-selected={isSelected}
          aria-disabled={option.disabled === true || undefined}
          tabindex="-1"
          class={cn(
            'relative flex cursor-default items-center gap-2.5 rounded-sm py-1.5 pr-2.5 pl-3 text-ui text-ink',
            highlighted === index &&
              'bg-wash-strong forced-colors:bg-sys-highlight forced-colors:text-sys-highlight-text',
            option.disabled === true && 'opacity-disabled',
          )}
          onclick={() => choose(index)}
          onkeydown={searchable ? onSearchKey : onkeydown}
          onpointermove={() => {
            if (option.disabled !== true) highlighted = index;
          }}
        >
          {#if isSelected}
            <span
              class="absolute top-1/2 left-0 h-4 w-1 -translate-y-1/2 rounded-full bg-accent"
              aria-hidden="true"
            ></span>
          {/if}
          {#if option.icon}<Icon icon={option.icon} size="sm" class="text-ink-muted" />{/if}
          <span class="flex min-w-0 flex-1 flex-col">
            <span class={cn('truncate', isSelected && 'font-medium')}>{option.label}</span>
            {#if option.description}
              <span class="text-small text-ink-muted">{option.description}</span>
            {/if}
          </span>
          {#if aside}
            <span class="flex shrink-0 items-center gap-1.5">{@render aside(option)}</span>
          {/if}
          <Icon icon={Check} size="sm" class={cn('text-accent', !isSelected && 'invisible')} />
        </div>
      {:else}
        <p class="px-3 py-3 text-ui text-ink-muted">Nothing matches “{query}”.</p>
      {/each}
    </div>
  </div>
{/if}
