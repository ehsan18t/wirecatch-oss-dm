<script lang="ts">
  // Saving files: the download folder, and a folder for each type of file.
  import type { AppSettings, Category } from '@wirecatch/protocol';
  import type { Snippet } from 'svelte';
  import { CATEGORIES, CATEGORY_LABEL, displayPath } from '$lib/format';
  import type { SettingIn } from '$lib/settings-schema';
  import { getApp } from '$lib/state/app.svelte';
  import Button from '../Button.svelte';
  import type { Row, SectionProps } from './row';
  import SettingRows from './SettingRows.svelte';

  let { items, settings, flash }: SectionProps = $props();

  const app = getApp();

  function save(change: (d: AppSettings) => void) {
    void app.updateSettings(change);
  }

  function ruleFor(category: Category) {
    return settings.categories.find((r) => r.category === category);
  }

  async function pickCategoryFolder(category: Category) {
    const picked = await app.backend.pickFolder(ruleFor(category)?.folder);
    if (!picked) return;
    save((d) => {
      const rule = d.categories.find((r) => r.category === category);
      if (rule) rule.folder = picked;
      else d.categories.push({ category, extensions: [], folder: picked });
    });
  }

  async function pickDefaultFolder() {
    const picked = await app.backend.pickFolder(settings.default_folder);
    if (picked) save((d) => void (d.default_folder = picked));
  }

  /** Extensions as typed: separated by spaces, commas or semicolons, with or without a dot. */
  function setExtensions(category: Category, typed: string) {
    const exts = typed
      .split(/[\s,;]+/)
      .map((x) => x.trim().toLowerCase().replace(/^\./, ''))
      .filter(Boolean);
    save((d) => {
      const r = d.categories.find((x) => x.category === category);
      if (r) r.extensions = exts;
      else d.categories.push({ category, extensions: exts, folder: d.default_folder });
    });
  }

  const controls: Record<SettingIn<'files'>, Snippet<[Row]>> = {
    default_folder: defaultFolder,
    categories,
  };
</script>

<SettingRows {items} {settings} {flash} {controls} />

{#snippet defaultFolder({ s, help }: Row)}
  <span class="path">{displayPath(s.default_folder)}</span>
  <Button small aria-describedby={help} onclick={pickDefaultFolder}>Change</Button>
{/snippet}

{#snippet categories(_: Row)}
  <table class="rules">
    <caption class="visually-hidden">Folders by file type</caption>
    <thead>
      <tr><th scope="col">Type</th><th scope="col">Extensions</th><th scope="col">Folder</th></tr>
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
              onchange={(e) => setExtensions(category, e.currentTarget.value)}
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
{/snippet}

<style>
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
</style>
