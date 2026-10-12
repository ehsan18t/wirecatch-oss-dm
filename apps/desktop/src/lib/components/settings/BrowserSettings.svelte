<script lang="ts">
  // Browser: what the extension catches and how a caught download is asked about. The extension's
  // popup changes these same settings.
  import { splitKbOrMb, toBytes } from '@wirecatch/units';
  import type { Snippet } from 'svelte';
  import type { SettingIn } from '$lib/settings-schema';
  import { getApp } from '$lib/state/app.svelte';
  import ListEditor from '../ListEditor.svelte';
  import Toggle from '../Toggle.svelte';
  import type { Row, SectionProps } from './row';
  import SettingRows from './SettingRows.svelte';

  let { items, settings, flash, save }: SectionProps = $props();

  const app = getApp();

  const minSize = $derived(splitKbOrMb(settings.extension.min_capture_size));

  /** Saves the smallest file the extension catches; anything but a size of zero or more is 0. */
  function setMinSize(value: string | number, unit: 'KB' | 'MB') {
    const bytes = toBytes(value, unit) ?? 0;
    save((d) => void (d.extension.min_capture_size = bytes));
  }

  /** A site as typed or pasted, down to its host name. */
  function siteHost(typed: string): string {
    return typed
      .trim()
      .toLowerCase()
      .replace(/^https?:\/\//, '')
      .replace(/\/.*$/, '');
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

{#snippet captureEnabled({ def, s, help }: Row)}
  <Toggle
    label={def.label}
    describedby={help}
    checked={s.extension.capture_enabled}
    onchange={(v: boolean) => save((d) => void (d.extension.capture_enabled = v))}
  />
{/snippet}

{#snippet askBeforeDownload({ def, s, help }: Row)}
  <Toggle
    label={def.label}
    describedby={help}
    checked={s.extension.ask_before_download}
    onchange={(v: boolean) => save((d) => void (d.extension.ask_before_download = v))}
  />
{/snippet}

{#snippet showProgressWindows({ def, s, help }: Row)}
  <Toggle
    label={def.label}
    describedby={help}
    checked={s.show_progress_windows}
    onchange={(v: boolean) => save((d) => void (d.show_progress_windows = v))}
  />
{/snippet}

{#snippet showMediaButton({ def, s, help }: Row)}
  <Toggle
    label={def.label}
    describedby={help}
    checked={s.extension.show_media_button}
    onchange={(v: boolean) => save((d) => void (d.extension.show_media_button = v))}
  />
{/snippet}

{#snippet ignoredSites({ def, s, help }: Row)}
  <ListEditor
    label={def.label}
    describedby={help}
    placeholder="example.com"
    items={s.extension.ignored_sites}
    normalize={siteHost}
    onchange={(sites: string[]) => save((d) => void (d.extension.ignored_sites = sites))}
  />
{/snippet}

{#snippet captureExtensions({ def, s, help }: Row)}
  <ListEditor
    label={def.label}
    describedby={help}
    placeholder="zip, mp4, pdf"
    items={s.extension.capture_extensions}
    normalize={(x: string) => x.trim().toLowerCase().replace(/^\./, '')}
    onchange={(exts: string[]) => save((d) => void (d.extension.capture_extensions = exts))}
  />
{/snippet}

{#snippet minCaptureSize({ labelId, help }: Row)}
  <input
    class="field narrow"
    type="number"
    min="0"
    step="1"
    aria-labelledby={labelId}
    aria-describedby={help}
    value={minSize.value}
    onchange={(e) => setMinSize(e.currentTarget.value, minSize.unit)}
  />
  <select
    class="field"
    aria-label="Unit"
    value={minSize.unit}
    onchange={(e) => setMinSize(minSize.value, e.currentTarget.value === 'MB' ? 'MB' : 'KB')}
  >
    <option value="KB">KB</option>
    <option value="MB">MB</option>
  </select>
{/snippet}

{#snippet defaultQueue({ s, labelId, help }: Row)}
  <select
    class="field"
    aria-labelledby={labelId}
    aria-describedby={help}
    value={s.extension.default_queue}
    onchange={(e) => {
      const q = Number(e.currentTarget.value);
      save((d) => void (d.extension.default_queue = q));
    }}
  >
    {#each app.queues.list as q (q.id)}
      <option value={q.id}>{q.name}</option>
    {/each}
  </select>
{/snippet}
