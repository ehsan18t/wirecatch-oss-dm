<script lang="ts">
  // Advanced: connections per download and downloads at once, within the core's limits, the person's sites on their network (D39), and the list of public domains that decides which sites are one owner's (D44).
  import {
    MAX_CONCURRENT_DOWNLOADS,
    MAX_CONNECTIONS_PER_DOWNLOAD,
    normalizeSite,
  } from '@wirecatch/protocol';
  import type { Snippet } from 'svelte';
  import { suffixListNote } from '$lib/format';
  import { concurrentDownloads, connectionsPerDownload } from '$lib/limits';
  import { commitNumber } from '$lib/number-field';
  import type { SettingIn } from '$lib/settings-schema';
  import { getApp } from '$lib/state/app.svelte';
  import Button from '../Button.svelte';
  import ListEditor from '../ListEditor.svelte';
  import Toggle from '../Toggle.svelte';
  import type { Row, SectionProps } from './row';
  import SettingRows from './SettingRows.svelte';

  let { items, settings, flash, save }: SectionProps = $props();

  const app = getApp();
  const suffixes = $derived(app.suffixList);

  const controls: Record<SettingIn<'advanced'>, Snippet<[Row]>> = {
    connections_per_download: connections,
    max_concurrent_downloads: concurrent,
    local_sites: localSites,
    update_suffix_list: suffixList,
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

{#snippet localSites({ def, s, helpId }: Row)}
  <ListEditor
    label={def.label}
    describedby={helpId}
    placeholder="wiki.example.com"
    items={s.local_sites}
    normalize={(typed: string) => normalizeSite(typed) ?? ''}
    onchange={(sites: string[]) => save((d) => void (d.local_sites = sites))}
  />
{/snippet}

{#snippet suffixList({ def, s, helpId }: Row)}
  <div class="suffix-list">
    <Toggle
      label={def.label}
      describedby={helpId}
      checked={s.update_suffix_list}
      onchange={(v: boolean) => save((d) => void (d.update_suffix_list = v))}
    />
    <p class="suffix-note">
      <span>
        {#if suffixes.info}
          {suffixListNote(suffixes.info, app.clock.now)}
        {:else if suffixes.error}
          {suffixes.error}
        {:else}
          Reading the list.
        {/if}
      </span>
      <Button small disabled={suffixes.checking} onclick={() => suffixes.check()}>Check now</Button>
    </p>
  </div>
{/snippet}

<style>
  .suffix-list {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }

  .suffix-note {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 8px;
    max-width: 320px;
    margin: 0;
    color: var(--ink-2);
    font-size: var(--text-sm);
  }

  input[type='range'] {
    width: 200px;
    accent-color: var(--accent);
  }
</style>
