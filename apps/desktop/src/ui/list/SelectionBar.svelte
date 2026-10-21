<script lang="ts">
  // What the selection can do, floating over the foot of the list while anything is selected: pause or resume it, move it to another queue, remove it, or let it go. It rises in, and its count pops each time it changes. Every action here is also in the list's context menu and keys (D3).
  import ArrowRightLeft from '~icons/lucide/arrow-right-left';
  import Pause from '~icons/lucide/pause';
  import Play from '~icons/lucide/play';
  import RotateCw from '~icons/lucide/rotate-cw';
  import Trash from '~icons/lucide/trash-2';
  import X from '~icons/lucide/x';
  import { Button, IconButton, Menu, shift, type MenuEntry } from '@wirecatch/ui';
  import { getApp } from '$lib/state/app.svelte';
  import { applicable } from '$lib/state/row-actions';

  const app = getApp();

  const rows = $derived(app.selection.rows);
  const ids = $derived(rows.map((r) => r.id));
  const can = $derived(applicable(rows));
  const queues = $derived<MenuEntry[]>(
    app.queues.list.map((q) => ({
      id: `queue-${String(q.id)}`,
      label: q.name,
      onselect: () => void app.actions.moveToQueue(app.selection.list, q.id),
    })),
  );
</script>

{#if rows.length > 0 && !app.detailsOpen}
  <div
    role="toolbar"
    aria-label="Selected downloads"
    class="absolute inset-x-0 bottom-4 z-raised mx-auto flex w-fit items-center gap-1 rounded-lg border border-line-strong bg-overlay py-1.5 pr-1.5 pl-2 whitespace-nowrap shadow-flyout"
    transition:shift={{ from: 'below' }}
  >
    {#key rows.length}
      <span
        class="mr-1.5 inline-grid h-5.5 min-w-5.5 animate-pop-in place-items-center rounded-full bg-accent px-1.5 text-small font-bold text-accent-fg tabular-nums"
        >{rows.length}</span
      >
    {/key}
    <span class="mr-2.5 text-ui font-medium text-ink">selected</span>
    {#if can.pause}
      <Button size="sm" variant="ghost" icon={Pause} onclick={() => app.actions.pause(ids)}
        >Pause</Button
      >
    {/if}
    {#if can.resume}
      <Button size="sm" variant="ghost" icon={Play} onclick={() => app.actions.resume(ids)}
        >Resume</Button
      >
    {/if}
    {#if can.retry}
      <Button size="sm" variant="ghost" icon={RotateCw} onclick={() => app.actions.retry(ids)}
        >Try again</Button
      >
    {/if}
    {#if app.queues.list.length > 1}
      <Menu entries={queues} label="Move to queue" align="center">
        {#snippet trigger(props)}
          <Button {...props} size="sm" variant="ghost" icon={ArrowRightLeft}>Move to queue</Button>
        {/snippet}
      </Menu>
    {/if}
    <Button
      size="sm"
      variant="ghost"
      icon={Trash}
      class="text-danger"
      onclick={() => app.openDialog({ kind: 'remove', ids })}>Remove</Button
    >
    <span class="mx-1 h-4.5 w-px bg-line-strong" aria-hidden="true"></span>
    <IconButton
      icon={X}
      label="Clear the selection"
      size="sm"
      onclick={() => app.selection.clear()}
    />
  </div>
{/if}
