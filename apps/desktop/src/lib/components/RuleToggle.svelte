<script lang="ts">
  // One kind of queue rule: a checkbox, and its values under it only while it is checked (D84.5).
  import type { Snippet } from 'svelte';
  import { Checkbox } from '@wirecatch/ui';

  let {
    label,
    checked,
    onchange,
    children,
  }: {
    label: string;
    checked: boolean;
    onchange: (checked: boolean) => void;
    children: Snippet;
  } = $props();
</script>

<div class="flex flex-col gap-2">
  <Checkbox {label} {checked} {onchange} />
  {#if checked}
    <div class="flex flex-col gap-2 pl-6.5">
      {@render children()}
    </div>
  {/if}
</div>
