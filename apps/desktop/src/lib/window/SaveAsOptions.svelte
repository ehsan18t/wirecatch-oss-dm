<script lang="ts">
  // The Save as part of a video's quality window (D24): the format, the audio quality, what that
  // does to the file, the subtitles and the chapters. Shown only when the quality and the video
  // offer more than one way to save them.
  import {
    AUDIO_KBPS_CHOICES,
    type MediaChoice,
    type MediaOption,
    type OutputDefaults,
  } from '@wirecatch/protocol';
  import {
    conversionNote,
    fitFor,
    formatChoiceLabel,
    kbpsFor,
    qualityNotes,
  } from '$lib/media-output';
  import type { OutputChoiceState } from './output-choice.svelte';

  let {
    option,
    choice,
    defaults,
    output,
  }: {
    /** The quality picked. */
    option: MediaOption;
    choice: MediaChoice;
    /** The last choices for this kind of download, which a new format starts from. */
    defaults: OutputDefaults | null;
    /** What is picked so far; this part changes it. */
    output: OutputChoiceState;
  } = $props();

  const fit = $derived(fitFor(option, output.format));
  const kbpsChoices = $derived(fit ? (AUDIO_KBPS_CHOICES[output.format] ?? null) : null);
  /** Save as, subtitles or chapters to show for this quality and video. */
  const offered = $derived(
    option.outputs.length > 1 || choice.subtitles.length > 0 || choice.chapters,
  );
  const note = $derived(conversionNote(fit, output.format, output.kbps));
  const notes = $derived(qualityNotes(fit, output.format, output.kbps));
  const canChapters = $derived(choice.chapters && (fit?.chapters ?? false));

  function formatChanged() {
    output.kbps = kbpsFor(fit, output.format, output.kbps ?? defaults?.audio_kbps ?? null);
  }
</script>

{#if offered}
  <div class="output">
    {#if option.outputs.length > 1}
      <div class="pair">
        <label class="stack">
          <span class="label">Save as</span>
          <select class="field" bind:value={output.format} onchange={formatChanged}>
            {#each option.outputs as each (each.format)}
              <option value={each.format}>{formatChoiceLabel(each)}</option>
            {/each}
          </select>
        </label>
        {#if kbpsChoices && fit}
          <label class="stack">
            <span class="label">Audio quality</span>
            <select
              class="field"
              value={output.kbps === null ? '' : String(output.kbps)}
              onchange={(e) => {
                const v = e.currentTarget.value;
                output.kbps = v === '' ? null : Number(v);
              }}
            >
              {#if fit.audio !== 'encode'}
                <option value=""
                  >As downloaded{fit.audio_kbps === null ? '' : ` (${fit.audio_kbps} kbps)`}</option
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
              <input type="checkbox" value={subtitle.id} bind:group={output.subtitles} />
              <span
                >{subtitle.name}{#if subtitle.auto}<span class="auto">, auto-generated</span
                  >{/if}</span
              >
            </label>
          {/each}
        </div>
        {#if output.subtitles.length > 0}
          {#if fit?.embeds_subtitles}
            <label class="check">
              <input type="checkbox" bind:checked={output.subtitleFiles} />
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
        <input type="checkbox" bind:checked={output.chapters} />
        <span>Add the video's chapters</span>
      </label>
    {/if}
  </div>
{/if}

<style>
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

  .stack {
    display: grid;
    gap: 4px;
  }

  .label {
    font-size: var(--text-sm);
    font-weight: 600;
  }

  .note {
    margin: 0;
    color: var(--ink-2);
    font-size: var(--text-sm);
    line-height: 1.5;
  }

  .help {
    margin: 0;
    color: var(--ink-2);
    font-size: var(--text-xs);
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
</style>
