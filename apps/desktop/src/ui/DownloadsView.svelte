<script lang="ts">
  // The downloads screen: what the list shows and its tools, then the list (with its notices, or what it says when it is empty), the bar of what the selection can do over its foot, and a download's details in a dialog over it all.
  import Unplug from '~icons/lucide/unplug';
  import { untrack } from 'svelte';
  import { Button, EmptyState, rise } from '@wirecatch/ui';
  import { getApp } from '$lib/state/app.svelte';
  import { keepToShown } from '$lib/state/selection.svelte';
  import DetailsDialog from './details/DetailsDialog.svelte';
  import DownloadList from './list/DownloadList.svelte';
  import ListEmpty from './list/ListEmpty.svelte';
  import ListHeader from './list/ListHeader.svelte';
  import ListNotices from './list/ListNotices.svelte';
  import SelectionBar from './list/SelectionBar.svelte';

  const app = getApp();
  keepToShown(app.selection, () => app.visible);

  let header = $state<HTMLElement>();
  let column = $state<HTMLElement>();
  // A new view rises in (D46), in place: the list keeps its focus and state.
  let shownView: string | null = null;
  $effect(() => {
    const view = JSON.stringify(app.view);
    untrack(() => {
      if (shownView !== null && view !== shownView) {
        for (const node of [header, column]) if (node) rise(node);
      }
      shownView = view;
    });
  });
</script>

{#if app.loadError}
  <div class="flex min-h-0 flex-1 items-center justify-center px-6" role="alert">
    <EmptyState
      icon={Unplug}
      tone="danger"
      title="{app.name} cannot reach its download engine"
      description={app.loadError}
    >
      {#snippet actions()}
        <Button variant="primary" onclick={() => app.init()}>Try again</Button>
      {/snippet}
    </EmptyState>
  </div>
{:else}
  <div class="relative flex min-h-0 flex-1 flex-col">
    <div bind:this={header}><ListHeader /></div>
    <div bind:this={column} class="relative flex min-h-0 min-w-0 flex-1 flex-col">
      <ListNotices />
      {#if app.layout.sections.length > 0}
        <DownloadList />
      {:else if app.ready}
        <ListEmpty />
      {/if}
      <SelectionBar />
    </div>
  </div>
  <DetailsDialog />
{/if}
