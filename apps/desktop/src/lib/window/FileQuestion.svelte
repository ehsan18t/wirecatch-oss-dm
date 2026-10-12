<script lang="ts">
  // A browser file waiting for an answer: its name (editable), size, folder, queue and site,
  // then Download now, Download later or Cancel. Enter is Download now.
  import type { PendingCapture, QueueId } from '@wirecatch/protocol';
  import Button from '$lib/components/Button.svelte';
  import CategoryIcon from '$lib/components/CategoryIcon.svelte';
  import { earlierNote, hostOf, linkForDisplay } from '$lib/format';
  import { formatBytes } from '@wirecatch/units';
  import type { DownloadWindowState } from './download-window.svelte';
  import { enterSubmits } from './keys';
  import WindowFrame from './WindowFrame.svelte';
  import WindowOptions from './WindowOptions.svelte';

  let { win, capture }: { win: DownloadWindowState; capture: PendingCapture } = $props();
  const uid = $props.id();

  // The form starts from the proposal and edits its own copy.
  const proposal = (() => ({ ...capture }))();
  let fileName = $state(proposal.file_name);
  let folder = $state(proposal.folder);
  let queue = $state<QueueId>(proposal.queue);

  async function changeFolder() {
    const picked = await win.changeFolder(folder);
    if (picked) folder = picked;
  }

  function answer(later: boolean) {
    void win.confirm({ fileName, folder, queue }, later);
  }
</script>

<WindowFrame title="Download this file?" description="From your browser. Change anything you like.">
  <!-- svelte-ignore a11y_no_noninteractive_element_interactions (Enter in any field, radio buttons and checkboxes included, is Download now: see keys.ts) -->
  <form
    id="{uid}-form"
    class="form"
    onkeydown={(e) => enterSubmits(e, () => answer(false))}
    onsubmit={(e) => {
      e.preventDefault();
      answer(false);
    }}
  >
    <div class="file">
      <span class="icon"><CategoryIcon category={capture.category} size="lg" /></span>
      <p class="facts">
        <span class="num"
          >{capture.size === null ? 'Size not known yet' : formatBytes(capture.size)}</span
        >
        <span>from {hostOf(capture.page_url ?? capture.url)}</span>
      </p>
    </div>

    {#if win.earlier}
      <p class="earlier" role="note">{earlierNote(win.earlier, win.clock.now)}</p>
    {/if}

    <label class="form-stack">
      <span class="form-label">File name</span>
      <input class="field" bind:value={fileName} spellcheck="false" />
    </label>

    <div class="row">
      <div class="form-stack grow">
        <span class="form-label">Save to</span>
        <p class="folder" id="{uid}-folder">{folder}</p>
      </div>
      <Button
        small
        aria-label="Change the folder"
        aria-describedby="{uid}-folder"
        onclick={changeFolder}>Change</Button
      >
    </div>

    <label class="form-stack">
      <span class="form-label">Queue</span>
      <select class="field" bind:value={queue}>
        {#each win.queues as q (q.id)}
          <option value={q.id}>{q.name}</option>
        {/each}
      </select>
    </label>

    <p class="link" title={linkForDisplay(capture.url)}>{linkForDisplay(capture.url)}</p>

    {#if win.error}<p class="error" role="alert">{win.error}</p>{/if}

    <WindowOptions {win} files />
  </form>

  {#snippet footer()}
    <Button onclick={() => win.discard()}>Cancel</Button>
    <Button disabled={win.busy} onclick={() => answer(true)}>Download later</Button>
    <Button variant="primary" type="submit" form="{uid}-form" data-autofocus disabled={win.busy}
      >Download now</Button
    >
  {/snippet}
</WindowFrame>

<style>
  .form {
    display: grid;
    gap: 14px;
  }

  .file {
    display: flex;
    align-items: center;
    gap: 10px;
    color: var(--ink-2);
  }

  .facts {
    display: flex;
    flex-wrap: wrap;
    gap: 2px 12px;
    margin: 0;
    font-size: var(--text-sm);
  }

  .earlier {
    margin: 0;
    padding: 8px 10px;
    border-left: 3px solid var(--accent);
    border-radius: var(--radius-s);
    background: var(--surface-2);
    font-size: var(--text-sm);
  }

  .grow {
    flex: 1;
    min-width: 0;
  }

  .row {
    display: flex;
    align-items: flex-end;
    gap: 10px;
  }

  .folder {
    margin: 0;
    color: var(--ink-2);
    font-size: var(--text-sm);
    overflow-wrap: anywhere;
  }

  .link {
    margin: 0;
    overflow: hidden;
    color: var(--ink-3);
    font-size: var(--text-xs);
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .error {
    margin: 0;
    color: var(--danger);
    font-size: var(--text-sm);
  }
</style>
