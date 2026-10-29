<script lang="ts">
  // One setting in a SettingsSection: its name with its control at the line's end and what it does under both, across the row, or the control under them (`stacked`) when it needs the row's width, such as a list. The control is labelled and described by the row: spread what the row hands it (`<Select {...control} />`), as with Field.
  import type { Snippet } from 'svelte';
  import { cn } from '../lib/cn';
  import type { FieldControl } from '../primitives/field-control';
  import InfoTip from '../primitives/InfoTip.svelte';

  interface Props {
    label: string;
    /** What the setting does, behind an info icon after the label (D84). */
    help?: string | undefined;
    /** What is wrong with the value: shown under the control and announced as it appears, and it marks the control invalid. */
    error?: string | null | undefined;
    stacked?: boolean;
    /** False for a row whose end shows a value or an action rather than a control the label names, such as a button with its own name: the label is then plain text. */
    labelled?: boolean;
    class?: string | undefined;
    children: Snippet<[FieldControl]>;
  }

  let {
    label,
    help,
    error,
    stacked = false,
    labelled = true,
    class: className,
    children,
  }: Props = $props();

  const id = $props.id();
  const hintId = `${id}-hint`;
  const errorId = `${id}-error`;
  const control = $derived<FieldControl>({
    id,
    'aria-describedby':
      [help ? hintId : null, error ? errorId : null].filter((part) => part !== null).join(' ') ||
      undefined,
    invalid: Boolean(error),
  });
  // Beside the control, the name's line is a control's height whatever the control, so every row keeps one rhythm.
  const labelClass = $derived(!stacked ? 'flex min-h-control items-center' : undefined);
  const nameClass = $derived(
    cn('flex min-w-0 items-center gap-1 text-ui font-medium text-ink', !stacked && 'flex-1'),
  );
</script>

<div
  class={cn(
    'px-4 py-3',
    stacked ? 'flex flex-col gap-y-2.5' : 'flex flex-wrap items-center gap-x-6 gap-y-0.5',
    className,
  )}
>
  <div class={nameClass}>
    {#if labelled}
      <label for={id} class={labelClass}>{label}</label>
    {:else}
      <p class={labelClass}>{label}</p>
    {/if}
    {#if help}
      <InfoTip {label} text={help} />
      <span id={hintId} class="sr-only">{help}</span>
    {/if}
  </div>
  <div
    class={cn(
      'flex max-w-full min-w-0 flex-wrap items-center gap-2',
      !stacked && 'order-1 shrink-0 justify-end',
    )}
  >
    {@render children(control)}
  </div>
  {#if error}
    <p id={errorId} role="alert" class="order-3 basis-full text-small text-danger">{error}</p>
  {/if}
</div>
