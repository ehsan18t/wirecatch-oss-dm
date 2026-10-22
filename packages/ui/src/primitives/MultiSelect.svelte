<script lang="ts" module>
  import type { Placement } from '../lib/flyout';

  const PLACEMENT: Placement = { side: 'below', align: 'start', gap: 4, inset: 8 };
</script>

<script lang="ts" generics="T extends string | number">
  // Several choices out of a long list, such as a video's subtitle languages: a field that says what is picked and opens a list to search and tick. The search field keeps the focus and points at the highlighted option (aria-activedescendant); Up and Down move, Enter ticks or unticks, Escape closes. What is picked lists first when the list opens, so it is never lost among the rest.
  import Check from '~icons/lucide/check';
  import ChevronDown from '~icons/lucide/chevron-down';
  import Search from '~icons/lucide/search';
  import { tick } from 'svelte';
  import { Anchored, dismissOnOutside } from '../lib/anchored.svelte';
  import { cn } from '../lib/cn';
  import { topLayer } from '../lib/layer';
  import { pop } from '../motion/motion';
  import { field, FIELD_INPUT, type FieldSize } from '../recipes/field';
  import { FLYOUT_SURFACE, TOP_LAYER } from '../recipes/surface';
  import Icon from './Icon.svelte';
  import { searched, type SelectOption } from './select';

  interface Props {
    values: readonly T[];
    options: readonly SelectOption<T>[];
    /** The trigger's id, for a `<label for>` (Field passes it). */
    id?: string | undefined;
    /** The name when no Field labels it. */
    label?: string | undefined;
    'aria-describedby'?: string | undefined;
    /** What the field says while nothing is picked. */
    placeholder?: string;
    /** The search field's hint. */
    searchPlaceholder?: string;
    size?: FieldSize;
    disabled?: boolean;
    class?: string | undefined;
    onchange?: (values: T[]) => void;
  }

  let {
    values,
    options,
    id,
    label,
    'aria-describedby': describedBy,
    placeholder = 'None',
    searchPlaceholder = 'Search',
    size,
    disabled = false,
    class: className,
    onchange,
  }: Props = $props();

  const uid = $props.id();
  const listboxId = `${uid}-listbox`;
  const panelId = `${uid}-panel`;
  const optionId = (index: number) => `${uid}-option-${String(index)}`;

  let open = $state(false);
  let query = $state('');
  let highlighted = $state(0);
  /** The order the list opened in: picked first. Kept while it is open, so a tick never moves a row. */
  let order = $state<readonly SelectOption<T>[]>([]);
  let trigger = $state<HTMLButtonElement | null>(null);
  let panel = $state<HTMLElement | null>(null);
  let search = $state<HTMLInputElement | null>(null);
  const anchored = new Anchored();

  const picked = $derived(options.filter((option) => values.includes(option.value)));
  const shown = $derived(searched(order, query));

  async function show() {
    if (disabled || open) return;
    order = [...picked, ...options.filter((option) => !values.includes(option.value))];
    query = '';
    highlighted = 0;
    open = true;
    await tick();
    if (trigger && panel) anchored.place(trigger.getBoundingClientRect(), panel, PLACEMENT);
    search?.focus();
  }

  function close(refocus = true) {
    open = false;
    if (refocus) trigger?.focus();
  }

  function toggle(option: SelectOption<T> | undefined) {
    if (!option || option.disabled === true) return;
    onchange?.(
      values.includes(option.value)
        ? values.filter((value) => value !== option.value)
        : [...values, option.value],
    );
  }

  function onSearchKey(event: KeyboardEvent) {
    const last = shown.length - 1;
    if (event.key === 'ArrowDown') {
      event.preventDefault();
      highlighted = Math.min(last, highlighted + 1);
    } else if (event.key === 'ArrowUp') {
      event.preventDefault();
      highlighted = Math.max(0, highlighted - 1);
    } else if (event.key === 'Enter') {
      event.preventDefault();
      // Enter ticks here; it never reaches a form around the field and submits it.
      event.stopPropagation();
      toggle(shown[highlighted]);
    } else if (event.key === 'Escape') {
      event.preventDefault();
      event.stopPropagation();
      close();
    } else if (event.key === 'Tab') {
      close(false);
    }
  }

  function onTriggerKey(event: KeyboardEvent) {
    if (event.key === 'ArrowDown' || event.key === 'ArrowUp') {
      event.preventDefault();
      void show();
    }
  }

  $effect(() => {
    if (!open || !panel) return;
    panel.querySelector(`#${CSS.escape(optionId(highlighted))}`)?.scrollIntoView({
      block: 'nearest',
    });
  });

  $effect(() => {
    if (!open) return;
    return dismissOnOutside(
      () => [trigger, panel],
      trigger,
      () => close(false),
    );
  });
