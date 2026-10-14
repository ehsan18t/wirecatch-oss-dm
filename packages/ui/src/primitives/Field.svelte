<script lang="ts">
  import type { Snippet } from 'svelte';
  import { cn } from '../lib/cn';
  import type { FieldControl } from './field-control';

  interface Props {
    label: string;
    /** A hint under the control. */
    description?: string | undefined;
    /** What is wrong with the value; shown in place of the hint, and marks the control invalid. */
    error?: string | null | undefined;
    /** Shows the label to screen readers only, when the layout already says what the control is. */
    hideLabel?: boolean;
    class?: string | undefined;
    children: Snippet<[FieldControl]>;
  }

  let {
    label,
    description,
    error,
    hideLabel = false,
    class: className,
    children,
  }: Props = $props();

  const id = $props.id();
  const hintId = `${id}-hint`;
  const message = $derived(error ?? description);
  const control = $derived<FieldControl>({
    id,
    'aria-describedby': message ? hintId : undefined,
    invalid: Boolean(error),
  });
</script>

<div class={cn('flex min-w-0 flex-col gap-1.5', className)}>
  <label for={id} class={cn('text-ui font-medium text-ink', hideLabel && 'sr-only')}>{label}</label>
  {@render children(control)}
  {#if message}
    <p id={hintId} class={cn('text-small', error ? 'text-danger' : 'text-ink-muted')}>{message}</p>
  {/if}
</div>
