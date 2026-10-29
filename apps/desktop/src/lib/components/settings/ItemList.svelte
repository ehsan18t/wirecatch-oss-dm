<script lang="ts" generics="T">
  // A settings list of saved things (proxy servers, queues): a row each, or one line saying there are none yet. Its New and other buttons sit in the setting's name line (`actions` of SettingRows), so they stay put however long the list.
  import type { Snippet } from 'svelte';

  interface Props {
    items: readonly T[];
    key: (item: T) => string | number;
    /** The list's name for screen readers. */
    label: string;
    /** What shows while there are none. */
    empty: string;
    describedby?: string | undefined;
    row: Snippet<[T]>;
  }

  let { items, key, label, empty, describedby, row }: Props = $props();
</script>

{#if items.length === 0}
  <p class="text-small text-ink-muted">{empty}</p>
{:else}
  <ul
    aria-label={label}
    aria-describedby={describedby}
    class="flex w-full flex-col divide-y divide-line rounded-md border border-line"
  >
    {#each items as item (key(item))}
      <li class="flex flex-wrap items-center gap-x-3 gap-y-1 px-3 py-2.5">{@render row(item)}</li>
    {/each}
  </ul>
{/if}
