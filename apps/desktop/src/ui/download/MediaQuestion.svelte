<script lang="ts">
  // A video's qualities, best first, each row saying only what sets it apart (Best, a conversion it needs, its size when known; that a format copies without converting is the note under Save as), with what to save it as (D24), its file name, folder and queue, then Download now, Download later or Cancel. Enter in a field is Download now. A quality whose video and audio must be joined while the media add-on is missing installs the add-on first. The window starts where the core proposes from the saved choices (D50), saying when the video lacks the saved resolution, and every choice says before it starts whether it converts. "Use these choices for every video" goes with the answer: the next video downloads with them without this window.
  import DownloadIcon from '~icons/lucide/download';
  import Info from '~icons/lucide/info';
  import { PRODUCT_NAME } from '@wirecatch/product';
  import type { MediaChoice, MediaOption, QueueId } from '@wirecatch/protocol';
  import { formatBytes } from '@wirecatch/units';
  import {
    Badge,
    Button,
    Checkbox,
    cn,
    Disclosure,
    Icon,
    Select,
    type SelectOption,
  } from '@wirecatch/ui';
  import { isInstalled } from '$lib/addons';
  import { folderTail } from '$lib/file-names';
  import { earlierNote, hostOf } from '$lib/format';
  import {
    fitFor,
    FORMAT_LABEL,
    kbpsFor,
    outputSelection,
    proposedChoice,
    rowTag,
    sizeFor,
    switchedChoice,
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

  // The form starts from the proposal, and edits its own copy.
  const proposal = (() => ({ ...choice }))();
  const defaultsFor = (option: MediaOption | undefined) =>
    win.settings?.media[option?.audio_only ? 'audio' : 'video'] ?? null;

  let optionId = $state(proposal.start.option_id);
  let fileName = $state('');
  let folder = $state(proposal.folder);
  let queue = $state<QueueId>(proposal.queue);
  const output = new OutputChoiceState(proposedChoice(proposal.start));
  /** "Use these choices for every video", as the settings say until the person changes it. */
  let everyVideo = $state<boolean | null>(null);
  const everyVideoShown = $derived(everyVideo ?? win.settings?.media.ask_each_time === false);
  /** What ticking it saves: from a video without the saved resolution, that resolution stays saved, not the one picked here. */
  const everyVideoNote = $derived(
    choice.missing_quality
      ? `Videos then download at once, at ${choice.missing_quality} as saved. Change it in Settings, Media.`
      : 'Videos then download at once with these choices. Change it in Settings, Media.',
  );
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
        ? `Comes as separate video and audio, joined into one file when it finishes.`
        : null;
    }
    const size = win.addon?.size ? ` (${formatBytes(win.addon.size)})` : '';
    return `Comes as separate video and audio: ${PRODUCT_NAME} first installs the media add-on${size} to join them.`;
  });
  const qualities = $derived<SelectOption[]>(
    choice.options.map((option) => ({ value: option.id, label: rowLabel(option) })),
  );

  /** Where the file goes, in one line: its folder, its queue, and the name typed if any. */
  const whereSummary = $derived(
    [folderTail(folder), win.queues.find((q) => q.id === queue)?.name ?? '', fileName.trim()]
      .filter(Boolean)
      .join(' · '),
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
      output.reset(switchedChoice(now, choice, defaultsFor(now), output));
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
      every_video: win.settings ? everyVideoShown : null,
    });
    sending = null;
  }
</script>

<WindowFrame
  title="Pick a quality"
  description={choice.page_url ? `${choice.title} · ${hostOf(choice.page_url)}` : choice.title}
  error={win.error}
>
  <!-- svelte-ignore a11y_no_noninteractive_element_interactions (Enter in any field, radio buttons and checkboxes included, is Download now: see keys.ts) -->
  <form
    id={formId}
    class="flex flex-col gap-3"
    onkeydown={(event) => enterSubmits(event, () => void answer(false))}
    onsubmit={(event) => {
      event.preventDefault();
      void answer(false);
    }}
  >
    {#if choice.missing_quality}
      <p class="flex items-center gap-1.5 text-small text-ink" role="note">
        <Icon icon={Info} size="sm" class="shrink-0 text-accent" />This video doesn’t offer {choice.missing_quality},
        your saved resolution.
      </p>
    {/if}
    {#if win.earlier}
      <p class="flex items-center gap-1.5 text-small text-ink-muted" role="note">
        <Icon icon={Info} size="sm" class="shrink-0" />{earlierNote(win.earlier, win.clock.now)}
      </p>
    {/if}

    <div class="flex items-center gap-2">
      <span class="w-18 shrink-0 text-ui text-ink-muted" aria-hidden="true">Quality</span>
      <Select
        label="Quality"
        class="min-w-0 flex-1"
        value={optionId}
        options={qualities}
        searchable
        onchange={selectOption}
      >
        {#snippet aside(quality: SelectOption)}
          {@const index = choice.options.findIndex((o) => o.id === quality.value)}
          {@const option = choice.options[index]}
          {#if option}
            {@const tag = rowTag(
              option,
              output.format,
              option.id === optionId ? output.kbps : null,
            )}
            {@const size = rowSize(option)}
            {#if index === 0}<Badge size="sm">Best</Badge>{/if}
            {#if tag === 'converts'}<Badge tone="warning" size="sm">Converts</Badge>{/if}
            {#if size !== null}
              <span class="text-small text-ink-muted tabular-nums">{formatBytes(size)}</span>
            {/if}
          {/if}
        {/snippet}
      </Select>
    </div>
    {#if joinNote}
      <p
        class={cn('flex gap-1.5 text-small', needsInstall ? 'text-ink' : 'text-ink-muted')}
        role="note"
      >
        <Icon
          icon={Info}
          size="sm"
          class={cn('mt-0.5 shrink-0', needsInstall ? 'text-accent' : 'text-ink-subtle')}
        />{joinNote}
      </p>
    {/if}

    {#if picked}
      <SaveAsOptions option={picked} {choice} defaults={defaultsFor(picked)} {output} />
    {/if}

    <Disclosure label="Save to" summary={whereSummary}>
      <FileNameField
        value={fileName}
        onchange={(name: string) => (fileName = name)}
        placeholder={choice.title}
        description="Leave empty to use the video's title."
      />

      <FolderField
        label="Folder"
        {folder}
        onchoose={() => void changeFolder()}
        onreset={folder === proposal.folder ? undefined : () => (folder = proposal.folder)}
      />

      <QueueField
        queues={win.queues}
        value={queue}
        onchange={(chosen: QueueId) => (queue = chosen)}
      />
    </Disclosure>

    <WindowOptions {win}>
      <Checkbox
        checked={everyVideoShown}
        label="Use these choices for every video"
        description={everyVideoNote}
        onchange={(on: boolean) => (everyVideo = on)}
      />
    </WindowOptions>
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
      loading={sending === 'now'}>{needsInstall ? 'Install and download' : 'Download now'}</Button
    >
  {/snippet}
</WindowFrame>
