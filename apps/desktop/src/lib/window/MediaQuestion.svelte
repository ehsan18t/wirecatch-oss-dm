<script lang="ts">
  // A video's qualities, best first and preselected, with its file name, folder and queue, then
  // Download now, Download later or Cancel. Enter is Download now. A quality whose video and
  // audio must be joined while the media add-on is missing installs the add-on first.
  import type { MediaChoice, QueueId } from '@wirecatch/protocol';
  import { isInstalled } from '$lib/addons';
  import { earlierNote, formatBytes, hostOf } from '$lib/format';
  import type { DownloadWindowState } from './download-window.svelte';
  import { enterSubmits } from './keys';
  import WindowFrame from './WindowFrame.svelte';
  import WindowOptions from './WindowOptions.svelte';

  let { win, choice }: { win: DownloadWindowState; choice: MediaChoice } = $props();
  const uid = $props.id();

  // The form starts from the proposal and edits its own copy.
  const proposal = (() => ({ ...choice }))();
  let optionId = $state(proposal.options[0]?.id ?? '');
  let fileName = $state('');
  let folder = $state(proposal.folder);
  let queue = $state<QueueId>(proposal.queue);

  const picked = $derived(choice.options.find((o) => o.id === optionId));
  const needsInstall = $derived(
    picked?.needs_ffmpeg === true && !isInstalled(win.addon ?? undefined),
  );
  const joinNote = $derived.by(() => {
    if (!picked?.needs_ffmpeg) return null;
    if (!needsInstall) {
      return 'This quality comes as separate video and audio. Wirecatch joins them into one file when the download finishes.';
    }
    const size = win.addon?.size ? ` (${formatBytes(win.addon.size)})` : '';
    return `This quality comes as separate video and audio. Wirecatch first installs the media add-on${size} to join them into one file, then downloads it.`;
  });

  async function changeFolder() {
    const chosen = await win.changeFolder(folder);
    if (chosen) folder = chosen;
  }

  function answer(later: boolean) {
    if (!picked) return;
    void win.choose({
      option_id: picked.id,
      file_name: fileName.trim() ? fileName.trim() : null,
      folder: folder !== choice.folder ? folder : null,
      queue: queue !== choice.queue ? queue : null,
      later,
    });
  }
</script>

<WindowFrame title="Pick a quality" description={choice.title}>
  <!-- Enter in any field below is Download now (see keys.ts); the keys come from the fields. -->
  <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
  <form
    id="{uid}-form"
    class="form"
    onkeydown={(e) => enterSubmits(e, () => answer(false))}
    onsubmit={(e) => {
      e.preventDefault();
      answer(false);
    }}
  >
    {#if win.earlier}
      <p class="earlier" role="note">{earlierNote(win.earlier, win.now)}</p>
    {/if}

    <fieldset class="options">
      <legend class="label">Quality</legend>
      {#each choice.options as option, i (option.id)}
        <label class="option" class:chosen={option.id === optionId}>
          <input type="radio" name="{uid}-quality" value={option.id} bind:group={optionId} />
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
      <input class="field" placeholder={choice.title} bind:value={fileName} spellcheck="false" />
      <span class="help">Leave empty to use the video's title.</span>
    </label>

    <div class="row">
      <div class="stack grow">
        <span class="label">Save to</span>
        <p class="folder" id="{uid}-folder">{folder}</p>
      </div>
      <button
        type="button"
        class="btn small"
        aria-label="Change the folder"
        aria-describedby="{uid}-folder"
        onclick={changeFolder}>Change</button
      >
    </div>

    <label class="stack">
      <span class="label">Queue</span>
      <select class="field" bind:value={queue}>
        {#each win.queues as q (q.id)}
          <option value={q.id}>{q.name}</option>
        {/each}
      </select>
    </label>

    {#if choice.page_url}
      <p class="link" title={choice.page_url}>From {hostOf(choice.page_url)}</p>
    {/if}

    {#if win.error}<p class="error" role="alert">{win.error}</p>{/if}

    <WindowOptions {win} />
  </form>

  {#snippet footer()}
    <button type="button" class="btn" onclick={() => win.discard()}>Cancel</button>
    <button type="button" class="btn" disabled={win.busy || !picked} onclick={() => answer(true)}
      >Download later</button
    >
    <button
      type="submit"
      form="{uid}-form"
      class="btn primary"
      data-autofocus
      disabled={win.busy || !picked}
      >{needsInstall ? 'Install add-on and download' : 'Download now'}</button
    >
  {/snippet}
</WindowFrame>

<style>
  .form {
    display: grid;
    gap: 14px;
  }

  .earlier {
    margin: 0;
    padding: 8px 10px;
    border-left: 3px solid var(--accent);
    border-radius: var(--radius-s);
    background: var(--surface-2);
    font-size: var(--text-sm);
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

  .error {
    margin: 0;
    color: var(--danger);
    font-size: var(--text-sm);
  }
</style>
