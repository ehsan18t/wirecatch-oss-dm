<script lang="ts">
  // How videos and audio download, folded under one line that says what they are set to (D61): the button over videos, the resolution and what to take when a video lacks it, the formats, subtitles, chapters and the audio quality, with the app's own choices (`@wirecatch/protocol` media-choices). Every change is saved in the app.
  import {
    AUDIO_OUTPUT_FORMATS,
    FALLBACK_CHOICES,
    FORMAT_LABEL,
    VIDEO_OUTPUT_FORMATS,
    formatChoices,
    kbpsChoices,
    kbpsForFormat,
    languageChoices,
    qualityChoices,
    qualityValue,
    videoQuality,
    type BrowserControls,
    type ExtensionSettings,
    type OutputFormat,
    type QualityFallback,
    type QualityValue,
  } from '@wirecatch/protocol';
  import { Card, Disclosure, MultiSelect, Select, SettingRow, SettingSwitch } from '@wirecatch/ui';
  import type { PopupModel } from '../../lib/popup/model.svelte';
  import { SETTINGS } from '../settings';

  interface Props {
    model: PopupModel;
    settings: ExtensionSettings;
    /** Null for an app from before the popup could show them: only the button shows. */
    controls: BrowserControls | null;
  }

  let { model, settings, controls }: Props = $props();

  const VIDEO_FORMATS = formatChoices(VIDEO_OUTPUT_FORMATS);
  const AUDIO_FORMATS = formatChoices(AUDIO_OUTPUT_FORMATS);

  const editable = $derived(model.editable);
  const summary = $derived.by(() => {
    if (controls === null) {
      return settings.show_media_button ? 'Download button on' : 'Download button off';
    }
    const { media } = controls;
    const quality = qualityChoices(media.quality).find(
      (c) => c.value === qualityValue(media.quality),
    );
    return [quality?.label, FORMAT_LABEL[media.video.format]]
      .filter((part) => part !== undefined)
      .join(' · ');
  });

  /** A new audio format keeps the quality nearest the one chosen, if the format has a choice. */
  function setAudioFormat(format: OutputFormat): void {
    const kbps = kbpsForFormat(format, controls?.media.audio.audio_kbps ?? null);
    void model.updateControls({ media: { audio: { format, audio_kbps: kbps } } });
  }
</script>

<Disclosure label="Video and audio" {summary}>
  <Card padding="none" class="divide-y divide-line">
    <SettingSwitch
      id="media-button"
      label={SETTINGS.mediaButton.label}
      description={SETTINGS.mediaButton.description}
      checked={settings.show_media_button}
      disabled={!editable}
      onchange={(on: boolean) => void model.update({ show_media_button: on })}
    />
    {#if controls === null}
      <p class="px-4 py-3 text-small text-ink-muted">
        Update Wirecatch to choose how videos download from here.
      </p>
    {:else}
      {@const media = controls.media}
      {@const kbps = kbpsChoices(media.audio.format)}
      <SettingRow label={SETTINGS.quality.label} description={SETTINGS.quality.description} stacked>
        {#snippet children(control)}
          <Select
            {...control}
            class="w-full"
            value={qualityValue(media.quality)}
            options={qualityChoices(media.quality)}
            disabled={!editable}
            onchange={(v: QualityValue) =>
              void model.updateControls({ media: { quality: videoQuality(v) } })}
          />
        {/snippet}
      </SettingRow>
      {#if typeof media.quality !== 'string'}
        <SettingRow
          label={SETTINGS.fallback.label}
          description={SETTINGS.fallback.description}
          stacked
        >
          {#snippet children(control)}
            <Select
              {...control}
              class="w-full"
              value={media.fallback}
              options={FALLBACK_CHOICES}
              disabled={!editable}
              onchange={(fallback: QualityFallback) =>
                void model.updateControls({ media: { fallback } })}
            />
          {/snippet}
        </SettingRow>
      {/if}
      <SettingRow
        label={SETTINGS.videoFormat.label}
        description={SETTINGS.videoFormat.description}
        stacked
      >
        {#snippet children(control)}
          <Select
            {...control}
            class="w-full"
            value={media.video.format}
            options={VIDEO_FORMATS}
            disabled={!editable}
            onchange={(format: OutputFormat) =>
              void model.updateControls({ media: { video: { format } } })}
          />
        {/snippet}
      </SettingRow>
      <SettingRow
        label={SETTINGS.subtitles.label}
        description={SETTINGS.subtitles.description}
        stacked
      >
        {#snippet children(control)}
          <MultiSelect
            {...control}
            class="w-full"
            placeholder="None"
            searchPlaceholder="Search languages"
            values={media.video.subtitle_languages}
            options={languageChoices(media.video.subtitle_languages)}
            disabled={!editable}
            onchange={(languages: string[]) =>
              void model.updateControls({ media: { video: { subtitle_languages: languages } } })}
          />
        {/snippet}
      </SettingRow>
      <SettingSwitch
        id="auto-subtitles"
        label={SETTINGS.autoSubtitles.label}
        description={SETTINGS.autoSubtitles.description}
        checked={media.video.auto_subtitles}
        disabled={!editable}
        onchange={(on: boolean) =>
          void model.updateControls({ media: { video: { auto_subtitles: on } } })}
      />
      <SettingSwitch
        id="subtitle-files"
        label={SETTINGS.subtitleFiles.label}
        description={SETTINGS.subtitleFiles.description}
        checked={media.video.subtitle_files}
        disabled={!editable}
        onchange={(on: boolean) =>
          void model.updateControls({ media: { video: { subtitle_files: on } } })}
      />
      <SettingSwitch
        id="chapters"
        label={SETTINGS.chapters.label}
        description={SETTINGS.chapters.description}
        checked={media.video.chapters}
        disabled={!editable}
        onchange={(on: boolean) =>
          void model.updateControls({ media: { video: { chapters: on } } })}
      />
      <SettingRow
        label={SETTINGS.audioFormat.label}
        description={SETTINGS.audioFormat.description}
        stacked
      >
        {#snippet children(control)}
          <Select
            {...control}
            class="w-full"
            value={media.audio.format}
            options={AUDIO_FORMATS}
            disabled={!editable}
            onchange={setAudioFormat}
          />
        {/snippet}
      </SettingRow>
      {#if kbps.length > 0}
        <SettingRow
          label={SETTINGS.audioQuality.label}
          description={SETTINGS.audioQuality.description}
          stacked
        >
          {#snippet children(control)}
            <Select
              {...control}
              class="w-full"
              value={media.audio.audio_kbps ?? 'usual'}
              options={kbps}
              disabled={!editable}
              onchange={(value: number | 'usual') =>
                void model.updateControls({
                  media: { audio: { audio_kbps: value === 'usual' ? null : value } },
                })}
            />
          {/snippet}
        </SettingRow>
      {/if}
    {/if}
  </Card>
</Disclosure>
