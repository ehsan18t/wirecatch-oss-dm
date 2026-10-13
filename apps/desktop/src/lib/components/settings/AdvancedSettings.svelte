<script lang="ts">
  // Advanced: connections per download and downloads at once, within the core's limits.
  import { MAX_CONCURRENT_DOWNLOADS, MAX_CONNECTIONS_PER_DOWNLOAD } from '@wirecatch/protocol';
  import type { Snippet } from 'svelte';
  import { concurrentDownloads, connectionsPerDownload } from '$lib/limits';
  import { commitNumber } from '$lib/number-field';
  import type { SettingIn } from '$lib/settings-schema';
  import type { Row, SectionProps } from './row';
  import SettingRows from './SettingRows.svelte';

  let { items, settings, flash, save }: SectionProps = $props();

  const controls: Record<SettingIn<'advanced'>, Snippet<[Row]>> = {
    connections_per_download: connections,
    max_concurrent_downloads: concurrent,
  };
</script>

<SettingRows {items} {settings} {flash} {controls} />

{#snippet connections({ s, labelId, helpId }: Row)}
  <input
    type="range"
    min="1"
    max={MAX_CONNECTIONS_PER_DOWNLOAD}
    aria-labelledby={labelId}
    aria-describedby={helpId}
    value={s.connections_per_download}
    onchange={(e) =>
      commitNumber(e.currentTarget, {
        read: connectionsPerDownload,
        current: s.connections_per_download,
        show: String,
        save: (v) => save((d) => void (d.connections_per_download = v)),
      })}
  />
  <span class="unit num">{s.connections_per_download}</span>
{/snippet}

{#snippet concurrent({ s, labelId, helpId }: Row)}
  <input
    class="field narrow"
    type="number"
    min="1"
    max={MAX_CONCURRENT_DOWNLOADS}
    aria-labelledby={labelId}
    aria-describedby={helpId}
    value={s.max_concurrent_downloads}
    onchange={(e) =>
      commitNumber(e.currentTarget, {
        read: concurrentDownloads,
        current: s.max_concurrent_downloads,
        show: String,
        save: (v) => save((d) => void (d.max_concurrent_downloads = v)),
      })}
  />
{/snippet}

<style>
  input[type='range'] {
    width: 200px;
    accent-color: var(--accent);
  }
</style>
