<script lang="ts">
  // One download's window (D23): its question, then its progress. Escape closes the window,
  // which never answers the question: it stays open in the core and the main window counts it.
  import Button from '$lib/components/Button.svelte';
  import type { DownloadWindowState } from './download-window.svelte';
  import AddonView from './AddonView.svelte';
  import FileQuestion from './FileQuestion.svelte';
  import MediaQuestion from './MediaQuestion.svelte';
  import ProgressView from './ProgressView.svelte';
  import ReadingView from './ReadingView.svelte';
  import WindowFrame from './WindowFrame.svelte';

  let { win }: { win: DownloadWindowState } = $props();

  const question = $derived(win.question);

  function onkeydown(e: KeyboardEvent) {
    if (e.key !== 'Escape' || e.defaultPrevented) return;
    e.preventDefault();
    win.close();
  }
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div class="download-window" {onkeydown}>
  {#if win.phase === 'loading'}
    <p class="loading" role="status">Loading.</p>
  {:else if win.phase === 'gone'}
    <WindowFrame title="Nothing to do here">
      <p class="gone" role="status">{win.goneReason}</p>
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

<style>
  .download-window {
    height: 100%;
    background: var(--surface);
  }

  .loading {
    margin: 0;
    padding: 20px;
    color: var(--ink-2);
  }

  .gone {
    margin: 0;
    line-height: 1.5;
  }
</style>
