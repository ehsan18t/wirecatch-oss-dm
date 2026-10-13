<script lang="ts">
  // Browser: what the extension catches and how a caught download is asked about. The extension's
  // popup changes these same settings.
  import { normalizeSite } from '@wirecatch/protocol';
  import { splitKbOrMb, toBytes } from '@wirecatch/units';
  import type { Snippet } from 'svelte';
  import { commitNumber } from '$lib/number-field';
  import { offeredValue } from '$lib/select-value';
  import type { SettingIn } from '$lib/settings-schema';
  import { getApp } from '$lib/state/app.svelte';
  import ListEditor from '../ListEditor.svelte';
  import Toggle from '../Toggle.svelte';
  import type { Row, SectionProps } from './row';
  import SettingRows from './SettingRows.svelte';

  let { items, settings, flash, save }: SectionProps = $props();

  const app = getApp();

  const minSize = $derived(splitKbOrMb(settings.extension.min_capture_size));

  /** The units the smallest file to catch can be typed in. */
  const MIN_SIZE_UNITS = ['KB', 'MB'] as const;

  /** Saves the smallest file the extension catches, in `unit`; anything but a size of zero or more is 0. */
  function setMinSize(field: HTMLInputElement, unit: 'KB' | 'MB') {
    commitNumber(field, {
      read: (typed) => toBytes(typed, unit) ?? 0,
      current: settings.extension.min_capture_size,
      show: (bytes) => String(splitKbOrMb(bytes).value),
      save: (bytes) => save((d) => void (d.extension.min_capture_size = bytes)),
    });
  }

  /** Saves the smallest file the extension catches as the number shown, in a new unit. */
  function setMinSizeUnit(unit: 'KB' | 'MB') {
    const bytes = toBytes(minSize.value, unit) ?? 0;
    save((d) => void (d.extension.min_capture_size = bytes));
  }

  /** A site as typed or pasted, down to its host name the way the app keeps it; empty when it names none. */
  function siteHost(typed: string): string {
    return normalizeSite(typed) ?? '';
  }

  const controls: Record<SettingIn<'browser'>, Snippet<[Row]>> = {
    capture_enabled: captureEnabled,
    ask_before_download: askBeforeDownload,
    show_progress_windows: showProgressWindows,
    show_media_button: showMediaButton,
    ignored_sites: ignoredSites,
    capture_extensions: captureExtensions,
    min_capture_size: minCaptureSize,
    default_queue: defaultQueue,
  };
</script>

<SettingRows {items} {settings} {flash} {controls} />

{#snippet captureEnabled({ def, s, helpId }: Row)}
  <Toggle
    label={def.label}
    describedby={helpId}
    checked={s.extension.capture_enabled}
    onchange={(v: boolean) => save((d) => void (d.extension.capture_enabled = v))}
  />
{/snippet}

{#snippet askBeforeDownload({ def, s, helpId }: Row)}
  <Toggle
    label={def.label}
    describedby={helpId}
    checked={s.extension.ask_before_download}
    onchange={(v: boolean) => save((d) => void (d.extension.ask_before_download = v))}
  />
{/snippet}

{#snippet showProgressWindows({ def, s, helpId }: Row)}
  <Toggle
    label={def.label}
    describedby={helpId}
    checked={s.show_progress_windows}
    onchange={(v: boolean) => save((d) => void (d.show_progress_windows = v))}
  />
{/snippet}

{#snippet showMediaButton({ def, s, helpId }: Row)}
  <Toggle
    label={def.label}
    describedby={helpId}
    checked={s.extension.show_media_button}
    onchange={(v: boolean) => save((d) => void (d.extension.show_media_button = v))}
  />
{/snippet}

{#snippet ignoredSites({ def, s, helpId }: Row)}
  <ListEditor
    label={def.label}
    describedby={helpId}
    placeholder="example.com"
    items={s.extension.ignored_sites}
    normalize={siteHost}
    onchange={(sites: string[]) => save((d) => void (d.extension.ignored_sites = sites))}
  />
{/snippet}

{#snippet captureExtensions({ def, s, helpId }: Row)}
  <ListEditor
    label={def.label}
    describedby={helpId}
    placeholder="zip, mp4, pdf"
    items={s.extension.capture_extensions}
    normalize={(x: string) => x.trim().toLowerCase().replace(/^\./, '')}
    onchange={(exts: string[]) => save((d) => void (d.extension.capture_extensions = exts))}
  />
{/snippet}

{#snippet minCaptureSize({ labelId, helpId }: Row)}
  <input
    class="field narrow"
    type="number"
    min="0"
    step="1"
    aria-labelledby={labelId}
    aria-describedby={helpId}
    value={minSize.value}
    onchange={(e) => setMinSize(e.currentTarget, minSize.unit)}
  />
  <select
    class="field"
    aria-label="Unit"
    value={minSize.unit}
    onchange={(e) => {
      const unit = offeredValue(MIN_SIZE_UNITS, e.currentTarget.value);
      if (unit !== undefined) setMinSizeUnit(unit);
    }}
  >
    {#each MIN_SIZE_UNITS as unit (unit)}<option value={unit}>{unit}</option>{/each}
  </select>
{/snippet}

{#snippet defaultQueue({ s, labelId, helpId }: Row)}
  <select
    class="field"
    aria-labelledby={labelId}
    aria-describedby={helpId}
    value={s.extension.default_queue}
    onchange={(e) => {
      const q = offeredValue(
        app.queues.list.map((queue) => queue.id),
        e.currentTarget.value,
      );
      if (q !== undefined) save((d) => void (d.extension.default_queue = q));
    }}
  >
    {#each app.queues.list as q (q.id)}
      <option value={q.id}>{q.name}</option>
    {/each}
  </select>
{/snippet}
