<script lang="ts">
  import type { MediaChoice, QueueId } from '@wirecatch/protocol';
  import { isInstalled, MEDIA_ADDON } from '$lib/addons';
  import { formatBytes, hostOf } from '$lib/format';
  import { getApp } from '$lib/state/app.svelte';
  import Dialog from './Dialog.svelte';

  let { choice, waiting }: { choice: MediaChoice; waiting: number } = $props();
  const app = getApp();
  const uid = $props.id();

  // The form starts from the proposal and edits its own copy. The best quality comes first.
  const proposal = (() => ({ ...choice }))();
  let optionId = $state(proposal.options[0]?.id ?? '');
  let fileName = $state('');
  let folder = $state(proposal.folder);
  let queue = $state<QueueId>(proposal.queue);
  let busy = $state(false);

  const picked = $derived(choice.options.find((o) => o.id === optionId));
  const addon = $derived(app.addons?.find((a) => a.id === MEDIA_ADDON));
  const joinNote = $derived.by(() => {
    if (!picked?.needs_ffmpeg) return null;
    if (isInstalled(addon)) {
      return 'This quality comes as separate video and audio. Wirecatch joins them into one file when the download finishes.';
    }
    const size = addon?.size ? ` (${formatBytes(addon.size)})` : '';
    return `This quality comes as separate video and audio. Wirecatch first installs the media add-on${size} to join them into one file.`;
  });

  async function changeFolder() {
    const picked = await app.backend.pickFolder(folder);
    if (picked) folder = picked;
  }

  async function download() {
    if (!picked) return;
    busy = true;
    await app.chooseMedia(choice.id, {
      option_id: picked.id,
      file_name: fileName.trim() ? fileName.trim() : null,
      folder: folder !== choice.folder ? folder : null,
      queue: queue !== choice.queue ? queue : null,
    });
    busy = false;
  }
</script>

<Dialog
  title="Pick a quality"
  description={waiting > 0
    ? `${choice.title}. ${waiting} more ${waiting === 1 ? 'video is' : 'videos are'} waiting after this one.`
    : choice.title}
  onclose={() => app.dismissMediaChoice(choice.id)}
  width={540}
>
  <form
    id="media-form"
    onsubmit={(e) => {
      e.preventDefault();
      void download();
    }}
  >
    <fieldset class="options">
      <legend class="label">Quality</legend>
      {#each choice.options as option, i (option.id)}
        <label class="option" class:chosen={option.id === optionId}>
          <input
            type="radio"
            name="{uid}-quality"
            value={option.id}
            bind:group={optionId}
            data-autofocus={i === 0 ? '' : undefined}
          />
          <span class="option-label">{option.label}</span>
          {#if i === 0}<span class="best">Best</span>{/if}
          <span class="size num"
            >{option.size === null ? 'Size not known' : `About ${formatBytes(option.size)}`}</span
          >
        </label>
      {/each}
    </fieldset>
    {#if joinNote}
      <p class="note" role="note">{joinNote}</p>
    {/if}

    <label class="stack">
      <span class="label">File name</span>
      <input class="field" placeholder={choice.title} bind:value={fileName} />
      <span class="help">Leave empty to use the video's title.</span>
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

    {#if choice.page_url}
      <p class="link" title={choice.page_url}>From {hostOf(choice.page_url)}</p>
    {/if}
  </form>

  {#snippet footer()}
    <button type="button" class="btn" onclick={() => app.dismissMediaChoice(choice.id)}
      >Don't download</button
    >
    <button type="submit" form="media-form" class="btn primary" disabled={busy || !picked}
      >Download</button
    >
  {/snippet}
</Dialog>

<style>
  form {
    display: grid;
    gap: 14px;
  }

  .options {
    display: grid;
    gap: 4px;
    margin: 0;
    padding: 0;
    border: 0;
  }

  .options legend {
    margin-bottom: 4px;
    padding: 0;
  }

  .option {
    display: grid;
    grid-template-columns: auto 1fr auto auto;
    align-items: center;
    gap: 10px;
    padding: 8px 10px;
    border: 1px solid var(--line);
    border-radius: var(--radius-m);
    cursor: pointer;
  }

  .option.chosen {
    border-color: var(--accent);
    background: var(--accent-soft);
  }

  .option input {
    margin: 0;
    accent-color: var(--accent);
  }

  .best {
    padding: 1px 6px;
    border-radius: 999px;
    background: var(--accent);
    color: var(--accent-ink);
    font-size: var(--text-xs);
    font-weight: 600;
  }

  .size {
    grid-column: 4;
    color: var(--ink-2);
    font-size: var(--text-sm);
  }

  .note {
    margin: 0;
    color: var(--ink-2);
    font-size: var(--text-sm);
    line-height: 1.5;
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

  .help {
    color: var(--ink-2);
    font-size: var(--text-xs);
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
</style>
