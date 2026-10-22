<script lang="ts">
  // A video's qualities, best first and preselected, with what to save it as (D24), its file name, folder and queue, then Download now, Download later or Cancel. Enter in a field is Download now. A quality whose video and audio must be joined while the media add-on is missing installs the add-on first. Save as, audio quality, subtitles and chapters start from the last choices for videos or for audio only, and every choice says before it starts whether it converts.
  import DownloadIcon from '~icons/lucide/download';
  import { PRODUCT_NAME } from '@wirecatch/product';
  import type { MediaChoice, MediaOption, QueueId } from '@wirecatch/protocol';
  import { formatBytes } from '@wirecatch/units';
  import {
    Badge,
    Button,
    Callout,
    RadioGroup,
    type SelectOption,
    textIfCut,
    tooltip,
  } from '@wirecatch/ui';
  import { isInstalled } from '$lib/addons';
  import { earlierNote, hostOf } from '$lib/format';
  import {
    fitFor,
    FORMAT_LABEL,
    kbpsFor,
    outputSelection,
    rowTag,
    sizeFor,
    startingChoice,
  } from '$lib/media-output';
  import type { DownloadWindowState } from '$lib/window/download-window.svelte';
  import { enterSubmits } from '$lib/window/keys';
  import { OutputChoiceState } from '$lib/window/output-choice.svelte';
  import FileNameField from '../save/FileNameField.svelte';
  import FolderField from '../save/FolderField.svelte';
  import QueueField from '../save/QueueField.svelte';
  import SaveAsOptions from './SaveAsOptions.svelte';
  import WindowFrame from './WindowFrame.svelte';
  import WindowOptions from './WindowOptions.svelte';

  let { win, choice }: { win: DownloadWindowState; choice: MediaChoice } = $props();
  const uid = $props.id();
  const formId = `${uid}-form`;

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
  /** Which button sent the answer, for its spinner. */
  let sending = $state<'now' | 'later' | null>(null);

  const picked = $derived(choice.options.find((o) => o.id === optionId));
  const needsInstall = $derived(
    picked?.needs_ffmpeg === true && !isInstalled(win.addon ?? undefined),
  );
  const joinNote = $derived.by(() => {
    if (!picked?.needs_ffmpeg) return null;
    if (!needsInstall) {
      return output.format === 'original'
        ? `This quality comes as separate video and audio. ${PRODUCT_NAME} joins them into one file when the download finishes.`
        : null;
    }
    const size = win.addon?.size ? ` (${formatBytes(win.addon.size)})` : '';
    return `This quality comes as separate video and audio. ${PRODUCT_NAME} first installs the media add-on${size} to join them into one file, then downloads it.`;
  });
  const qualities = $derived<SelectOption[]>(
    choice.options.map((option) => ({ value: option.id, label: rowLabel(option) })),
  );

  /** A row's words: as the site names it for Original, else the quality in the chosen format. */
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

  async function answer(later: boolean) {
    if (!picked || win.busy) return;
    sending = later ? 'later' : 'now';
    await win.choose({
      option_id: picked.id,
      file_name: fileName.trim() ? fileName.trim() : null,
      folder: folder !== choice.folder ? folder : null,
      queue: queue !== choice.queue ? queue : null,
      later,
      ...outputSelection(picked, choice, output),
    });
    sending = null;
  }
</script>

<WindowFrame title="Pick a quality" description={choice.title} error={win.error}>
  <!-- svelte-ignore a11y_no_noninteractive_element_interactions (Enter in any field, radio buttons and checkboxes included, is Download now: see keys.ts) -->
  <form
    id={formId}
    class="flex flex-col gap-4"
    onkeydown={(event) => enterSubmits(event, () => void answer(false))}
    onsubmit={(event) => {
      event.preventDefault();
      void answer(false);
    }}
  >
    {#if win.earlier}
      <div role="note">
        <Callout tone="info">{earlierNote(win.earlier, win.clock.now)}</Callout>
      </div>
    {/if}

    <RadioGroup label="Quality" value={optionId} options={qualities} cards onchange={selectOption}>
      {#snippet aside(_quality: SelectOption, index: number)}
        {@const option = choice.options[index]}
        {#if option}
          {@const tag = rowTag(option, output.format, option.id === optionId ? output.kbps : null)}
          {@const size = rowSize(option)}
          {#if index === 0}<Badge tone="accent" size="sm">Best</Badge>{/if}
          {#if tag === 'converts'}
            <Badge tone="warning" size="sm">Converts</Badge>
          {:else if tag === 'copied'}
            <Badge size="sm">No conversion</Badge>
          {/if}
          <span class="text-small text-ink-muted tabular-nums"
            >{size === null ? 'Size not known' : `About ${formatBytes(size)}`}</span
          >
        {/if}
      {/snippet}
    </RadioGroup>
    {#if joinNote}
      <p class="text-ui text-ink-muted" role="note">{joinNote}</p>
    {/if}

    {#if picked}
      <SaveAsOptions option={picked} {choice} defaults={defaultsFor(picked)} {output} />
    {/if}

    <FileNameField
      value={fileName}
      onchange={(name: string) => (fileName = name)}
      placeholder={choice.title}
      description="Leave empty to use the video's title."
    />

    <FolderField
      {folder}
      onchoose={() => void changeFolder()}
      onreset={folder === proposal.folder ? undefined : () => (folder = proposal.folder)}
    />

    <QueueField
      queues={win.queues}
      value={queue}
      onchange={(chosen: QueueId) => (queue = chosen)}
    />

    {#if choice.page_url}
      {@const from = `From ${hostOf(choice.page_url)}`}
      <p class="truncate text-small text-ink-muted" {@attach tooltip(() => textIfCut(from))}>
        {from}
      </p>
    {/if}

    <WindowOptions {win} />
  </form>

  {#snippet footer()}
    <Button variant="ghost" disabled={win.busy} onclick={() => void win.discard()}>Cancel</Button>
    <Button
      disabled={!picked || (win.busy && sending !== 'later')}
      loading={sending === 'later'}
      onclick={() => void answer(true)}>Download later</Button
    >
    <Button
      variant="primary"
      type="submit"
      form={formId}
      icon={DownloadIcon}
      data-autofocus
      disabled={!picked || (win.busy && sending !== 'now')}
      loading={sending === 'now'}
      >{needsInstall ? 'Install add-on and download' : 'Download now'}</Button
    >
  {/snippet}
</WindowFrame>
