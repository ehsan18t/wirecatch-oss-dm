<script lang="ts">
  // One download's window (D23, D36): the frameless title bar with its close button, over the window's material, then its question, then its progress, on the content layer. Escape and Close close the window, which never answers the question: it stays open in the core and the main window counts it.
  import { AppMark, Button, TitleBar } from '@wirecatch/ui';
  import type { DownloadWindowState } from '$lib/window/download-window.svelte';
  import { getFitWindow, setFitWindow } from '$lib/window/fit';
  import { closeOnlyControls } from '$lib/window/window-controls';
  import AddonView from './AddonView.svelte';
  import FileQuestion from './FileQuestion.svelte';
  import MediaQuestion from './MediaQuestion.svelte';
  import ProgressView from './ProgressView.svelte';
  import ReadingView from './ReadingView.svelte';
  import WindowFrame from './WindowFrame.svelte';

  let { win }: { win: DownloadWindowState } = $props();

  const question = $derived(win.question);
  const controls = closeOnlyControls(() => win.close());

  let root = $state<HTMLDivElement>();
  let barBox = $state<readonly ResizeObserverSize[]>();

  // The frame below measures what it needs; the window needs the title bar on top of that, and asks again when the bar alone changes height (a larger text size).
  const fitWindow = getFitWindow();
  let frameNeeds = $state(0);
  if (fitWindow) setFitWindow((height) => (frameNeeds = height));
  $effect(() => {
    if (fitWindow && frameNeeds > 0)
      fitWindow(Math.ceil(frameNeeds + (barBox?.[0]?.blockSize ?? 0)));
  });

  // Escape closes the window. The sample page shows several windows in one page, so only keys from inside this one count; the window's own controls are what take them.
  function onkeydown(event: KeyboardEvent) {
    if (event.key !== 'Escape' || event.defaultPrevented) return;
    if (!(event.target instanceof Node) || !root?.contains(event.target)) return;
    event.preventDefault();
    win.close();
  }
</script>

<svelte:window {onkeydown} />

<div bind:this={root} class="flex h-full flex-col">
  <div bind:borderBoxSize={barBox} class="shrink-0 border-b border-line">
    <TitleBar title={win.name} {controls} closeOnly>
      {#snippet icon()}
        <AppMark />
      {/snippet}
    </TitleBar>
  </div>
  <div class="min-h-0 flex-1 bg-layer">
    {#if win.phase === 'loading'}
      <p class="p-5 text-body text-ink-muted" role="status">Loading.</p>
    {:else if win.phase === 'gone'}
      <WindowFrame title="Nothing to do here">
        <p class="text-body text-ink" role="status">{win.goneReason}</p>
        {#snippet footer()}
          <Button variant="primary" data-autofocus onclick={() => win.close()}>Close</Button>
        {/snippet}
      </WindowFrame>
    {:else if win.phase === 'progress' && win.row}
      <ProgressView {win} row={win.row} />
    {:else if question?.kind === 'capture'}
      {#key question.data.id}
        <FileQuestion {win} capture={question.data} />
      {/key}
    {:else if question?.kind === 'reading'}
      <ReadingView {win} url={question.data.url} />
    {:else if question?.kind === 'choice' && win.awaitingAddon && win.addon}
      <AddonView {win} addon={win.addon} url={question.data.page_url ?? ''} forQuality />
    {:else if question?.kind === 'choice'}
      {#key question.data.id}
        <MediaQuestion {win} choice={question.data} />
      {/key}
    {:else if question?.kind === 'addon'}
      <AddonView {win} addon={win.addon ?? question.data.addon} url={question.data.url} />
    {/if}
  </div>
</div>
