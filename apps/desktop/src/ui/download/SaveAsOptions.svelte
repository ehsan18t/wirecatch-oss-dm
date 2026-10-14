<script lang="ts">
  // The Save as part of a video's quality window (D24): the format, the audio quality, what that does to the file, the subtitles and the chapters. Shown only when the quality and the video offer more than one way to save them.
  import {
    AUDIO_KBPS_CHOICES,
    type MediaChoice,
    type MediaOption,
    type OutputDefaults,
    type OutputFormat,
  } from '@wirecatch/protocol';
  import { Checkbox, Field, type FieldControl, Select } from '@wirecatch/ui';
  import {
    conversionNote,
    fitFor,
    formatChoiceLabel,
    kbpsFor,
    qualityNotes,
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

  function toggleSubtitle(id: string, on: boolean) {
    output.subtitles = on
      ? [...output.subtitles.filter((each) => each !== id), id]
      : output.subtitles.filter((each) => each !== id);
  }
</script>

{#if offered}
  <div class="flex flex-col gap-3 rounded-md border border-line bg-wash p-3">
    {#if option.outputs.length > 1}
      <div class="flex flex-wrap gap-3">
        <Field label="Save as" class="flex-1 basis-40">
          {#snippet children(control: FieldControl)}
            <Select {...control} value={output.format} options={formats} onchange={chooseFormat} />
          {/snippet}
        </Field>
        {#if kbpsOptions.length > 0}
          <Field label="Audio quality" class="flex-1 basis-40">
            {#snippet children(control: FieldControl)}
              <Select
                {...control}
                value={output.kbps ?? AS_DOWNLOADED}
                options={kbpsOptions}
                onchange={chooseKbps}
              />
            {/snippet}
          </Field>
        {/if}
      </div>
    {/if}
    {#if note}<p class="text-ui text-ink-muted" role="note">{note}</p>{/if}
    {#each notes as text (text)}<p class="text-ui text-ink-muted" role="note">{text}</p>{/each}

    {#if choice.subtitles.length > 0}
      <fieldset class="flex min-w-0 flex-col gap-2">
        <legend class="mb-2 text-ui font-medium text-ink">Subtitles</legend>
        <div
          class="flex max-h-32 flex-col gap-2 overflow-y-auto rounded-sm border border-line bg-field px-2.5 py-2"
        >
          {#each choice.subtitles as subtitle (subtitle.id)}
            <Checkbox
              checked={output.subtitles.includes(subtitle.id)}
              label={subtitle.auto ? `${subtitle.name}, auto-generated` : subtitle.name}
              onchange={(on: boolean) => toggleSubtitle(subtitle.id, on)}
            />
          {/each}
        </div>
        {#if output.subtitles.length > 0}
          {#if fit?.embeds_subtitles}
            <Checkbox
              checked={output.subtitleFiles}
              label="Save them as .srt files next to the video instead"
              onchange={(on: boolean) => (output.subtitleFiles = on)}
            />
          {:else}
            <p class="text-small text-ink-muted">They are saved as .srt files next to the file.</p>
          {/if}
        {/if}
      </fieldset>
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
