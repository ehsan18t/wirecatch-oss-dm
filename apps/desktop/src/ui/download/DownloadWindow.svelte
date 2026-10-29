<script lang="ts">
  // One download's window (D23, D36): its question, then its progress, in the window's shell. Escape and Close close the window, which never answers the question: it stays open in the core and the main window counts it.
  import { Button } from '@wirecatch/ui';
  import type { DownloadWindowState } from '$lib/window/download-window.svelte';
  import AddonView from './AddonView.svelte';
  import FileExistsView from './FileExistsView.svelte';
  import FileQuestion from './FileQuestion.svelte';
  import MediaQuestion from './MediaQuestion.svelte';
  import ProgressView from './ProgressView.svelte';
  import ReadingView from './ReadingView.svelte';
  import WindowFrame from './WindowFrame.svelte';
  import WindowShell from './WindowShell.svelte';

  let { win }: { win: DownloadWindowState } = $props();

  const question = $derived(win.question);
</script>

<WindowShell name={win.name} onclose={() => win.close()}>
  <!-- Each view the window moves to rises in (D46); the progress counts once its row is there. -->
  {#key `${win.phase}:${question?.kind ?? ''}:${String(win.row !== null)}`}
    <div class="h-full animate-rise-in">
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
      {:else if question?.kind === 'file_exists'}
        <FileExistsView {win} question={question.data} />
      {/if}
    </div>
  {/key}
</WindowShell>
