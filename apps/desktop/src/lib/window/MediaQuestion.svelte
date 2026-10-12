<script lang="ts">
  // A video's qualities, best first and preselected, with what to save it as (D24), its file
  // name, folder and queue, then Download now, Download later or Cancel. Enter is Download now.
  // A quality whose video and audio must be joined while the media add-on is missing installs the
  // add-on first. Save as, audio quality, subtitles and chapters start from the last choices for
  // videos or for audio only, and every choice says before it starts whether it converts.
  import type { MediaChoice, MediaOption, QueueId } from '@wirecatch/protocol';
  import { isInstalled } from '$lib/addons';
  import Button from '$lib/components/Button.svelte';
  import { earlierNote, hostOf } from '$lib/format';
  import { formatBytes } from '@wirecatch/units';
  import {
    fitFor,
    FORMAT_LABEL,
    kbpsFor,
    outputSelection,
    rowTag,
    sizeFor,
    startingChoice,
  } from '$lib/media-output';
  import type { DownloadWindowState } from './download-window.svelte';
  import { enterSubmits } from './keys';
  import { OutputChoiceState } from './output-choice.svelte';
  import SaveAsOptions from './SaveAsOptions.svelte';
  import WindowFrame from './WindowFrame.svelte';
  import WindowOptions from './WindowOptions.svelte';

  let { win, choice }: { win: DownloadWindowState; choice: MediaChoice } = $props();
  const uid = $props.id();

  // The form starts from the proposal and the last choices, and edits its own copy.
  const proposal = (() => ({ ...choice }))();
  const defaultsFor = (option: MediaOption | undefined) =>
    win.settings?.media[option?.audio_only ? 'audio' : 'video'] ?? null;
  const first = proposal.options[0];
  const start = startingChoice(first, proposal, defaultsFor(first));

  let optionId = $state(first?.id ?? '');
  let fileName = $state('');
  let folder = $state(proposal.folder);
  let queue = $state<QueueId>(proposal.queue);
  const output = new OutputChoiceState(start);

  const picked = $derived(choice.options.find((o) => o.id === optionId));
  const needsInstall = $derived(
    picked?.needs_ffmpeg === true && !isInstalled(win.addon ?? undefined),
  );
  const joinNote = $derived.by(() => {
    if (!picked?.needs_ffmpeg) return null;
    if (!needsInstall) {
      return output.format === 'original'
        ? 'This quality comes as separate video and audio. Wirecatch joins them into one file when the download finishes.'
        : null;
    }
    const size = win.addon?.size ? ` (${formatBytes(win.addon.size)})` : '';
    return `This quality comes as separate video and audio. Wirecatch first installs the media add-on${size} to join them into one file, then downloads it.`;
  });

  /** A row's words: as today for Original, else the quality in the chosen format. */
  function rowLabel(option: MediaOption): string {
    const format = output.format;
    if (format === 'original' || !fitFor(option, format)) return option.label;
    return `${option.quality || option.label}, ${FORMAT_LABEL[format]}`;
  }

  function rowSize(option: MediaOption): number | null {
    const shown = fitFor(option, output.format) ? output.format : 'original';
    const kbps = option.id === optionId ? output.kbps : null;
    return sizeFor(option, shown, kbps, choice.duration_ms);
  }

  /** Another quality: switching between video and audio only starts from that kind's choices. */
  function selectOption(id: string) {
    const before = picked;
    optionId = id;
    const now = picked;
    if (!now || now === before) return;
    if (before?.audio_only !== now.audio_only) {
      output.reset(startingChoice(now, choice, defaultsFor(now)));
      return;
    }
    if (!fitFor(now, output.format)) output.format = 'original';
    output.kbps = kbpsFor(fitFor(now, output.format), output.format, output.kbps);
  }

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
      ...outputSelection(picked, choice, output),
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
      <p class="earlier" role="note">{earlierNote(win.earlier, win.openedAt)}</p>
    {/if}

    <fieldset class="options">
      <legend class="form-label">Quality</legend>
      {#each choice.options as option, i (option.id)}
        {@const tag = rowTag(option, output.format, option.id === optionId ? output.kbps : null)}
        {@const size = rowSize(option)}
        <label class="option" class:chosen={option.id === optionId}>
          <input
            type="radio"
            name="{uid}-quality"
            value={option.id}
            checked={option.id === optionId}
            onchange={() => selectOption(option.id)}
          />
          <span class="option-label">{rowLabel(option)}</span>
          {#if i === 0}<span class="best">Best</span>{/if}
          {#if tag === 'converts'}
            <span class="tag converts">Converts</span>
          {:else if tag === 'copied'}
            <span class="tag">No conversion</span>
          {/if}
          <span class="size num"
            >{size === null ? 'Size not known' : `About ${formatBytes(size)}`}</span
          >
        </label>
      {/each}
    </fieldset>
    {#if joinNote}
      <p class="note" role="note">{joinNote}</p>
    {/if}

    {#if picked}
      <SaveAsOptions option={picked} {choice} defaults={defaultsFor(picked)} {output} />
    {/if}

    <label class="form-stack">
      <span class="form-label">File name</span>
      <input class="field" placeholder={choice.title} bind:value={fileName} spellcheck="false" />
      <span class="form-help">Leave empty to use the video's title.</span>
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

    {#if choice.page_url}
      <p class="link" title={choice.page_url}>From {hostOf(choice.page_url)}</p>
    {/if}

    {#if win.error}<p class="error" role="alert">{win.error}</p>{/if}

    <WindowOptions {win} />
  </form>

  {#snippet footer()}
    <Button onclick={() => win.discard()}>Cancel</Button>
    <Button disabled={win.busy || !picked} onclick={() => answer(true)}>Download later</Button>
    <Button
      variant="primary"
      type="submit"
      form="{uid}-form"
      data-autofocus
      disabled={win.busy || !picked}
      >{needsInstall ? 'Install add-on and download' : 'Download now'}</Button
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
    grid-template-columns: auto 1fr auto auto auto;
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
    grid-column: 3;
    padding: 1px 6px;
    border-radius: 999px;
    background: var(--accent);
    color: var(--accent-ink);
    font-size: var(--text-xs);
    font-weight: 600;
  }

  .tag {
    grid-column: 4;
    padding: 1px 6px;
    border: 1px solid var(--line-strong);
    border-radius: 999px;
    color: var(--ink-2);
    font-size: var(--text-xs);
    white-space: nowrap;
  }

  .tag.converts {
    border-color: color-mix(in srgb, var(--warn) 55%, transparent);
    color: var(--ink);
  }

  .size {
    grid-column: 5;
    color: var(--ink-2);
    font-size: var(--text-sm);
  }

  .note {
    margin: 0;
    color: var(--ink-2);
    font-size: var(--text-sm);
    line-height: 1.5;
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
