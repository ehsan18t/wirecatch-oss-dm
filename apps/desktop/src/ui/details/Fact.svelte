<script lang="ts">
  // One line of the details' facts: what it is, and its value with anything that changes it.
  import type { Snippet } from 'svelte';
  import { cn } from '@wirecatch/ui';

  interface Props {
    label: string;
    /** The value is a control (a queue to choose): the label lines up with its middle. */
    control?: boolean;
    children: Snippet;
  }

  let { label, control = false, children }: Props = $props();
</script>

<div class={cn('flex gap-3', control ? 'items-center' : 'items-start')}>
  <dt class={cn('w-28 shrink-0 text-small text-ink-muted', !control && 'pt-0.5')}>{label}</dt>
  <dd class="flex min-w-0 flex-1 flex-col items-start gap-1.5 text-ui text-ink tabular-nums">
    {@render children()}
  </dd>
</div>
