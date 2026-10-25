<script lang="ts">
  // Browser: what the extension catches and how a caught download is asked about. The extension's
  // popup changes these same settings.
  import { normalizeSite, takesPrivateWindows, type QueueId } from '@wirecatch/protocol';
  import { splitKbOrMb, toBytes } from '@wirecatch/units';
  import type { Snippet } from 'svelte';
  import {
    NumberInput,
    SegmentedControl,
    Select,
    TagInput,
    type SelectOption,
  } from '@wirecatch/ui';
  import { typedExtension } from '$lib/file-names';
  import type { SettingIn } from '$lib/settings-schema';
  import { getApp } from '$lib/state/app.svelte';
  import type { Row, SectionProps } from './row';
  import SettingRows from './SettingRows.svelte';
  import SettingToggle from './SettingToggle.svelte';

  let { items, settings, flash, save }: SectionProps = $props();

  const app = getApp();

  const minSize = $derived(splitKbOrMb(settings.extension.min_capture_size));

  /** The units the smallest file to catch can be typed in. */
  const MIN_SIZE_UNITS: readonly SelectOption<'KB' | 'MB'>[] = [
    { value: 'KB', label: 'KB' },
    { value: 'MB', label: 'MB' },
  ];

  /** The smallest file to catch is typed to a half unit: 1.5 MB, not 1.53. */
  const SIZE_STEP = 0.5;

  /** Saves the smallest file the extension catches, in `unit`; an empty field is 0. */
  function setMinSize(value: number | null, unit: 'KB' | 'MB') {
    const bytes = toBytes(value ?? 0, unit) ?? 0;
    save((d) => void (d.extension.min_capture_size = bytes));
  }

  const queueOptions = $derived(
    app.queues.list.map((q): SelectOption<QueueId> => ({ value: q.id, label: q.settings.name })),
  );

  /** Saves the smallest file the extension catches as the number shown, in a new unit. */
  function setMinSizeUnit(unit: 'KB' | 'MB') {
    const bytes = toBytes(minSize.value, unit) ?? 0;
    save((d) => void (d.extension.min_capture_size = bytes));
  }

  const controls: Record<SettingIn<'browser'>, Snippet<[Row]>> = {
    capture_enabled: captureEnabled,
    ask_before_download: askBeforeDownload,
    show_progress_windows: showProgressWindows,
    show_media_button: showMediaButton,
    private_windows: privateWindows,
    ignored_sites: ignoredSites,
    capture_extensions: captureExtensions,
    min_capture_size: minCaptureSize,
    default_queue: defaultQueue,
  };
</script>

<SettingRows
  {items}
  {settings}
  {flash}
  {controls}
  stacked={['ignored_sites', 'capture_extensions']}
/>

{#snippet captureEnabled({ def, s, control }: Row)}
  <SettingToggle
    {control}
    label={def.label}
    checked={s.extension.capture_enabled}
    onchange={(v: boolean) => save((d) => void (d.extension.capture_enabled = v))}
  />
{/snippet}

{#snippet askBeforeDownload({ def, s, control }: Row)}
  <SettingToggle
    {control}
    label={def.label}
    checked={s.extension.ask_before_download}
    onchange={(v: boolean) => save((d) => void (d.extension.ask_before_download = v))}
  />
{/snippet}

{#snippet showProgressWindows({ def, s, control }: Row)}
  <SettingToggle
    {control}
    label={def.label}
    checked={s.show_progress_windows}
    onchange={(v: boolean) => save((d) => void (d.show_progress_windows = v))}
  />
{/snippet}

{#snippet showMediaButton({ def, s, control }: Row)}
  <SettingToggle
    {control}
    label={def.label}
    checked={s.extension.show_media_button}
    onchange={(v: boolean) => save((d) => void (d.extension.show_media_button = v))}
  />
{/snippet}

{#snippet privateWindows({ def, s, control }: Row)}
  <SettingToggle
    {control}
    label={def.label}
    checked={takesPrivateWindows(s.extension)}
    onchange={(v: boolean) => save((d) => void (d.extension.private_windows = v))}
  />
{/snippet}

{#snippet ignoredSites({ def, s, control }: Row)}
  <TagInput
    {...control}
    label={def.label}
    placeholder="Add a site, like example.com"
    values={s.extension.ignored_sites}
    normalize={normalizeSite}
    onchange={(sites: string[]) => save((d) => void (d.extension.ignored_sites = sites))}
  />
{/snippet}

{#snippet captureExtensions({ def, s, control }: Row)}
  <TagInput
    {...control}
    label={def.label}
    placeholder="Add a type, like zip or mp4"
    values={s.extension.capture_extensions}
    normalize={typedExtension}
    onchange={(exts: string[]) => save((d) => void (d.extension.capture_extensions = exts))}
  />
{/snippet}

{#snippet minCaptureSize({ control }: Row)}
  <NumberInput
    {...control}
    class="w-28"
    min={0}
    step={SIZE_STEP}
    value={minSize.value}
    onchange={(value: number | null) => setMinSize(value, minSize.unit)}
  />
  <SegmentedControl
    label="Unit"
    size="sm"
    value={minSize.unit}
    options={MIN_SIZE_UNITS}
    onchange={setMinSizeUnit}
  />
{/snippet}

{#snippet defaultQueue({ s, control }: Row)}
  <Select
    {...control}
    class="w-48"
    value={s.extension.default_queue}
    options={queueOptions}
    onchange={(queue: QueueId) => save((d) => void (d.extension.default_queue = queue))}
  />
{/snippet}