</script>

<button
  bind:this={trigger}
  {id}
  type="button"
  aria-haspopup="dialog"
  aria-expanded={open}
  aria-controls={panelId}
  aria-label={label}
  aria-describedby={describedBy}
  {disabled}
  class={field({
    size,
    class: cn('cursor-default justify-between text-left', open && 'border-accent', className),
  })}
  onclick={() => {
    if (open) close();
    else void show();
  }}
  onkeydown={onTriggerKey}
>
  <span class={cn('min-w-0 truncate', picked.length === 0 && 'text-ink-subtle')}>
    {picked.length === 0 ? placeholder : picked.map((option) => option.label).join(', ')}
  </span>
  <span class="flex shrink-0 items-center gap-1.5">
    {#if picked.length > 1}
      <span
        class="rounded-full bg-accent-soft px-1.5 text-caption font-medium text-accent tabular-nums"
        >{picked.length}</span
      >
    {/if}
    <Icon
      icon={ChevronDown}
      size="sm"
      class={cn('text-ink-muted transition-transform duration-normal', open && 'rotate-180')}
    />
  </span>
</button>

{#if open}
  <!-- A small dialog, as the APG's combobox with a dialog popup: a search field and the list it filters. -->
  <div
    bind:this={panel}
    id={panelId}
    role="dialog"
    aria-label={label}
    tabindex="-1"
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
    onkeydown={(event) => {
      // Escape closes from anywhere in the list, the Clear button too.
      if (event.key !== 'Escape') return;
      event.preventDefault();
      event.stopPropagation();
      close();
    }}
  >
    <div class="flex items-center gap-2 border-b border-line px-3">
      <Icon icon={Search} size="sm" class="shrink-0 text-ink-subtle" />
      <input
        bind:this={search}
        bind:value={query}
        class={cn(FIELD_INPUT, 'h-control')}
        role="combobox"
        aria-label={label ? `Search ${label.toLowerCase()}` : searchPlaceholder}
        aria-expanded="true"
        aria-controls={listboxId}
        aria-autocomplete="list"
        aria-activedescendant={shown.length > 0 ? optionId(highlighted) : undefined}
        placeholder={searchPlaceholder}
        autocomplete="off"
        spellcheck="false"
        oninput={() => (highlighted = 0)}
        onkeydown={onSearchKey}
      />
    </div>
    <div
      id={listboxId}
      role="listbox"
      aria-multiselectable="true"
      aria-label={label}
      tabindex="-1"
      class="min-h-0 flex-1 overflow-y-auto p-1"
    >
      {#each shown as option, index (option.value)}
        {@const on = values.includes(option.value)}
        <!-- Options take no focus: the search field keeps it and points here. -->
        <div
          id={optionId(index)}
          role="option"
          aria-selected={on}
          aria-disabled={option.disabled === true || undefined}
          tabindex="-1"
          class={cn(
            'flex cursor-default items-center gap-2.5 rounded-sm px-2.5 py-1.5 text-ui text-ink',
            highlighted === index &&
              'bg-wash-strong forced-colors:bg-sys-highlight forced-colors:text-sys-highlight-text',
            option.disabled === true && 'opacity-disabled',
          )}
          onclick={() => toggle(option)}
          onkeydown={onSearchKey}
          onpointermove={() => (highlighted = index)}
        >
          <span
            class={cn(
              'flex size-4 shrink-0 items-center justify-center rounded-xs border',
              on ? 'border-accent bg-accent text-accent-fg' : 'border-line-strong bg-field',
            )}
            aria-hidden="true"
          >
            {#if on}<Icon icon={Check} size="xs" />{/if}
          </span>
          <span class="flex min-w-0 flex-1 flex-col">
            <span class="truncate">{option.label}</span>
            {#if option.description}
              <span class="text-small text-ink-muted">{option.description}</span>
            {/if}
          </span>
        </div>
      {:else}
        <p class="px-2.5 py-3 text-ui text-ink-muted">Nothing matches “{query}”.</p>
      {/each}
    </div>
    {#if values.length > 0}
      <div class="flex items-center justify-between border-t border-line px-3 py-1.5">
        <span class="text-small text-ink-muted">{values.length} picked</span>
        <button
          type="button"
          class="rounded-sm px-1.5 text-small font-medium text-accent hover:underline"
          onclick={() => {
            onchange?.([]);
            search?.focus();
          }}>Clear</button
        >
      </div>
    {/if}
  </div>
{/if}
