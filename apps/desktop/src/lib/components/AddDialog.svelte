<script lang="ts">
  import type { QueueId } from '@wirecatch/protocol';
  import { extractUrls, fileNameFromUrl } from '$lib/format';
  import { getApp } from '$lib/state/app.svelte';
  import Button from './Button.svelte';
  import Dialog from './Dialog.svelte';

  let { urls: initial, queue: initialQueue }: { urls: string[]; queue?: QueueId } = $props();

  const app = getApp();

  /** Rows the links box has for one link or none: room to paste or type a few by hand. */
  const LINK_ROWS = 3;
  /** The links box grows with the links up to this many rows, then scrolls. */
  const LINK_ROWS_MAX = 8;

  // The dialog opens with whatever was pasted; later edits are the person's own.
  const start = (() => ({ text: initial.join('\n'), queue: initialQueue }))();
  let text = $state(start.text);
  let fileName = $state('');
  let folder = $state<string | null>(null);
  // No queue (none listed yet) lets the core put the downloads in its default queue.
  let queue = $state<QueueId | null>(
    start.queue ??
      (app.view.kind === 'queue' ? app.view.id : (app.queues.defaultQueue?.id ?? null)),
  );
  let error = $state('');
  let busy = $state(false);
  const fromPaste = start.text.length > 0;

  const urls = $derived(extractUrls(text));
  const single = $derived(urls.length === 1 ? urls[0] : undefined);

  async function chooseFolder() {
    const picked = await app.backend.pickFolder(folder ?? undefined);
    if (picked) folder = picked;
  }

  async function submit(startNow: boolean) {
    if (urls.length === 0) {
      error = 'Paste at least one link that starts with https://, http:// or ftp://.';
      return;
    }
    busy = true;
    const ids = await app.add(
      urls.map((url) => ({
        url,
        queue,
        folder,
        file_name: single && fileName.trim() ? fileName.trim() : null,
        start: startNow,
      })),
    );
    busy = false;
    if (ids.length > 0) {
      app.closeDialog();
      const first = ids[0];
      if (first !== undefined) app.selection.only(first);
    }
  }
</script>

<Dialog
  title={urls.length > 1 ? `Add ${urls.length} downloads` : 'Add a download'}
  onclose={() => app.closeDialog()}
  size="wide"
>
  <form
    id="add-form"
    onsubmit={(e) => {
      e.preventDefault();
      void submit(true);
    }}
  >
    <div class="form-stack">
      <label class="form-label" for="add-links">Links</label>
      <textarea
        id="add-links"
        class="field"
        rows={urls.length > 1 ? Math.min(LINK_ROWS_MAX, urls.length + 1) : LINK_ROWS}
        placeholder="https://example.com/file.zip"
        aria-describedby="add-links-help{error ? ' add-error' : ''}"
        aria-invalid={error ? 'true' : undefined}
        bind:value={text}
        oninput={() => (error = '')}
        onkeydown={(e) => {
          if (e.key === 'Enter' && (e.ctrlKey || e.metaKey)) {
            e.preventDefault();
            void submit(true);
          }
        }}></textarea>
      <span id="add-links-help" class="form-help"
        >One link per line. Ctrl+Enter downloads them all.</span
      >
      {#if error}<span id="add-error" class="error" role="alert">{error}</span>{/if}
    </div>

    {#if single}
      <div class="form-stack">
        <label class="form-label" for="add-name">File name</label>
        <input
          id="add-name"
          class="field"
          placeholder={fileNameFromUrl(single)}
          aria-describedby="add-name-help"
          bind:value={fileName}
        />
        <span id="add-name-help" class="form-help"
          >Leave empty to use the name the server gives.</span
        >
      </div>
    {/if}

    <div class="row">
      <div class="form-stack grow">
        <span class="form-label">Save to</span>
        <p class="folder">
          {folder ?? 'A folder picked by file type, as set in Settings'}
        </p>
      </div>
      <div class="folder-actions">
        <Button small onclick={chooseFolder}>Change</Button>
        {#if folder}
          <Button variant="ghost" small onclick={() => (folder = null)}>Use default</Button>
        {/if}
      </div>
    </div>

    <label class="form-stack">
      <span class="form-label">Queue</span>
      <select class="field" bind:value={queue}>
        {#each app.queues.list as q (q.id)}
          <option value={q.id}>{q.name}{q.is_default ? ' (default)' : ''}</option>
        {/each}
      </select>
    </label>
  </form>

  {#snippet footer()}
    <Button disabled={busy} onclick={() => submit(false)}>Add paused</Button>
    <Button
      variant="primary"
      type="submit"
      form="add-form"
      disabled={busy}
      data-autofocus={fromPaste ? '' : undefined}
    >
      {urls.length > 1 ? `Download ${urls.length}` : 'Download'}
    </Button>
  {/snippet}
</Dialog>

<style>
  form {
    display: grid;
    gap: 14px;
  }

  .grow {
    flex: 1;
    min-width: 0;
  }

  .error {
    color: var(--danger);
    font-size: var(--text-sm);
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

  .folder-actions {
    display: flex;
    gap: 4px;
  }
</style>
