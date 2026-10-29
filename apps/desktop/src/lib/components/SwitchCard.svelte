<script lang="ts">
  // One of the queue dialog's cards, or a row of a grouped card (`flush`): a switch that opens its own fields underneath when it is on, and while it is off shows the global value it follows.
  import type { Snippet } from 'svelte';
  import { Switch } from '@wirecatch/ui';

  let {
    label,
    help,
    inherited = null,
    checked,
    onchange,
    flush = false,
    children,
  }: {
    label: string;
    help: string;
    /** The global value used while it is off ("Global: 3"). */
    inherited?: string | null;
    checked: boolean;
    onchange: (on: boolean) => void;
    flush?: boolean;
    children: Snippet;
  } = $props();
</script>

<section
  class={flush
    ? 'flex flex-col gap-3 p-3'
    : 'flex flex-col gap-3 rounded-md border border-line bg-layer p-3'}
>
  <Switch labelSide="start" {label} {help} {checked} {onchange}>
    {#if !checked && inherited}
      <span class="text-small text-ink-muted">{inherited}</span>
    {/if}
  </Switch>
  {#if checked}
    {@render children()}
  {/if}
</section>
