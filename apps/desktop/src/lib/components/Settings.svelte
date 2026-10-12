<script lang="ts">
  import ArrowLeft from '@lucide/svelte/icons/arrow-left';
  import Search from '@lucide/svelte/icons/search';
  import {
    AUDIO_KBPS_CHOICES,
    MAX_CONCURRENT_DOWNLOADS,
    MAX_CONNECTIONS_PER_DOWNLOAD,
    type AppSettings,
    type Category,
    type OutputFormat,
    type Theme,
  } from '@wirecatch/protocol';
  import { inUnit, MB, splitKbOrMb, toBytes } from '@wirecatch/units';
  import { tick } from 'svelte';
  import { CATEGORIES, CATEGORY_LABEL, displayPath } from '$lib/format';
  import { concurrentDownloads, connectionsPerDownload } from '$lib/limits';
  import { FORMAT_LABEL, nearestKbps } from '$lib/media-output';
  import {
    SECTIONS,
    searchSettings,
    settingAnchor,
    type SettingDef,
    type SettingId,
  } from '$lib/settings-schema';
  import { getApp } from '$lib/state/app.svelte';
  import { MEDIA_ADDON } from '$lib/addons';
  import AddonCard from './AddonCard.svelte';
  import Button from './Button.svelte';
  import Icon from './Icon.svelte';
  import ListEditor from './ListEditor.svelte';
  import Toggle from './Toggle.svelte';

  const app = getApp();
  /** The total speed limit a person starts from when turning it on; a suggestion, not a default of the core. */
  const FIRST_SPEED_LIMIT = 5 * MB;

  let query = $state('');
  /** How long a setting opened from elsewhere stays highlighted, so the eye finds it. */
  const FLASH_MS = 1600;
  let flash = $state<SettingId | null>(null);

  const matches = $derived(searchSettings(query));
  const s = $derived(app.settings);

  function save(change: (d: AppSettings) => void) {
    void app.updateSettings(change);
  }

  function itemsIn(section: string): SettingDef[] {
    return matches.filter((m) => m.section === section);
  }

  // Jump to a setting asked for from the command palette.
  $effect(() => {
    const target = app.settingsFocus;
    if (!target) return;
    query = '';
    void tick().then(() => {
      const el = document.getElementById(settingAnchor(target));
      el?.scrollIntoView({ block: 'center' });
      el?.querySelector<HTMLElement>('button, input, select, textarea')?.focus({
        preventScroll: true,
      });
      flash = target;
      setTimeout(() => (flash = null), FLASH_MS);
      app.settingsFocus = null;
    });
  });

  const VIDEO_FORMATS: OutputFormat[] = ['original', 'mp4', 'mkv', 'webm'];
  const AUDIO_FORMATS: OutputFormat[] = ['original', 'mp3', 'm4a', 'opus', 'flac', 'wav'];

  const THEMES: { value: Theme; label: string }[] = [
    { value: 'system', label: 'Match system' },
    { value: 'light', label: 'Light' },
    { value: 'dark', label: 'Dark' },
  ];

  function ruleFor(category: Category) {
    return s?.categories.find((r) => r.category === category);
  }

  async function pickCategoryFolder(category: Category) {
    const current = ruleFor(category)?.folder;
    const picked = await app.backend.pickFolder(current);
    if (!picked) return;
    save((d) => {
      const rule = d.categories.find((r) => r.category === category);
      if (rule) rule.folder = picked;
      else d.categories.push({ category, extensions: [], folder: picked });
    });
  }

  async function pickDefaultFolder() {
    const picked = await app.backend.pickFolder(s?.default_folder);
    if (picked) save((d) => void (d.default_folder = picked));
  }

  const minSize = $derived(splitKbOrMb(s?.extension.min_capture_size ?? 0));

  /** Saves the smallest file the extension catches; anything but a size of zero or more is 0. */
  function setMinSize(value: string | number, unit: 'KB' | 'MB') {
    const bytes = toBytes(value, unit) ?? 0;
    save((d) => void (d.extension.min_capture_size = bytes));
  }
</script>

