<script lang="ts" module>
  import type { Placement } from '../lib/flyout';

  const PLACEMENT: Placement = { side: 'below', align: 'start', gap: 4, inset: 8 };
</script>

<script lang="ts" generics="T extends string | number">
  // A select-only combobox (WAI-ARIA APG): a button that opens a listbox, the highlight followed through aria-activedescendant so focus never leaves the button.
  import Check from '~icons/lucide/check';
  import ChevronDown from '~icons/lucide/chevron-down';
  import { tick } from 'svelte';
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
  import { field, type FieldSize } from '../recipes/field';
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
    class: className,
    onchange,
  }: Props = $props();

  const uid = $props.id();
  const listboxId = `${uid}-listbox`;
  // Index, not value: values may hold characters an id reference cannot.
  const optionId = (index: number) => `${uid}-option-${String(index)}`;

  let open = $state(false);
  let highlighted = $state(-1);
  let trigger = $state<HTMLButtonElement | null>(null);
  let listbox = $state<HTMLElement | null>(null);
  const anchored = new Anchored();
  const typeahead = new Typeahead();

  const selected = $derived(options.find((option) => option.value === value));

  async function show() {
    if (disabled || open) return;
    open = true;
    const index = options.findIndex((option) => option.value === value);
    highlighted = index >= 0 ? index : nextEnabledIndex(options, -1, 1);
    await tick();
    if (trigger && listbox) anchored.place(trigger.getBoundingClientRect(), listbox, PLACEMENT);
  }

  function close() {
    open = false;
    highlighted = -1;
  }

  function choose(index: number) {
    const option = options[index];
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
    const next = listKeyIndex(options, highlighted, event.key, 'vertical');
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

  $effect(() => {
    if (!open || highlighted < 0 || !listbox) return;
    listbox
      .querySelector(`#${CSS.escape(optionId(highlighted))}`)
      ?.scrollIntoView({ block: 'nearest' });
  });

  $effect(() => {
    if (!open) return;
    return dismissOnOutside(() => [trigger, listbox], trigger, close);
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
    class: cn('cursor-default justify-between text-left', open && 'border-b-accent', className),
  })}
  aria-label={label}
  aria-describedby={describedBy}
  aria-haspopup="listbox"
  aria-expanded={open}
  aria-controls={listboxId}
  aria-activedescendant={open && highlighted >= 0 ? optionId(highlighted) : undefined}
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
  <Icon
    icon={ChevronDown}
    size="sm"
    class={cn('text-ink-muted transition-transform duration-normal', open && 'rotate-180')}
  />
</button>

{#if open}
  <div
    bind:this={listbox}
    id={listboxId}
    role="listbox"
    popover="manual"
    tabindex="-1"
    aria-label={label}
    class={cn(
      TOP_LAYER,
      FLYOUT_SURFACE,
      'z-popover max-h-80 min-w-menu overflow-y-auto p-1',
      anchored.above ? 'origin-bottom' : 'origin-top',
    )}
    style="{anchored.style} min-width: {anchored.anchorWidth}px;"
    {@attach topLayer}
    transition:pop
  >
    {#each options as option, index (option.value)}
      {@const isSelected = option.value === value}
      <!-- Options take no focus: the trigger keeps it and points here with aria-activedescendant. -->
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
        {onkeydown}
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
        <Icon icon={Check} size="sm" class={cn('text-accent', !isSelected && 'invisible')} />
      </div>
    {/each}
  </div>
{/if}
