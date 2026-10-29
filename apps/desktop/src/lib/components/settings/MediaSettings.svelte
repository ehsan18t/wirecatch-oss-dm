<script lang="ts">
  // Media: the media add-on, then whether a video asks for a quality or downloads at once with the saved choices (D50), the resolution and what to do when a video lacks it, and what the quality window starts with, separately for videos and for audio only.
  import {
    AUDIO_OUTPUT_FORMATS,
    FALLBACK_CHOICES,
    VIDEO_OUTPUT_FORMATS,
    formatChoices,
    kbpsChoices,
    kbpsForFormat,
    languageChoices,
    normalizeSite,
    qualityChoices,
    qualityValue,
    videoQuality,
    type OutputFormat,
    type QualityFallback,
    type QualityValue,
  } from '@wirecatch/protocol';
  import type { Snippet } from 'svelte';
  import {
    Button,
    MultiSelect,
    SegmentedControl,
    Select,
    TagInput,
    type SelectOption,
  } from '@wirecatch/ui';
  import { MEDIA_ADDON } from '$lib/addons';
  import type { SettingId, SettingIn } from '$lib/settings-schema';
  import { getApp } from '$lib/state/app.svelte';
  import AddonCard from '../AddonCard.svelte';
  import type { Row, SectionProps } from './row';
  import RowNote from './RowNote.svelte';
  import SettingRows from './SettingRows.svelte';
  import SettingToggle from './SettingToggle.svelte';

  let { items, settings, flash, save }: SectionProps = $props();

  const app = getApp();
  const addon = $derived(app.addons.list?.find((a) => a.id === MEDIA_ADDON));

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

  /** A new audio format keeps the bit rate nearest the one chosen, if the format has a choice. */
  function setAudioFormat(format: OutputFormat) {
    save((d) => {
      d.media.audio.format = format;
      d.media.audio.audio_kbps = kbpsForFormat(format, d.media.audio.audio_kbps);
    });
  }

  /** Whether a video asks, as the switch holds it. */
  type Asking = 'ask' | 'saved';

  const ASKING: SelectOption<Asking>[] = [
    { value: 'ask', label: 'Ask each time' },
    { value: 'saved', label: 'Use my choices' },
  ];

  const VIDEO_FORMATS = formatChoices(VIDEO_OUTPUT_FORMATS);
  const AUDIO_FORMATS = formatChoices(AUDIO_OUTPUT_FORMATS);

  const controls: Record<SettingIn<'media'>, Snippet<[Row]>> = {
    media_addon: mediaAddon,
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

<SettingRows
  {items}
  {settings}
  {flash}
  {controls}
  notes={{ media_fallback: fallbackNote }}
  stacked={['video_sites']}
/>

{#snippet fallbackNote()}
  {#if typeof settings.media.quality === 'string'}
    <RowNote>Used only when Resolution is a height, like 1080p.</RowNote>
  {/if}
{/snippet}

{#snippet mediaAddon({ control }: Row)}
  {#if addon}
    <AddonCard {addon} describedby={control['aria-describedby']} />
  {:else if app.addons.error}
    <span class="text-small text-danger">{app.addons.error}</span>
    <Button size="sm" onclick={() => app.addons.load()}>Check again</Button>
  {:else}
    <span class="text-small text-ink-muted">Checking.</span>
  {/if}
{/snippet}

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
    class="w-56"
    value={qualityValue(s.media.quality)}
    options={qualityChoices(s.media.quality)}
    onchange={(v: QualityValue) => save((d) => void (d.media.quality = videoQuality(v)))}
  />
{/snippet}

{#snippet fallback({ def, s }: Row)}
  <SegmentedControl
    value={s.media.fallback}
    options={FALLBACK_CHOICES}
    label={def.label}
    disabled={typeof s.media.quality === 'string'}
    size="sm"
    onchange={(f: QualityFallback) => save((d) => void (d.media.fallback = f))}
  />
{/snippet}

{#snippet videoFormat({ s, control }: Row)}
  <Select
    {...control}
    class="w-56"
    value={s.media.video.format}
    options={VIDEO_FORMATS}
    onchange={(format: OutputFormat) => save((d) => void (d.media.video.format = format))}
  />
{/snippet}

{#snippet audioFormat({ s, control }: Row)}
  {const audio = $derived(s.media.audio)}
  {const kbps = $derived(kbpsChoices(audio.format))}
  <Select
    {...control}
    class="w-56"
    value={audio.format}
    options={AUDIO_FORMATS}
    onchange={setAudioFormat}
  />
  {#if kbps.length > 0}
    <Select
      label="Audio quality"
      class="w-56"
      value={audio.audio_kbps ?? 'usual'}
      options={kbps}
      onchange={(kbps: number | 'usual') =>
        save((d) => void (d.media.audio.audio_kbps = kbps === 'usual' ? null : kbps))}
    />
  {/if}
{/snippet}

{#snippet subtitles({ def, s, control }: Row)}
  {const kind = $derived(kindOf(def.id))}
  <MultiSelect
    {...control}
    class="w-56"
    label={def.label}
    searchPlaceholder="Search languages"
    values={s.media[kind].subtitle_languages}
    options={languageChoices(s.media[kind].subtitle_languages)}
    onchange={(langs: string[]) => save((d) => void (d.media[kind].subtitle_languages = langs))}
  />
{/snippet}

{#snippet autoSubtitles({ def, s, control }: Row)}
  {const kind = $derived(kindOf(def.id))}
  <SettingToggle
    {control}
    label={def.label}
    checked={s.media[kind].auto_subtitles}
    onchange={(v: boolean) => save((d) => void (d.media[kind].auto_subtitles = v))}
  />
{/snippet}

{#snippet chapters({ def, s, control }: Row)}
  {const kind = $derived(kindOf(def.id))}
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
