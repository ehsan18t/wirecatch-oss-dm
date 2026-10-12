<script lang="ts">
  // Media: what the quality window starts with, separately for videos and for audio only.
  import { AUDIO_KBPS_CHOICES, type OutputFormat } from '@wirecatch/protocol';
  import type { Snippet } from 'svelte';
  import { FORMAT_LABEL, nearestKbps } from '$lib/media-output';
  import type { SettingId, SettingIn } from '$lib/settings-schema';
  import ListEditor from '../ListEditor.svelte';
  import Toggle from '../Toggle.svelte';
  import type { Row, SectionProps } from './row';
  import SettingRows from './SettingRows.svelte';

  let { items, settings, flash, save }: SectionProps = $props();

  const VIDEO_FORMATS: OutputFormat[] = ['original', 'mp4', 'mkv', 'webm'];
  const AUDIO_FORMATS: OutputFormat[] = ['original', 'mp3', 'm4a', 'opus', 'flac', 'wav'];

  /** Videos, or audio only: each keeps its own choices. */
  type Kind = 'video' | 'audio';

  /** The settings that come in a pair, one for videos and one for audio only: the audio ones. */
  const AUDIO_ONLY: ReadonlySet<SettingId> = new Set<SettingId>([
    'media_audio_subtitles',
    'media_audio_auto_subtitles',
    'media_audio_chapters',
  ]);

  function kindOf(id: SettingId): Kind {
    return AUDIO_ONLY.has(id) ? 'audio' : 'video';
  }

  function formatName(f: OutputFormat): string {
    return f === 'original' ? 'Original' : FORMAT_LABEL[f];
  }

  /** The format a menu's value names, from the formats it offers. */
  function pick(formats: readonly OutputFormat[], value: string): OutputFormat | undefined {
    return formats.find((f) => f === value);
  }

  /** A new audio format keeps the bit rate nearest the one chosen, if the format has a choice. */
  function setAudioFormat(format: OutputFormat) {
    save((d) => {
      d.media.audio.format = format;
      const list = AUDIO_KBPS_CHOICES[format];
      const kbps = d.media.audio.audio_kbps;
      d.media.audio.audio_kbps = list && kbps !== null ? nearestKbps(list, kbps) : null;
    });
  }

  /** A subtitle language as typed: lower case letters, digits, `_` and `-`. */
  function language(typed: string): string {
    return typed
      .trim()
      .toLowerCase()
      .replace(/[^a-z0-9_-]/g, '');
  }

  const controls: Record<SettingIn<'media'>, Snippet<[Row]>> = {
    media: videoFormat,
    media_subtitles: subtitles,
    media_auto_subtitles: autoSubtitles,
    media_subtitle_files: subtitleFiles,
    media_chapters: chapters,
    media_audio: audioFormat,
    media_audio_subtitles: subtitles,
    media_audio_auto_subtitles: autoSubtitles,
    media_audio_chapters: chapters,
  };
</script>

<SettingRows {items} {settings} {flash} {controls} />

{#snippet videoFormat({ s, labelId, helpId }: Row)}
  <select
    class="field"
    aria-labelledby={labelId}
    aria-describedby={helpId}
    value={s.media.video.format}
    onchange={(e) => {
      const format = pick(VIDEO_FORMATS, e.currentTarget.value);
      if (format) save((d) => void (d.media.video.format = format));
    }}
  >
    {#each VIDEO_FORMATS as f (f)}<option value={f}>{formatName(f)}</option>{/each}
  </select>
{/snippet}

{#snippet audioFormat({ s, labelId, helpId }: Row)}
  {@const audio = s.media.audio}
  {@const kbpsList = AUDIO_KBPS_CHOICES[audio.format]}
  <select
    class="field"
    aria-labelledby={labelId}
    aria-describedby={helpId}
    value={audio.format}
    onchange={(e) => {
      const format = pick(AUDIO_FORMATS, e.currentTarget.value);
      if (format) setAudioFormat(format);
    }}
  >
    {#each AUDIO_FORMATS as f (f)}<option value={f}>{formatName(f)}</option>{/each}
  </select>
  {#if kbpsList}
    <select
      class="field"
      aria-label="Audio quality"
      value={audio.audio_kbps === null ? '' : String(audio.audio_kbps)}
      onchange={(e) => {
        const v = e.currentTarget.value;
        save((d) => void (d.media.audio.audio_kbps = v === '' ? null : Number(v)));
      }}
    >
      <option value="">Usual quality</option>
      {#each kbpsList as k (k)}
        <option value={String(k)}>{k} kbps</option>
      {/each}
    </select>
  {/if}
{/snippet}

{#snippet subtitles({ def, s, helpId }: Row)}
  {@const kind = kindOf(def.id)}
  <ListEditor
    label={def.label}
    describedby={helpId}
    placeholder="en, fr"
    items={s.media[kind].subtitle_languages}
    normalize={language}
    onchange={(langs: string[]) => save((d) => void (d.media[kind].subtitle_languages = langs))}
  />
{/snippet}

{#snippet autoSubtitles({ def, s, helpId }: Row)}
  {@const kind = kindOf(def.id)}
  <Toggle
    label={def.label}
    describedby={helpId}
    checked={s.media[kind].auto_subtitles}
    onchange={(v: boolean) => save((d) => void (d.media[kind].auto_subtitles = v))}
  />
{/snippet}

{#snippet chapters({ def, s, helpId }: Row)}
  {@const kind = kindOf(def.id)}
  <Toggle
    label={def.label}
    describedby={helpId}
    checked={s.media[kind].chapters}
    onchange={(v: boolean) => save((d) => void (d.media[kind].chapters = v))}
  />
{/snippet}

{#snippet subtitleFiles({ def, s, helpId }: Row)}
  <Toggle
    label={def.label}
    describedby={helpId}
    checked={s.media.video.subtitle_files}
    onchange={(v: boolean) => save((d) => void (d.media.video.subtitle_files = v))}
  />
{/snippet}
