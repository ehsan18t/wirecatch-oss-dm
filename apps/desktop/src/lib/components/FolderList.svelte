<script lang="ts" module>
  /** A folder setting as an import lists it: what it is now, and why, when that needs saying. */
  export interface FolderRow {
    setting: string;
    resolved: string;
    note: string | null;
    /** The folder needs a look before downloads go there. */
    flagged: boolean;
  }
</script>

<script lang="ts">
  // The folders an import sets, each with the reason it is not the one the file asked for.
  import { cn } from '@wirecatch/ui';

  let { rows }: { rows: readonly FolderRow[] } = $props();
</script>

<ul class="flex flex-col divide-y divide-line rounded-md border border-line">
  {#each rows as row (row.setting)}
    <li class="flex flex-col gap-0.5 px-3 py-2">
      <span class="text-ui text-ink-muted"
        >{row.setting}:
        <span class={cn('font-medium break-all', row.flagged ? 'text-danger' : 'text-ink')}
          >{row.resolved}</span
        ></span
      >
      {#if row.note}<span class="text-small text-ink-subtle">{row.note}</span>{/if}
    </li>
  {/each}
</ul>
