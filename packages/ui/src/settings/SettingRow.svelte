<script lang="ts">
  // One setting in a SettingsSection: its name with its control at the line's end and what it does under both, across the row, or the control under them (`stacked`) when it needs the row's width, such as a list. The control is labelled and described by the row: spread what the row hands it (`<Select {...control} />`), as with Field.
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
  // Beside the control, the name's line is a control's height whatever the control, so every row keeps one rhythm.
  const nameClass = $derived(
    cn(
      'text-ui font-medium text-ink',
      !stacked && 'flex min-h-control min-w-0 flex-1 items-center',
    ),
  );
</script>

<div
  class={cn(
    'px-4 py-3',
    stacked ? 'flex flex-col gap-y-2.5' : 'flex flex-wrap items-center gap-x-6 gap-y-0.5',
    className,
  )}
>
  <!-- Beside the control, its parts sit in the row itself: the name on the control's line, taking the room the control leaves, and the description under both. -->
  <div class={cn('flex min-w-0 flex-col gap-0.5', !stacked && 'contents')}>
    {#if labelled}
      <label for={id} class={nameClass}>{label}</label>
    {:else}
      <p class={nameClass}>{label}</p>
    {/if}
    {#if description}
      <p id={hintId} class={cn('text-small text-ink-muted', !stacked && 'order-2 basis-full')}>
        {description}
      </p>
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
