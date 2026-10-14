<script lang="ts">
  // One setting in a SettingsSection: its name and what it does, with its control at the row's end, or under them (`stacked`) when the control needs the row's width, such as a list. The control is labelled and described by the row: spread what the row hands it (`<Select {...control} />`), as with Field.
  import type { Snippet } from 'svelte';
  import { cn } from '../lib/cn';
  import type { FieldControl } from '../primitives/field-control';

  interface Props {
    label: string;
    description?: string | undefined;
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
    description,
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
      [description ? hintId : null, error ? errorId : null]
        .filter((part) => part !== null)
        .join(' ') || undefined,
    invalid: Boolean(error),
  });
</script>

<div
  class={cn(
    'flex gap-x-6 gap-y-2.5 px-4 py-3',
    stacked ? 'flex-col' : 'flex-wrap items-center',
    className,
  )}
>
  <div class={cn('flex min-w-0 flex-col gap-0.5', !stacked && 'flex-1 basis-56')}>
    {#if labelled}
      <label for={id} class="text-ui text-ink">{label}</label>
    {:else}
      <p class="text-ui text-ink">{label}</p>
    {/if}
    {#if description}
      <p id={hintId} class="text-small text-ink-muted">{description}</p>
    {/if}
  </div>
  <div class={cn('flex max-w-full min-w-0 flex-wrap items-center gap-2', !stacked && 'shrink-0')}>
    {@render children(control)}
  </div>
  {#if error}
    <p id={errorId} role="alert" class="basis-full text-small text-danger">{error}</p>
  {/if}
</div>
