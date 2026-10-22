<script lang="ts">
  // What the download complete window shows (D52). One download gets the room: its file, where it went, and Open as the main button, or why it failed and Try again. Several are a list, the newest first, a queue's summary under a heading of its own. The window never closes by itself; "Use notifications instead" switches the setting in one click.
  import FolderOpen from '~icons/lucide/folder-open';
  import { MAX_FINISHED_LISTED, type FinishedGroup } from '@wirecatch/protocol';
  import { Button, Callout } from '@wirecatch/ui';
  import { finishedTitle, retryHelps } from '$lib/finish-notices';
  import { folderOf, formatSize, statusShort } from '$lib/format';
  import type { FinishedWindowState } from '$lib/window/finished-window.svelte';
  import CategoryIcon from '../CategoryIcon.svelte';
  import FinishedRow from './FinishedRow.svelte';
  import WindowFrame from './WindowFrame.svelte';

  let { win }: { win: FinishedWindowState } = $props();

  const title = $derived(finishedTitle(win.counts.completed, win.counts.failed));
  const only = $derived(win.rows.length === 1 ? win.rows[0] : undefined);

  /** A queue's heading: that it is done, what it came to when other groups share the window (the title says it otherwise), and that the list stops short when it does. */
  function heading(group: FinishedGroup): string | null {
    const queue = group.queue;
    if (!queue) return null;
    const cut =
      group.downloads.length >= MAX_FINISHED_LISTED &&
      queue.completed + queue.failed > MAX_FINISHED_LISTED;
    // A download that ended again since moved to a newer group: count what this one lists.
    const listed = (status: string) => group.downloads.filter((d) => d.status === status).length;
    const completed = cut ? queue.completed : listed('completed');
    const failed = cut ? queue.failed : listed('failed');
    const counts = [
      completed > 0 ? `${String(completed)} finished` : null,
      failed > 0 ? `${String(failed)} failed` : null,
    ].filter((part) => part !== null);
    const done =
      win.groups.length > 1
        ? `${queue.name} is done: ${counts.join(' and ')}`
        : `${queue.name} is done`;
    return cut ? `${done}. The latest ${String(MAX_FINISHED_LISTED)} are listed.` : done;
  }
</script>

<WindowFrame {title} error={win.error}>
  {#if only}
    <div class="flex flex-col gap-4" data-finished-row={only.id}>
      <div class="flex items-center gap-3">
        <CategoryIcon category={only.category} size="lg" />
        <div class="flex min-w-0 flex-col gap-0.5">
          <p class="text-ui font-semibold text-ink wrap-anywhere">{only.file_name}</p>
          <p class="text-small text-ink-muted tabular-nums">
            {only.status === 'completed' || only.status === 'failed'
              ? formatSize(only.done, only.total, only.status)
              : statusShort(only)}
          </p>
        </div>
      </div>
      {#if only.status === 'failed'}
        <Callout tone="danger">{only.error?.message ?? 'It stopped with an error.'}</Callout>
        {#if !retryHelps(only)}
          <p class="text-small text-ink-muted">Fix it from the download list in Wirecatch.</p>
        {/if}
      {:else}
        <dl>
          <dt class="text-small text-ink-muted">Saved to</dt>
          <dd class="text-ui text-ink wrap-anywhere">{folderOf(only.path)}</dd>
        </dl>
      {/if}
    </div>
  {:else}
    <div class="flex flex-col gap-4">
      {#each win.groups as group (group.key)}
        {@const label = heading(group)}
        <section aria-label={label ?? 'Finished one by one'} class="flex flex-col">
          {#if label}
            <h2 class="text-small font-semibold text-ink-muted">{label}</h2>
          {/if}
          <ul class="flex flex-col divide-y divide-line">
            {#each group.downloads as row (row.id)}
              <FinishedRow {win} {row} />
            {/each}
          </ul>
        </section>
      {/each}
    </div>
  {/if}
  {#if win.settings?.finish_notice === 'window'}
    <p class="mt-4 text-small text-ink-muted">
      Rather not have a window?
      <Button variant="link" size="sm" onclick={() => void win.useNotifications()}
        >Use notifications instead</Button
      >
    </p>
  {/if}

  {#snippet footer()}
    {#if only?.status === 'completed'}
      <Button variant="ghost" onclick={() => win.close()}>Close</Button>
      <Button icon={FolderOpen} onclick={() => void win.showInFolder(only.id)}
        >Show in folder</Button
      >
      <Button variant="primary" data-autofocus onclick={() => void win.open(only.id)}>Open</Button>
    {:else if only?.status === 'failed' && retryHelps(only)}
      <Button variant="ghost" onclick={() => win.close()}>Close</Button>
      <Button variant="primary" data-autofocus onclick={() => void win.retry(only.id)}
        >Try again</Button
      >
    {:else}
      <Button data-autofocus onclick={() => win.close()}>Close</Button>
    {/if}
  {/snippet}
</WindowFrame>
