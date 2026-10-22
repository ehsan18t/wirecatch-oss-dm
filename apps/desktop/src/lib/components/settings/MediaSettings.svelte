<script lang="ts">
  // Media: whether a video asks for a quality or downloads at once with the saved choices (D50), the resolution and what to do when a video lacks it, and what the quality window starts with, separately for videos and for audio only.
  import {
    AUDIO_KBPS_CHOICES,
    AUDIO_OUTPUT_FORMATS,
    VIDEO_OUTPUT_FORMATS,
    type OutputFormat,
    type QualityFallback,
    type VideoQuality,
    normalizeSite,
  } from '@wirecatch/protocol';
  import type { Snippet } from 'svelte';
  import { languageOptions } from '$lib/languages';
  import { FORMAT_LABEL, nearestKbps } from '$lib/media-output';
  import {
    MultiSelect,
    SegmentedControl,
    Select,
    TagInput,
    type SelectOption,
  } from '@wirecatch/ui';
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

  /** Whether a video asks, as the switch holds it. */
  type Asking = 'ask' | 'saved';

  const ASKING: SelectOption<Asking>[] = [
    { value: 'ask', label: 'Ask each time' },
    { value: 'saved', label: 'Use my choices' },
  ];

  const FALLBACKS: SelectOption<QualityFallback>[] = [
    { value: 'lower', label: 'Next lower' },
    { value: 'higher', label: 'Next higher' },
    { value: 'ask', label: 'Ask me' },
  ];

  /** The resolutions offered, tallest first, between Best available and Audio only. */
  const HEIGHTS = [2160, 1440, 1080, 720, 480, 360];

  /** A resolution as the menu holds it: a height, or a word. */
  type QualityValue = number | 'best' | 'audio_only';

  function qualityValue(q: VideoQuality): QualityValue {
    return typeof q === 'string' ? q : q.height;
  }

  function videoQuality(v: QualityValue): VideoQuality {
    return typeof v === 'number' ? { height: v } : v;
  }

  /** Best available, the usual heights and the saved one if it is another (a pick from the window), Audio only. */
  function qualityOptions(saved: VideoQuality): SelectOption<QualityValue>[] {
    const heights = [...HEIGHTS];
    if (typeof saved !== 'string' && !heights.includes(saved.height)) {
      heights.push(saved.height);
      heights.sort((a, b) => b - a);
    }
    return [
      { value: 'best', label: 'Best available' },
      ...heights.map((h) => ({ value: h, label: `${String(h)}p` })),
      { value: 'audio_only', label: 'Audio only' },
    ];
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
    media_ask: asking,
    media_quality: quality,
    media_fallback: fallback,
    media: videoFormat,
    media_subtitles: subtitles,
    media_auto_subtitles: autoSubtitles,
    media_subtitle_files: subtitleFiles,
    media_chapters: chapters,
    video_sites: videoSites,
    media_audio: audioFormat,
    media_audio_subtitles: subtitles,
    media_audio_auto_subtitles: autoSubtitles,
    media_audio_chapters: chapters,
  };
</script>

<SettingRows {items} {settings} {flash} {controls} stacked={['video_sites']} />

{#snippet videoSites({ def, s, control }: Row)}
  <TagInput
    {...control}
    label={def.label}
    placeholder="Add a site, like example.com"
    values={s.extension.video_sites ?? []}
    normalize={normalizeSite}
    onchange={(sites: string[]) => save((d) => void (d.extension.video_sites = sites))}
  />
{/snippet}

{#snippet asking({ def, s }: Row)}
  <SegmentedControl
    value={s.media.ask_each_time ? 'ask' : 'saved'}
    options={ASKING}
    label={def.label}
    size="sm"
    onchange={(v: Asking) => save((d) => void (d.media.ask_each_time = v === 'ask'))}
  />
{/snippet}

{#snippet quality({ s, control }: Row)}
  <Select
    {...control}
    class="w-44"
    value={qualityValue(s.media.quality)}
    options={qualityOptions(s.media.quality)}
    onchange={(v: QualityValue) => save((d) => void (d.media.quality = videoQuality(v)))}
  />
{/snippet}

{#snippet fallback({ def, s }: Row)}
  <SegmentedControl
    value={s.media.fallback}
    options={FALLBACKS}
    label={def.label}
    disabled={typeof s.media.quality === 'string'}
    size="sm"
    onchange={(f: QualityFallback) => save((d) => void (d.media.fallback = f))}
  />
{/snippet}

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
  <MultiSelect
    {...control}
    class="w-64"
    label={def.label}
    searchPlaceholder="Search languages"
    values={s.media[kind].subtitle_languages}
    options={languageOptions(s.media[kind].subtitle_languages)}
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
