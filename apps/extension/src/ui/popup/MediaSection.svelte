<script lang="ts">
  // How videos and audio download, folded under one line that says what they are set to (D61): the button over videos and the keyboard shortcut (D70), the resolution and what to take when a video lacks it, the formats, subtitles, chapters and the audio quality, with the app's own choices (`@wirecatch/protocol` media-choices). Every change is saved in the app.
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
    SUBTITLE_PLACEMENT_CHOICES,
    UNEMBEDDED_CHOICES,
    videoQuality,
    type BrowserControls,
    type ExtensionSettings,
    type OutputFormat,
    type QualityFallback,
    type QualityValue,
    type UnembeddedSubtitles,
  } from '@wirecatch/protocol';
  import {
    Button,
    Disclosure,
    Kbd,
    MultiSelect,
    Select,
    SettingRow,
    SettingSwitch,
  } from '@wirecatch/ui';
  import type { PopupModel } from '../../lib/popup/model.svelte';
  import { SETTINGS } from '../settings';

  interface Props {
    model: PopupModel;
    settings: ExtensionSettings;
    /** Null for an app from before the popup could show them: only the button shows. */
    controls: BrowserControls | null;
    /** The keyboard shortcut that downloads a page's media; null when none is set. */
    shortcut: string | null;
    /** Opens the browser's page of extension shortcuts; null where no page can open it (Firefox). */
    openShortcuts: (() => void) | null;
  }

  let { model, settings, controls, shortcut, openShortcuts }: Props = $props();

  const VIDEO_FORMATS = formatChoices(VIDEO_OUTPUT_FORMATS);
  const AUDIO_FORMATS = formatChoices(AUDIO_OUTPUT_FORMATS);

  const editable = $derived(model.editable);
  const keys = $derived(shortcut === null ? [] : shortcut.split('+'));
  const shortcutText = $derived(
    openShortcuts === null ? SETTINGS.shortcutFirefox : SETTINGS.shortcut,
  );
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

<Disclosure variant="card" label="Video and audio" {summary}>
  <SettingSwitch
    id="media-button"
    label={SETTINGS.mediaButton.label}
    help={SETTINGS.mediaButton.help}
    checked={settings.show_media_button}
    disabled={!editable}
    onchange={(on: boolean) => void model.update({ show_media_button: on })}
  />
  <SettingRow label={shortcutText.label} help={shortcutText.help} labelled={false}>
    {#if keys.length > 0}
      <Kbd {keys} />
    {:else}
      <span class="text-ui text-ink-muted">Not set</span>
    {/if}
    {#if openShortcuts !== null}
      {const action = $derived(keys.length > 0 ? 'Change' : 'Set')}
      <Button size="sm" aria-label="{action} the keyboard shortcut" onclick={openShortcuts}>
        {action}
      </Button>
    {/if}
  </SettingRow>
  {#if controls === null}
    <p class="px-4 py-3 text-small text-ink-muted">
      Update Wirecatch to choose how videos download from here.
    </p>
  {:else}
    {const media = $derived(controls.media)}
    {const kbps = $derived(kbpsChoices(media.audio.format))}
    <SettingRow label={SETTINGS.quality.label} help={SETTINGS.quality.help} stacked>
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
      <SettingRow label={SETTINGS.fallback.label} help={SETTINGS.fallback.help} stacked>
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
    <SettingRow label={SETTINGS.videoFormat.label} help={SETTINGS.videoFormat.help} stacked>
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
    <SettingRow label={SETTINGS.subtitles.label} help={SETTINGS.subtitles.help} stacked>
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
      help={SETTINGS.autoSubtitles.help}
      checked={media.video.auto_subtitles}
      disabled={!editable}
      onchange={(on: boolean) =>
        void model.updateControls({ media: { video: { auto_subtitles: on } } })}
    />
    <SettingRow label={SETTINGS.subtitleFiles.label} help={SETTINGS.subtitleFiles.help} stacked>
      {#snippet children(control)}
        <Select
          {...control}
          class="w-full"
          value={media.video.subtitle_files ? 'files' : 'inside'}
          options={SUBTITLE_PLACEMENT_CHOICES}
          disabled={!editable}
          onchange={(placed: 'inside' | 'files') =>
            void model.updateControls({
              media: { video: { subtitle_files: placed === 'files' } },
            })}
        />
      {/snippet}
    </SettingRow>
    {#if !media.video.subtitle_files}
      <SettingRow
        label={SETTINGS.unembeddedSubtitles.label}
        help={SETTINGS.unembeddedSubtitles.help}
        stacked
      >
        {#snippet children(control)}
          <Select
            {...control}
            class="w-full"
            value={media.video.unembedded_subtitles}
            options={UNEMBEDDED_CHOICES}
            disabled={!editable}
            onchange={(when: UnembeddedSubtitles) =>
              void model.updateControls({ media: { video: { unembedded_subtitles: when } } })}
          />
        {/snippet}
      </SettingRow>
    {/if}
    <SettingSwitch
      id="chapters"
      label={SETTINGS.chapters.label}
      help={SETTINGS.chapters.help}
      checked={media.video.chapters}
      disabled={!editable}
      onchange={(on: boolean) => void model.updateControls({ media: { video: { chapters: on } } })}
    />
    <SettingRow label={SETTINGS.audioFormat.label} help={SETTINGS.audioFormat.help} stacked>
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
      <SettingRow label={SETTINGS.audioQuality.label} help={SETTINGS.audioQuality.help} stacked>
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
</Disclosure>
