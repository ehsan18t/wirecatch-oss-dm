<script lang="ts">
  // A video's qualities, best first and preselected, with what to save it as (D24), its file
  // name, folder and queue, then Download now, Download later or Cancel. Enter is Download now.
  // A quality whose video and audio must be joined while the media add-on is missing installs the
  // add-on first. Save as, audio quality, subtitles and chapters start from the last choices for
  // videos or for audio only, and every choice says before it starts whether it converts.
  import type { MediaChoice, MediaOption, OutputFormat, QueueId } from '@wirecatch/protocol';
  import { isInstalled } from '$lib/addons';
  import { earlierNote, hostOf } from '$lib/format';
  import { formatBytes } from '@wirecatch/units';
  import {
    conversionNote,
    fitFor,
    FORMAT_LABEL,
    formatChoiceLabel,
    KBPS_CHOICES,
    kbpsFor,
    outputSelection,
    qualityNotes,
    rowTag,
    sizeFor,
    startingChoice,
  } from '$lib/media-output';
  import type { DownloadWindowState } from './download-window.svelte';
  import { enterSubmits } from './keys';
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
  let format = $state<OutputFormat>(start.format);
  let kbps = $state<number | null>(start.kbps);
  let subtitles = $state<string[]>(start.subtitles);
  let subtitleFiles = $state(start.subtitleFiles);
  let chapters = $state(start.chapters);

  const picked = $derived(choice.options.find((o) => o.id === optionId));
  const fit = $derived(fitFor(picked, format));
  const kbpsChoices = $derived(fit ? (KBPS_CHOICES[format] ?? null) : null);
  /** Save as, subtitles or chapters to show for this quality and video. */
  const offersOutput = $derived(
    (picked?.outputs.length ?? 0) > 1 || choice.subtitles.length > 0 || choice.chapters,
  );
  const note = $derived(conversionNote(fit, format, kbps));
  const notes = $derived(qualityNotes(fit, format, kbps));
  const canChapters = $derived(choice.chapters && (fit?.chapters ?? false));

  const needsInstall = $derived(
    picked?.needs_ffmpeg === true && !isInstalled(win.addon ?? undefined),
  );
  const joinNote = $derived.by(() => {
    if (!picked?.needs_ffmpeg) return null;
    if (!needsInstall) {
      return format === 'original'
        ? 'This quality comes as separate video and audio. Wirecatch joins them into one file when the download finishes.'
        : null;
    }
    const size = win.addon?.size ? ` (${formatBytes(win.addon.size)})` : '';
    return `This quality comes as separate video and audio. Wirecatch first installs the media add-on${size} to join them into one file, then downloads it.`;
  });

  /** A row's words: as today for Original, else the quality in the chosen format. */
  function rowLabel(option: MediaOption): string {
    if (format === 'original' || !fitFor(option, format)) return option.label;
    return `${option.quality || option.label}, ${FORMAT_LABEL[format]}`;
  }

  function rowSize(option: MediaOption): number | null {
    const shown = fitFor(option, format) ? format : 'original';
    return sizeFor(option, shown, option.id === optionId ? kbps : null, choice.duration_ms);
  }

  /** Another quality: switching between video and audio only starts from that kind's choices. */
  function selectOption(id: string) {
    const before = picked;
    optionId = id;
    const now = picked;
    if (!now || now === before) return;
    if (before?.audio_only !== now.audio_only) {
      const next = startingChoice(now, choice, defaultsFor(now));
      format = next.format;
      kbps = next.kbps;
      subtitles = next.subtitles;
      chapters = next.chapters;
      subtitleFiles = next.subtitleFiles;
      return;
    }
    if (!fitFor(now, format)) format = 'original';
    kbps = kbpsFor(fitFor(now, format), format, kbps);
  }

  function formatChanged() {
    kbps = kbpsFor(fit, format, kbps ?? defaultsFor(picked)?.audio_kbps ?? null);
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
      ...outputSelection(picked, choice, { format, kbps, subtitles, subtitleFiles, chapters }),
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
        {@const tag = rowTag(option, format, option.id === optionId ? kbps : null)}
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

    {#if offersOutput && picked}
      <div class="output">
        {#if picked.outputs.length > 1}
          <div class="pair">
            <label class="stack">
              <span class="label">Save as</span>
              <select class="field" bind:value={format} onchange={formatChanged}>
                {#each picked.outputs as each (each.format)}
                  <option value={each.format}>{formatChoiceLabel(each)}</option>
                {/each}
              </select>
            </label>
            {#if kbpsChoices && fit}
              <label class="stack">
                <span class="label">Audio quality</span>
                <select
                  class="field"
                  value={kbps === null ? '' : String(kbps)}
                  onchange={(e) => {
                    const v = e.currentTarget.value;
                    kbps = v === '' ? null : Number(v);
                  }}
                >
                  {#if !fit.converts_audio}
                    <option value=""
                      >As downloaded{fit.audio_kbps === null
                        ? ''
                        : ` (${fit.audio_kbps} kbps)`}</option
                    >
                  {/if}
                  {#each kbpsChoices as k (k)}
                    <option value={String(k)}>{k} kbps</option>
                  {/each}
                </select>
              </label>
            {/if}
          </div>
        {/if}
        {#if note}<p class="note" role="note">{note}</p>{/if}
        {#each notes as text (text)}<p class="note" role="note">{text}</p>{/each}

        {#if choice.subtitles.length > 0}
          <fieldset class="subs">
            <legend class="label">Subtitles</legend>
            <div class="sub-list">
              {#each choice.subtitles as subtitle (subtitle.id)}
                <label class="check">
                  <input type="checkbox" value={subtitle.id} bind:group={subtitles} />
                  <span
                    >{subtitle.name}{#if subtitle.auto}<span class="auto">, auto-generated</span
                      >{/if}</span
                  >
                </label>
              {/each}
            </div>
            {#if subtitles.length > 0}
              {#if fit?.embeds_subtitles}
                <label class="check">
                  <input type="checkbox" bind:checked={subtitleFiles} />
                  <span>Save them as .srt files next to the video instead</span>
                </label>
              {:else}
                <p class="help">They are saved as .srt files next to the file.</p>
              {/if}
            {/if}
          </fieldset>
        {/if}

        {#if canChapters}
          <label class="check">
            <input type="checkbox" bind:checked={chapters} />
            <span>Add the video's chapters</span>
          </label>
        {/if}
      </div>
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

  .output {
    display: grid;
    gap: 10px;
    padding: 12px;
    border: 1px solid var(--line);
    border-radius: var(--radius-m);
    background: var(--surface-2);
  }

  .pair {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(150px, 1fr));
    gap: 10px;
  }

  .subs {
    display: grid;
    gap: 6px;
    margin: 0;
    padding: 0;
    border: 0;
  }

  .subs legend {
    margin-bottom: 4px;
    padding: 0;
  }

  .sub-list {
    display: grid;
    gap: 2px;
    max-height: 7.5rem;
    overflow-y: auto;
    padding: 4px 8px;
    border: 1px solid var(--line);
    border-radius: var(--radius-s);
    background: var(--surface);
  }

  .check {
    display: flex;
    align-items: center;
    gap: 8px;
    font-size: var(--text-sm);
    cursor: pointer;
  }

  .check input {
    margin: 0;
    accent-color: var(--accent);
  }

  .auto {
    color: var(--ink-2);
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
    margin: 0;
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
