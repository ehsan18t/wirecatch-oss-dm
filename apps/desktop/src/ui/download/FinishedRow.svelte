<script lang="ts">
  // One download in the download complete window (D52): its kind, name and where it went, or why it failed, with Show in folder and Open, or Try again when that can fix it (a fresh link or another folder needs the download list). A download tried again from here shows how it goes until it ends again.
  import FolderOpen from '~icons/lucide/folder-open';
  import type { DownloadSummary } from '@wirecatch/protocol';
  import { Button, cn, IconButton } from '@wirecatch/ui';
  import { retryHelps } from '$lib/notify-options';
  import { folderOf, formatSize, statusShort } from '$lib/format';
  import type { FinishedWindowState } from '$lib/window/finished-window.svelte';
  import CategoryIcon from '../CategoryIcon.svelte';
  import { STATUS_TEXT } from '../status';

  let { win, row }: { win: FinishedWindowState; row: DownloadSummary } = $props();

  const where = $derived(
    row.status === 'completed'
      ? `${formatSize(row.done, row.total, row.status)} · ${folderOf(row.path)}`
      : row.status === 'failed'
        ? (row.error?.message ?? 'It stopped with an error.')
        : statusShort(row),
  );
</script>

<li class="flex animate-rise-in items-center gap-3 py-2.5" data-finished-row={row.id}>
  <CategoryIcon category={row.category} />
  <div class="flex min-w-0 flex-1 flex-col gap-0.5">
    <span class="truncate text-ui font-medium text-ink" title={row.file_name}>{row.file_name}</span>
    <span
      class={cn(
        'text-small',
        row.status === 'failed' ? 'line-clamp-3' : 'truncate tabular-nums',
        STATUS_TEXT[row.status],
      )}
      title={where}>{where}</span
    >
  </div>
  <div class="flex shrink-0 items-center gap-1">
    {#if row.status === 'completed'}
      <IconButton
        icon={FolderOpen}
        size="sm"
        label="Show {row.file_name} in its folder"
        tooltip="Show in folder"
        onclick={() => void win.showInFolder(row.id)}
      />
      <Button size="sm" aria-label="Open {row.file_name}" onclick={() => void win.open(row.id)}
        >Open</Button
      >
    {:else if row.status === 'failed' && retryHelps(row)}
      <Button
        size="sm"
        aria-label="Try {row.file_name} again"
        onclick={() => void win.retry(row.id)}>Try again</Button
      >
    {/if}
  </div>
</li>
