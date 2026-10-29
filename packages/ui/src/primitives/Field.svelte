<script lang="ts">
  import type { Snippet } from 'svelte';
  import { cn } from '../lib/cn';
  import type { FieldControl } from './field-control';
  import InfoTip from './InfoTip.svelte';

  interface Props {
    label: string;
    /** What the field accepts, under the control; never what an option does (D84.3). */
    description?: string | undefined;
    /** What the field is for, behind an info icon after the label. */
    help?: string | undefined;
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
    help,
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
  {#if help && !hideLabel}
    <div class="flex items-center gap-1">
      <label for={id} class="text-ui font-medium text-ink">{label}</label>
      <InfoTip {label} text={help} />
    </div>
  {:else}
    <label for={id} class={cn('text-ui font-medium text-ink', hideLabel && 'sr-only')}
      >{label}</label
    >
  {/if}
  {@render children(control)}
  {#if message}
    <p id={hintId} class={cn('text-small', error ? 'text-danger' : 'text-ink-muted')}>{message}</p>
  {/if}
</div>
