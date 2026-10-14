<script lang="ts" generics="T extends string | number">
  // Native radios in a fieldset: the browser gives them their arrow keys and one tab stop. Drawn in the design's colours; in forced colours, the system's own. As cards, each option is a bordered row and the chosen one has a heavier accent edge (not a tinted fill, which would take the contrast from accent badges in it), for choices that carry facts of their own (a video's qualities with their sizes).
  import type { Snippet } from 'svelte';
  import { cn } from '../lib/cn';
  import { DISABLED, FOCUS_RING } from '../recipes/control';
  import type { SelectOption } from './select';

  interface Props {
    value: T | null;
    options: readonly SelectOption<T>[];
    /** The group's name, shown as its legend. */
    label: string;
    hideLabel?: boolean;
    /** The form field name; one is made up when the group is not part of a form. */
    name?: string | undefined;
    orientation?: 'vertical' | 'horizontal';
    /** Each option a bordered row, the chosen one marked. */
    cards?: boolean;
    /** What a row adds at its end (badges, a size), read to screen readers as the option's description. */
    aside?: Snippet<[SelectOption<T>, number]>;
    disabled?: boolean;
    class?: string | undefined;
    onchange?: (value: T) => void;
  }

  let {
    value,
    options,
    label,
    hideLabel = false,
    name,
    orientation = 'vertical',
    cards = false,
    aside,
    disabled = false,
    class: className,
    onchange,
  }: Props = $props();

  const uid = $props.id();
  const groupName = $derived(name ?? uid);
</script>

<fieldset class={cn('min-w-0', className)} {disabled}>
  <legend class={cn('mb-2 text-ui font-medium text-ink', hideLabel && 'sr-only')}>{label}</legend>
  <div
    class={cn(
      'flex',
      cards ? 'gap-1.5' : 'gap-2.5',
      orientation === 'vertical' ? 'flex-col' : 'flex-row flex-wrap gap-x-5',
    )}
  >
    {#each options as option, index (option.value)}
      {@const hintId = `${uid}-hint-${String(index)}`}
      {@const asideId = `${uid}-aside-${String(index)}`}
      {@const labelId = `${uid}-label-${String(index)}`}
      {@const checked = option.value === value}
      {@const described = [option.description ? hintId : '', aside ? asideId : '']
        .filter(Boolean)
        .join(' ')}
      <label
        data-checked={checked ? '' : undefined}
        class={cn(
          'inline-flex items-start gap-2.5 text-ui text-ink select-none',
          cards &&
            'flex items-center rounded-md border border-line bg-control px-3 py-2 transition hover:bg-control-hover data-checked:border-accent data-checked:ring-1 data-checked:ring-accent forced-colors:data-checked:border-sys-highlight',
          (disabled || option.disabled === true) && 'cursor-not-allowed opacity-disabled',
        )}
      >
        <input
          type="radio"
          name={groupName}
          value={option.value}
          {checked}
          disabled={option.disabled}
          aria-labelledby={labelId}
          aria-describedby={described || undefined}
          class={cn(
            'size-4 shrink-0 appearance-none rounded-full border border-line-strong bg-field transition',
            !cards && 'mt-0.5',
            'enabled:hover:border-ink-muted checked:border-5 checked:border-accent checked:bg-accent-fg',
            'enabled:active:scale-pressed forced-colors:appearance-auto',
            FOCUS_RING,
            DISABLED,
          )}
          onchange={() => onchange?.(option.value)}
        />
        <span class="flex min-w-0 flex-1 flex-col">
          <span id={labelId}>{option.label}</span>
          {#if option.description}
            <span id={hintId} class="text-small text-ink-muted">{option.description}</span>
          {/if}
        </span>
        {#if aside}
          <span id={asideId} class="flex shrink-0 flex-wrap items-center justify-end gap-1.5">
            {@render aside(option, index)}
          </span>
        {/if}
      </label>
    {/each}
  </div>
</fieldset>