<div class="settings">
  <header>
    <Button
      variant="ghost"
      small
      class="back"
      icon={ArrowLeft}
      onclick={() => (app.screen = 'downloads')}
    >
      Back to downloads
    </Button>
    <h1>Settings</h1>
    <p class="lede">
      Changes save as you make them. Browser settings are shared with the extension.
    </p>
    <div class="search">
      <Icon icon={Search} size="sm" />
      <input
        id="settings-search"
        class="field"
        type="search"
        placeholder="Search settings, for example: speed, folder, dark"
        aria-label="Search settings"
        bind:value={query}
      />
    </div>
  </header>

  <div class="scroll">
    {#if !s}
      <p class="muted">Loading settings.</p>
    {:else}
      {#each SECTIONS as section (section.id)}
        {@const items = itemsIn(section.id)}
        {#if items.length > 0}
          <section aria-labelledby="sec-{section.id}">
            <h2 id="sec-{section.id}">{section.title}</h2>
            <p class="blurb">{section.blurb}</p>
            {#each items as def (def.id)}
              {@render setting(def)}
            {/each}
          </section>
        {/if}
      {:else}
        <div class="none">
          <p>No setting matches "{query}".</p>
          <Button onclick={() => (query = '')}>Clear search</Button>
        </div>
      {/each}
    {/if}
  </div>
</div>

{#snippet setting(def: SettingDef)}
  {@const help = `help-${def.id}`}
  <div class="setting" class:flash={flash === def.id} id={settingAnchor(def.id)}>
    <div class="text">
      <span class="label" id="label-{def.id}">{def.label}</span>
      <span class="help" id={help}>{def.help}</span>
    </div>
    <div class="control">
      {#if s}
        {#if def.id === 'theme'}
          <div
            class="segmented"
            role="radiogroup"
            aria-labelledby="label-theme"
            aria-describedby={help}
          >
            {#each THEMES as t (t.value)}
              <button
                type="button"
                role="radio"
                aria-checked={s.theme === t.value}
                tabindex={s.theme === t.value ? 0 : -1}
                onclick={() => save((d) => void (d.theme = t.value))}
                onkeydown={(e) => {
                  if (e.key !== 'ArrowRight' && e.key !== 'ArrowLeft') return;
                  e.preventDefault();
                  const i = THEMES.findIndex((x) => x.value === s.theme);
                  const next =
                    THEMES[(i + (e.key === 'ArrowRight' ? 1 : THEMES.length - 1)) % THEMES.length];
                  if (next) {
                    save((d) => void (d.theme = next.value));
                    const group = e.currentTarget.parentElement;
                    void tick().then(() =>
                      group?.querySelector<HTMLElement>('[aria-checked="true"]')?.focus(),
                    );
                  }
                }}
              >
                {t.label}
              </button>
            {/each}
          </div>
        {:else if def.id === 'start_with_system'}
          <Toggle
            label={def.label}
            describedby={help}
            checked={s.start_with_system}
            onchange={(v: boolean) => save((d) => void (d.start_with_system = v))}
          />
        {:else if def.id === 'close_to_tray'}
          <Toggle
            label={def.label}
            describedby={help}
            checked={s.close_to_tray}
            onchange={(v: boolean) => save((d) => void (d.close_to_tray = v))}
          />
        {:else if def.id === 'notify_on_complete'}
          <Toggle
            label={def.label}
            describedby={help}
            checked={s.notify_on_complete}
            onchange={(v: boolean) => save((d) => void (d.notify_on_complete = v))}
          />
        {:else if def.id === 'media_addon'}
          {@const addon = app.addons?.find((a) => a.id === MEDIA_ADDON)}
          {#if addon}
            <AddonCard {addon} describedby={help} />
          {:else if app.addonsError}
            <p class="addon-note">
              {app.addonsError}
              <Button small onclick={() => app.loadAddons()}>Check again</Button>
            </p>
          {:else}
            <p class="addon-note">Checking.</p>
          {/if}
        {:else if def.id === 'auto_update_addons'}
          <Toggle
            label={def.label}
            describedby={help}
            checked={s.auto_update_addons}
            onchange={(v: boolean) => save((d) => void (d.auto_update_addons = v))}
          />
        {:else if def.id === 'default_folder'}
          <span class="path">{displayPath(s.default_folder)}</span>
          <Button small aria-describedby={help} onclick={pickDefaultFolder}>Change</Button>
        {:else if def.id === 'categories'}
          <table class="rules">
            <caption class="visually-hidden">Folders by file type</caption>
            <thead>
              <tr
                ><th scope="col">Type</th><th scope="col">Extensions</th><th scope="col">Folder</th
                ></tr
              >
            </thead>
            <tbody>
              {#each CATEGORIES.filter((c) => c !== 'other') as category (category)}
                {@const rule = ruleFor(category)}
                <tr>
                  <th scope="row">{CATEGORY_LABEL[category]}</th>
                  <td>
                    <input
                      class="field"
                      aria-label="Extensions for {CATEGORY_LABEL[category].toLowerCase()}"
                      value={rule?.extensions.join(', ') ?? ''}
                      placeholder="none"
                      onchange={(e) => {
                        const exts = e.currentTarget.value
                          .split(/[\s,;]+/)
                          .map((x) => x.trim().toLowerCase().replace(/^\./, ''))
                          .filter(Boolean);
                        save((d) => {
                          const r = d.categories.find((x) => x.category === category);
                          if (r) r.extensions = exts;
                          else
                            d.categories.push({
                              category,
                              extensions: exts,
                              folder: d.default_folder,
                            });
                        });
                      }}
                    />
                  </td>
                  <td class="folder-cell">
                    <span class="path">{rule ? displayPath(rule.folder) : 'Download folder'}</span>
                    <Button
                      small
                      aria-label="Change folder for {CATEGORY_LABEL[category].toLowerCase()}"
                      onclick={() => pickCategoryFolder(category)}>Change</Button
                    >
                  </td>
                </tr>
              {/each}
            </tbody>
          </table>
        {:else if def.id === 'capture_enabled'}
          <Toggle
            label={def.label}
            describedby={help}
            checked={s.extension.capture_enabled}
            onchange={(v: boolean) => save((d) => void (d.extension.capture_enabled = v))}
          />
        {:else if def.id === 'ask_before_download'}
          <Toggle
            label={def.label}
            describedby={help}
            checked={s.extension.ask_before_download}
            onchange={(v: boolean) => save((d) => void (d.extension.ask_before_download = v))}
          />
        {:else if def.id === 'show_progress_windows'}
          <Toggle
            label={def.label}
            describedby={help}
            checked={s.show_progress_windows}
            onchange={(v: boolean) => save((d) => void (d.show_progress_windows = v))}
          />
        {:else if def.id === 'show_media_button'}
          <Toggle
            label={def.label}
            describedby={help}
            checked={s.extension.show_media_button}
            onchange={(v: boolean) => save((d) => void (d.extension.show_media_button = v))}
          />
        {:else if def.id === 'ignored_sites'}
          <ListEditor
            label={def.label}
            describedby={help}
            placeholder="example.com"
            items={s.extension.ignored_sites}
            normalize={(x: string) =>
              x
                .trim()
                .toLowerCase()
                .replace(/^https?:\/\//, '')
                .replace(/\/.*$/, '')}
            onchange={(items: string[]) => save((d) => void (d.extension.ignored_sites = items))}
          />
        {:else if def.id === 'capture_extensions'}
          <ListEditor
            label={def.label}
            describedby={help}
            placeholder="zip, mp4, pdf"
            items={s.extension.capture_extensions}
            normalize={(x: string) => x.trim().toLowerCase().replace(/^\./, '')}
            onchange={(items: string[]) =>
              save((d) => void (d.extension.capture_extensions = items))}
          />
        {:else if def.id === 'min_capture_size'}
          <input
            class="field narrow"
            type="number"
            min="0"
            step="1"
            aria-labelledby="label-min_capture_size"
            aria-describedby={help}
            value={minSize.value}
            onchange={(e) => setMinSize(e.currentTarget.value, minSize.unit)}
          />
          <select
            class="field"
            aria-label="Unit"
            value={minSize.unit}
            onchange={(e) =>
              setMinSize(minSize.value, e.currentTarget.value === 'MB' ? 'MB' : 'KB')}
          >
            <option value="KB">KB</option>
            <option value="MB">MB</option>
          </select>
        {:else if def.id === 'default_queue'}
          <select
            class="field"
            aria-labelledby="label-default_queue"
            aria-describedby={help}
            value={s.extension.default_queue}
            onchange={(e) => {
              const q = Number(e.currentTarget.value);
              save((d) => void (d.extension.default_queue = q));
            }}
          >
            {#each app.queues as q (q.id)}
              <option value={q.id}>{q.name}</option>
            {/each}
          </select>
        {:else if def.id === 'media'}
          <select
            class="field"
            aria-labelledby="label-media"
            aria-describedby={help}
            value={s.media.video.format}
            onchange={(e) => {
              const v = e.currentTarget.value as OutputFormat;
              save((d) => void (d.media.video.format = v));
            }}
          >
            {#each VIDEO_FORMATS as f (f)}
              <option value={f}>{f === 'original' ? 'Original' : FORMAT_LABEL[f]}</option>
            {/each}
          </select>
        {:else if def.id === 'media_subtitles' || def.id === 'media_audio_subtitles'}
          {@const kind = def.id === 'media_subtitles' ? 'video' : 'audio'}
          <ListEditor
            label={def.label}
            describedby={help}
            placeholder="en, fr"
            items={s.media[kind].subtitle_languages}
            normalize={(x: string) =>
              x
                .trim()
                .toLowerCase()
                .replace(/[^a-z0-9_-]/g, '')}
            onchange={(items: string[]) =>
              save((d) => void (d.media[kind].subtitle_languages = items))}
          />
        {:else if def.id === 'media_auto_subtitles' || def.id === 'media_audio_auto_subtitles'}
          {@const kind = def.id === 'media_auto_subtitles' ? 'video' : 'audio'}
          <Toggle
            label={def.label}
            describedby={help}
            checked={s.media[kind].auto_subtitles}
            onchange={(v: boolean) => save((d) => void (d.media[kind].auto_subtitles = v))}
          />
        {:else if def.id === 'media_subtitle_files'}
          <Toggle
            label={def.label}
            describedby={help}
            checked={s.media.video.subtitle_files}
            onchange={(v: boolean) => save((d) => void (d.media.video.subtitle_files = v))}
          />
        {:else if def.id === 'media_chapters' || def.id === 'media_audio_chapters'}
          {@const kind = def.id === 'media_chapters' ? 'video' : 'audio'}
          <Toggle
            label={def.label}
            describedby={help}
            checked={s.media[kind].chapters}
            onchange={(v: boolean) => save((d) => void (d.media[kind].chapters = v))}
          />
        {:else if def.id === 'media_audio'}
          {@const audio = s.media.audio}
          {@const kbpsList = AUDIO_KBPS_CHOICES[audio.format]}
          <select
            class="field"
            aria-labelledby="label-media_audio"
            aria-describedby={help}
            value={audio.format}
            onchange={(e) => {
              const v = e.currentTarget.value as OutputFormat;
              save((d) => {
                d.media.audio.format = v;
                const list = AUDIO_KBPS_CHOICES[v];
                const kbps = d.media.audio.audio_kbps;
                d.media.audio.audio_kbps = list && kbps !== null ? nearestKbps(list, kbps) : null;
              });
            }}
          >
            {#each AUDIO_FORMATS as f (f)}
              <option value={f}>{f === 'original' ? 'Original' : FORMAT_LABEL[f]}</option>
            {/each}
          </select>
          {#if kbpsList}
            <select
              class="field"
              aria-label="Audio quality"
              value={audio.audio_kbps === null ? '' : String(audio.audio_kbps)}
              onchange={(e) => {
                const v = e.currentTarget.value;
                save((d) => void (d.media.audio.audio_kbps = v === '' ? null : Number(v)));
              }}
            >
              <option value="">Usual quality</option>
              {#each kbpsList as k (k)}
                <option value={String(k)}>{k} kbps</option>
              {/each}
            </select>
          {/if}
        {:else if def.id === 'global_speed_limit'}
          <Toggle
            label="Limit total speed"
            describedby={help}
            checked={s.global_speed_limit !== null}
            onchange={(v: boolean) =>
              save((d) => void (d.global_speed_limit = v ? FIRST_SPEED_LIMIT : null))}
          />
          {#if s.global_speed_limit !== null}
            <input
              class="field narrow"
              type="number"
              min="0.1"
              step="0.5"
              aria-label="Speed limit in megabytes per second"
              value={inUnit(s.global_speed_limit, 'MB')}
              onchange={(e) => {
                const bytes = toBytes(e.currentTarget.value, 'MB');
                if (bytes !== null && bytes > 0) save((d) => void (d.global_speed_limit = bytes));
              }}
            />
            <span class="unit">MB/s</span>
          {/if}
        {:else if def.id === 'export'}
          <Button aria-describedby={help} onclick={() => app.openDialog({ kind: 'export' })}
            >Export settings</Button
          >
        {:else if def.id === 'import'}
          <Button aria-describedby={help} onclick={() => app.openDialog({ kind: 'import' })}
            >Import settings</Button
          >
        {:else if def.id === 'connections_per_download'}
          <input
            type="range"
            min="1"
            max={MAX_CONNECTIONS_PER_DOWNLOAD}
            aria-labelledby="label-connections_per_download"
            aria-describedby={help}
            value={s.connections_per_download}
            onchange={(e) => {
              const v = connectionsPerDownload(Number(e.currentTarget.value));
              save((d) => void (d.connections_per_download = v));
            }}
          />
          <span class="unit num">{s.connections_per_download}</span>
        {:else}
          <input
            class="field narrow"
            type="number"
            min="1"
            max={MAX_CONCURRENT_DOWNLOADS}
            aria-labelledby="label-max_concurrent_downloads"
            aria-describedby={help}
            value={s.max_concurrent_downloads}
            onchange={(e) => {
              const v = concurrentDownloads(Number(e.currentTarget.value));
              save((d) => void (d.max_concurrent_downloads = v));
            }}
          />
        {/if}
      {/if}
    </div>
  </div>
{/snippet}

<style>
  .settings {
    display: flex;
    flex: 1;
    flex-direction: column;
    min-height: 0;
  }

  header {
    padding: 14px 28px 14px;
    border-bottom: 1px solid var(--line);
  }

  header > :global(.back) {
    margin-left: -9px;
    color: var(--ink-2);
  }

  h1 {
    margin: 6px 0 2px;
    font-size: var(--text-xl);
    font-weight: 620;
    letter-spacing: -0.01em;
  }

  .lede,
  .muted {
    margin: 0;
    color: var(--ink-2);
    font-size: var(--text-sm);
  }

  .search {
    position: relative;
    display: flex;
    align-items: center;
    max-width: 520px;
    margin-top: 12px;
    color: var(--ink-3);
  }

  .search :global(svg) {
    position: absolute;
    left: 10px;
    pointer-events: none;
  }

  .search input {
    width: 100%;
    height: 34px;
    padding-left: 31px;
  }

  .scroll {
    flex: 1;
    min-height: 0;
    padding: 4px 28px 48px;
    overflow-y: auto;
  }

  section {
    max-width: 860px;
    padding: 22px 0 6px;
  }

  section + section {
    border-top: 1px solid var(--line);
  }

  h2 {
    margin: 0;
    font-size: var(--text-lg);
    font-weight: 620;
  }

  .blurb {
    margin: 2px 0 12px;
    color: var(--ink-2);
    font-size: var(--text-sm);
  }

  .setting {
    display: grid;
    grid-template-columns: minmax(220px, 1fr) minmax(0, 1.2fr);
    align-items: center;
    gap: 8px 28px;
    margin: 0 -10px;
    padding: 10px;
    border-radius: var(--radius-m);
    transition: background 400ms var(--ease);
  }

  .setting.flash {
    background: var(--accent-soft);
  }

  .setting:has(table) {
    grid-template-columns: 1fr;
  }

  .text {
    display: grid;
    gap: 2px;
  }

  .label {
    font-weight: 600;
  }

  .help {
    color: var(--ink-2);
    font-size: var(--text-sm);
  }

  .control {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 8px;
  }

  .path {
    color: var(--ink-2);
    font-size: var(--text-sm);
    overflow-wrap: anywhere;
  }

  .narrow {
    width: 90px;
  }

  .addon-note {
    display: flex;
    align-items: center;
    gap: 8px;
    max-width: 320px;
    margin: 0;
    color: var(--ink-2);
    font-size: var(--text-sm);
  }

  .unit {
    color: var(--ink-2);
    font-size: var(--text-sm);
  }

  input[type='range'] {
    width: 200px;
    accent-color: var(--accent);
  }

  .segmented {
    display: inline-flex;
    padding: 2px;
    border: 1px solid var(--line-strong);
    border-radius: var(--radius-m);
    background: var(--surface-2);
  }

  .segmented button {
    height: 26px;
    padding: 0 12px;
    border: 0;
    border-radius: 5px;
    background: transparent;
    color: var(--ink-2);
    font-size: var(--text-sm);
    cursor: pointer;
  }

  .segmented button[aria-checked='true'] {
    background: var(--surface);
    color: var(--ink);
    font-weight: 600;
    box-shadow: 0 0 0 1px var(--line-strong);
  }

  .rules {
    width: 100%;
    border-collapse: collapse;
    font-size: var(--text-sm);
  }

  .rules th,
  .rules td {
    padding: 5px 8px 5px 0;
    text-align: left;
    vertical-align: middle;
  }

  .rules thead th {
    color: var(--ink-2);
    font-size: var(--text-xs);
    font-weight: 600;
  }

  .rules tbody th {
    font-weight: 500;
    white-space: nowrap;
  }

  .rules input {
    width: 100%;
  }

  .folder-cell {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
  }

  .none {
    padding: 40px 0;
    color: var(--ink-2);
  }
</style>
