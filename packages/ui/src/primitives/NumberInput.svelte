<script lang="ts">
  import Minus from '~icons/lucide/minus';
  import Plus from '~icons/lucide/plus';
  import type { HTMLInputAttributes } from 'svelte/elements';
  import { cn } from '../lib/cn';
  import { FIELD_INPUT, field, type FieldSize } from '../recipes/field';
  import { FOCUS_RING } from '../recipes/control';
  import Icon from './Icon.svelte';
  import { clampToRange, keyValue, parseNumber, stepValue } from './number-input';

  interface Props extends Omit<
    HTMLInputAttributes,
    // The spin button's own handlers commit what is typed; one passed in would replace them.
    | 'class'
    | 'size'
    | 'value'
    | 'min'
    | 'max'
    | 'step'
    | 'type'
    | 'onchange'
    | 'oninput'
    | 'onblur'
    | 'onkeydown'
    | 'aria-label'
  > {
    /** The name when no Field labels it. */
    label?: string | undefined;
    /** Null is an empty field, when `optional`. */
    value: number | null;
    min?: number | undefined;
    max?: number | undefined;
    step?: number;
    /** The unit shown after the number, such as "KB/s". */
    unit?: string | undefined;
    /** Allows an empty field (null), such as "no limit". */
    optional?: boolean;
    size?: FieldSize;
    invalid?: boolean;
    /** Called with each committed value: a step, or typed text when the field loses focus or Enter is pressed. */
    onchange?: (value: number | null) => void;
    class?: string | undefined;
  }

  let {
    value,
    min,
    max,
    step = 1,
    unit,
    optional = false,
    size,
    invalid = false,
    label,
    onchange,
    disabled,
    class: className,
    ...rest
  }: Props = $props();

  const range = $derived({ min, max, step });
  // What is typed, while it is typed; the committed value otherwise.
  let draft = $state<string | null>(null);
  const shown = $derived(draft ?? (value === null ? '' : String(value)));

  function commit(next: number | null) {
    draft = null;
    if (next !== value) onchange?.(next);
  }

  function commitDraft() {
    if (draft === null) return;
    const parsed = parseNumber(draft);
    if (parsed === null) commit(optional ? null : value);
    else if (Number.isNaN(parsed)) draft = null;
    else commit(clampToRange(parsed, range));
  }

  /** What a key steps from: the typed number, or the committed value while the text is not a number. */
  function typedOrValue(): number | null {
    if (draft === null) return value;
    const typed = parseNumber(draft);
    return typed !== null && Number.isNaN(typed) ? value : typed;
  }

  function onkeydown(event: KeyboardEvent) {
    if (event.key === 'Enter') {
      commitDraft();
      return;
    }
    if (event.key === 'Escape' && draft !== null) {
      event.preventDefault();
      draft = null;
      return;
    }
    const next = keyValue(event.key, typedOrValue(), range);
    if (next === null) return;
    event.preventDefault();
    commit(next);
  }

  const STEPPER = cn(
    'flex h-full items-center justify-center rounded-xs px-1 text-ink-muted enabled:hover:bg-wash enabled:hover:text-ink disabled:opacity-disabled',
    FOCUS_RING,
  );
</script>

<div class={field({ size, invalid, class: cn('pr-0.5', className) })}>
  <input
    type="text"
    inputmode={step % 1 === 0 ? 'numeric' : 'decimal'}
    role="spinbutton"
    aria-label={label}
    class={cn(FIELD_INPUT, 'tabular-nums')}
    value={shown}
    aria-valuenow={value ?? undefined}
    aria-valuemin={min}
    aria-valuemax={max}
    aria-valuetext={value === null ? undefined : `${String(value)}${unit ? ` ${unit}` : ''}`}
    aria-invalid={invalid || undefined}
    {disabled}
    oninput={(event) => (draft = event.currentTarget.value)}
    onblur={commitDraft}
    {onkeydown}
    {...rest}
  />
  {#if unit}
    <span class="shrink-0 text-ink-subtle" aria-hidden="true">{unit}</span>
  {/if}
  <!-- Arrow keys step the value; these are for the pointer, so they stay out of the tab order. -->
  <span class="flex h-full items-center py-0.5">
    <button
      type="button"
      tabindex="-1"
      class={STEPPER}
      aria-label="Decrease"
      disabled={disabled === true || (min !== undefined && value !== null && value <= min)}
      onclick={() => commit(stepValue(value, -1, range))}
    >
      <Icon icon={Minus} size="xs" />
    </button>
    <button
      type="button"
      tabindex="-1"
      class={STEPPER}
      aria-label="Increase"
      disabled={disabled === true || (max !== undefined && value !== null && value >= max)}
      onclick={() => commit(stepValue(value, 1, range))}
    >
      <Icon icon={Plus} size="xs" />
    </button>
  </span>
</div>
