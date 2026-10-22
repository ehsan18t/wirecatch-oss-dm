<script lang="ts">
  // Media: what the quality window starts with, separately for videos and for audio only.
  import {
    AUDIO_KBPS_CHOICES,
    AUDIO_OUTPUT_FORMATS,
    VIDEO_OUTPUT_FORMATS,
    type OutputFormat,
  } from '@wirecatch/protocol';
  import type { Snippet } from 'svelte';
  import { FORMAT_LABEL, nearestKbps } from '$lib/media-output';
  import { Select, TagInput, type SelectOption } from '@wirecatch/ui';
  import type { SettingId, SettingIn } from '$lib/settings-schema';
  import type { Row, SectionProps } from './row';
  import SettingRows from './SettingRows.svelte';
  import SettingToggle from './SettingToggle.svelte';

  let { items, settings, flash, save }: SectionProps = $props();

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

  /** A new audio format keeps the bit rate nearest the one chosen, if the format has a choice. */
  function setAudioFormat(format: OutputFormat) {
    save((d) => {
      d.media.audio.format = format;
      const list = AUDIO_KBPS_CHOICES[format];
      const kbps = d.media.audio.audio_kbps;
      d.media.audio.audio_kbps = list && kbps !== null ? nearestKbps(list, kbps) : null;
    });
  }

  /** A subtitle language as typed: lower case letters, digits, `_` and `-`; null when none is left. */
  function language(typed: string): string | null {
    return (
      typed
        .trim()
        .toLowerCase()
        .replace(/[^a-z0-9_-]/g, '') || null
    );
  }

  const formats = (list: readonly OutputFormat[]): SelectOption<OutputFormat>[] =>
    list.map((f) => ({ value: f, label: formatName(f) }));
  const VIDEO_FORMATS = formats(VIDEO_OUTPUT_FORMATS);
  const AUDIO_FORMATS = formats(AUDIO_OUTPUT_FORMATS);

  /** The bit rates a format offers, after the format's usual one (no number). */
  function kbpsOptions(list: readonly number[]): SelectOption<number | 'usual'>[] {
    return [
      { value: 'usual', label: 'Usual quality' },
      ...list.map((k) => ({ value: k, label: `${String(k)} kbps` })),
    ];
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

{#snippet videoFormat({ s, control }: Row)}
  <Select
    {...control}
    class="w-44"
    value={s.media.video.format}
    options={VIDEO_FORMATS}
    onchange={(format: OutputFormat) => save((d) => void (d.media.video.format = format))}
  />
{/snippet}

{#snippet audioFormat({ s, control }: Row)}
  {@const audio = s.media.audio}
  {@const kbpsList = AUDIO_KBPS_CHOICES[audio.format]}
  <Select
    {...control}
    class="w-36"
    value={audio.format}
    options={AUDIO_FORMATS}
    onchange={setAudioFormat}
  />
  {#if kbpsList}
    <Select
      label="Audio quality"
      class="w-36"
      value={audio.audio_kbps ?? 'usual'}
      options={kbpsOptions(kbpsList)}
      onchange={(kbps: number | 'usual') =>
        save((d) => void (d.media.audio.audio_kbps = kbps === 'usual' ? null : kbps))}
    />
  {/if}
{/snippet}

{#snippet subtitles({ def, s, control }: Row)}
  {@const kind = kindOf(def.id)}
  <TagInput
    {...control}
    class="w-64"
    label={def.label}
    placeholder="Add a language, like en"
    values={s.media[kind].subtitle_languages}
    normalize={language}
    onchange={(langs: string[]) => save((d) => void (d.media[kind].subtitle_languages = langs))}
  />
{/snippet}

{#snippet autoSubtitles({ def, s, control }: Row)}
  {@const kind = kindOf(def.id)}
  <SettingToggle
    {control}
    label={def.label}
    checked={s.media[kind].auto_subtitles}
    onchange={(v: boolean) => save((d) => void (d.media[kind].auto_subtitles = v))}
  />
{/snippet}

{#snippet chapters({ def, s, control }: Row)}
  {@const kind = kindOf(def.id)}
  <SettingToggle
    {control}
    label={def.label}
    checked={s.media[kind].chapters}
    onchange={(v: boolean) => save((d) => void (d.media[kind].chapters = v))}
  />
{/snippet}

{#snippet subtitleFiles({ def, s, control }: Row)}
  <SettingToggle
    {control}
    label={def.label}
    checked={s.media.video.subtitle_files}
    onchange={(v: boolean) => save((d) => void (d.media.video.subtitle_files = v))}
  />
{/snippet}
