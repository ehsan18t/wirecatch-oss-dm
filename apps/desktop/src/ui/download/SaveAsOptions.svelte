<script lang="ts">
  // The Save as part of a video's quality window (D24): the format and the audio quality on one line, what that does to the file, the subtitles as one field that opens a list to search and tick (a video may offer dozens), and the chapters. Shown only when the quality and the video offer more than one way to save them.
  import {
    AUDIO_KBPS_CHOICES,
    type MediaChoice,
    type MediaOption,
    type OutputDefaults,
    type OutputFormat,
  } from '@wirecatch/protocol';
  import { Checkbox, MultiSelect, Select } from '@wirecatch/ui';
  import {
    conversionNote,
    fitFor,
    formatChoiceLabel,
    kbpsFor,
    qualityNotes,
    subtitleLabels,
  } from '$lib/media-output';
  import type { OutputChoiceState } from '$lib/window/output-choice.svelte';

  interface Props {
    /** The quality picked. */
    option: MediaOption;
    choice: MediaChoice;
    /** The last choices for this kind of download, which a new format starts from. */
    defaults: OutputDefaults | null;
    /** What is picked so far; this part changes it. */
    output: OutputChoiceState;
  }

  let { option, choice, defaults, output }: Props = $props();

  /** The audio quality menu's entry for the quality the file comes with. */
  const AS_DOWNLOADED = 'as-downloaded';

  const fit = $derived(fitFor(option, output.format));
  const kbpsChoices = $derived(fit ? (AUDIO_KBPS_CHOICES[output.format] ?? null) : null);
  /** Save as, subtitles or chapters to show for this quality and video. */
  const offered = $derived(
    option.outputs.length > 1 || choice.subtitles.length > 0 || choice.chapters,
  );
  const note = $derived(conversionNote(fit, output.format, output.kbps));
  const notes = $derived(qualityNotes(fit, output.format, output.kbps));
  const canChapters = $derived(choice.chapters && (fit?.chapters ?? false));
  const subtitleOptions = $derived.by(() => {
    const names = subtitleLabels(choice.subtitles);
    return choice.subtitles.map((each) => ({
      value: each.id,
      label: names.get(each.id) ?? each.name,
    }));
  });
  const formats = $derived(
    option.outputs.map((each) => ({ value: each.format, label: formatChoiceLabel(each) })),
  );
  const kbpsOptions = $derived.by(() => {
    if (!kbpsChoices || !fit) return [];
    const each = kbpsChoices.map((k) => ({ value: k, label: `${String(k)} kbps` }));
    if (fit.audio === 'encode') return each;
    const usual = fit.audio_kbps === null ? '' : ` (${String(fit.audio_kbps)} kbps)`;
    return [{ value: AS_DOWNLOADED, label: `As downloaded${usual}` }, ...each];
  });

  function chooseFormat(format: OutputFormat) {
    output.format = format;
    output.kbps = kbpsFor(fit, format, output.kbps ?? defaults?.audio_kbps ?? null);
  }

  function chooseKbps(value: number | string) {
    if (value === AS_DOWNLOADED) output.kbps = null;
    else if (typeof value === 'number' && kbpsChoices?.includes(value)) output.kbps = value;
  }
</script>

{#if offered}
  <div class="flex flex-col gap-2">
    {#if option.outputs.length > 1}
      <div class="flex items-center gap-2">
        <span class="w-18 shrink-0 text-ui text-ink-muted" aria-hidden="true">Save as</span>
        <Select
          label="Save as"
          class="min-w-0 flex-1"
          value={output.format}
          options={formats}
          onchange={chooseFormat}
        />
        {#if kbpsOptions.length > 0}
          <Select
            label="Audio quality"
            class="min-w-0 flex-1"
            value={output.kbps ?? AS_DOWNLOADED}
            options={kbpsOptions}
            onchange={chooseKbps}
          />
        {/if}
      </div>
    {/if}
    {#if note}<p class="text-small text-ink-muted" role="note">{note}</p>{/if}
    {#each notes as text (text)}<p class="text-small text-ink-muted" role="note">{text}</p>{/each}

    {#if choice.subtitles.length > 0}
      <div class="flex items-center gap-2">
        <span class="w-18 shrink-0 text-ui text-ink-muted" aria-hidden="true">Subtitles</span>
        <MultiSelect
          label="Subtitles"
          class="min-w-0 flex-1"
          values={output.subtitles}
          options={subtitleOptions}
          searchPlaceholder="Search languages"
          onchange={(ids: string[]) => (output.subtitles = ids)}
        />
      </div>
      {#if output.subtitles.length > 0}
        {#if fit?.embeds_subtitles}
          <Checkbox
            checked={output.subtitleFiles}
            label="Save subtitles as .srt files next to the video"
            onchange={(on: boolean) => (output.subtitleFiles = on)}
          />
        {:else}
          <p class="text-small text-ink-muted">
            Subtitles are saved as .srt files next to the file.
          </p>
        {/if}
      {/if}
    {/if}
    {#if canChapters}
      <Checkbox
        checked={output.chapters}
        label="Add the video's chapters"
        onchange={(on: boolean) => (output.chapters = on)}
      />
    {/if}
  </div>
{/if}
