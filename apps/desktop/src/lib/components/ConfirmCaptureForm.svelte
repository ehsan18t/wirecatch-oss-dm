<script lang="ts">
  import type { PendingCapture, QueueId } from '@wirecatch/protocol';
  import { formatBytes, hostOf } from '$lib/format';
  import { getApp } from '$lib/state/app.svelte';
  import CategoryIcon from './CategoryIcon.svelte';
  import Dialog from './Dialog.svelte';

  let { capture, waiting }: { capture: PendingCapture; waiting: number } = $props();
  const app = getApp();

  // The form starts from the proposal and edits its own copy.
  const proposal = (() => ({ ...capture }))();
  let fileName = $state(proposal.file_name);
  let folder = $state(proposal.folder);
  let queue = $state<QueueId>(proposal.queue);
  let busy = $state(false);

  async function changeFolder() {
    const picked = await app.backend.pickFolder(folder);
    if (picked) folder = picked;
  }

  async function confirm() {
    busy = true;
    await app.confirmCapture(capture.id, {
      file_name: fileName.trim() && fileName.trim() !== capture.file_name ? fileName.trim() : null,
      folder: folder !== capture.folder ? folder : null,
      queue: queue !== capture.queue ? queue : null,
    });
    busy = false;
  }
</script>

<Dialog
  title="Download this file?"
  description={waiting > 0
    ? `From your browser. ${waiting} more ${waiting === 1 ? 'is' : 'are'} waiting after this one.`
    : 'From your browser. Change anything you like, then download.'}
  onclose={() => (app.pendingHidden = true)}
  width={520}
>
  <form
    id="confirm-form"
    onsubmit={(e) => {
      e.preventDefault();
      void confirm();
    }}
  >
    <div class="file">
      <span class="icon"><CategoryIcon category={capture.category} size={20} /></span>
      <div class="facts">
        <span class="num"
          >{capture.size === null ? 'Size not known yet' : formatBytes(capture.size)}</span
        >
        <span>from {hostOf(capture.page_url ?? capture.url)}</span>
      </div>
    </div>

    <label class="stack">
      <span class="label">File name</span>
      <input class="field" bind:value={fileName} />
    </label>

    <div class="row">
      <div class="stack grow">
        <span class="label">Save to</span>
        <p class="folder">{folder}</p>
      </div>
      <button type="button" class="btn small" onclick={changeFolder}>Change</button>
    </div>

    <label class="stack">
      <span class="label">Queue</span>
      <select class="field" bind:value={queue}>
        {#each app.queues as q (q.id)}
          <option value={q.id}>{q.name}</option>
        {/each}
      </select>
    </label>

    <p class="link" title={capture.url}>{capture.url}</p>
  </form>

  {#snippet footer()}
    <button type="button" class="btn ghost ask" onclick={() => app.setAskBeforeDownload(false)}>
      Stop asking, start right away
    </button>
    <button type="button" class="btn" onclick={() => app.rejectCapture(capture.id)}
      >Don't download</button
    >
    <button type="submit" form="confirm-form" class="btn primary" data-autofocus disabled={busy}
      >Download</button
    >
  {/snippet}
</Dialog>

<style>
  form {
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
    font-size: var(--text-sm);
  }

  .stack {
    display: grid;
    gap: 4px;
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

  .label {
    font-size: var(--text-sm);
    font-weight: 600;
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

  .ask {
    margin-right: auto;
  }
</style>
