<script lang="ts">
  // The downloads screen: what the list shows and its tools, then the list (with its notices, or what it says when it is empty) and the details of the selection. In wide content the details are a column beside the list, under its tools, so neither covers the other; in narrow content they open over the list with a veil.
  import Unplug from '~icons/lucide/unplug';
  import { untrack } from 'svelte';
  import { Button, EmptyState, rise } from '@wirecatch/ui';
  import { getApp } from '$lib/state/app.svelte';
  import DetailsPanel from './details/DetailsPanel.svelte';
  import DownloadList from './list/DownloadList.svelte';
  import ListEmpty from './list/ListEmpty.svelte';
  import ListHeader from './list/ListHeader.svelte';
  import ListNotices from './list/ListNotices.svelte';

  const app = getApp();

  let width = $state(0);
  /** `--width-details-beside` in pixels, measured from the page so it follows the text size. */
  let beside = $state(0);
  // Where nothing is laid out (unit tests) both are 0, and the details sit beside the list.
  const docked = $derived(width >= beside);
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
  <div class="relative flex min-h-0 flex-1 flex-col" bind:clientWidth={width}>
    <div
      class="pointer-events-none invisible absolute h-0 w-details-beside"
      aria-hidden="true"
      bind:clientWidth={beside}
    ></div>
    <div bind:this={header}><ListHeader /></div>
    <div class="flex min-h-0 flex-1">
      <div bind:this={column} class="flex min-w-0 flex-1 flex-col">
        <ListNotices />
        {#if app.visible.length > 0}
          <DownloadList />
        {:else if app.ready}
          <ListEmpty />
        {/if}
      </div>
      <DetailsPanel {docked} />
    </div>
  </div>
{/if}
