<script lang="ts">
  // Saving files: the download folder, and a folder for each type of file.
  import type { Category } from '@wirecatch/protocol';
  import type { Snippet } from 'svelte';
  import { CATEGORIES, CATEGORY_LABEL, displayPath } from '$lib/format';
  import { typedExtension } from '$lib/file-names';
  import type { SettingIn } from '$lib/settings-schema';
  import { getApp } from '$lib/state/app.svelte';
  import { Button, TagInput, textIfCut, tooltip } from '@wirecatch/ui';
  import type { Row, SectionProps } from './row';
  import SettingRows from './SettingRows.svelte';

  let { items, settings, flash, save }: SectionProps = $props();

  const app = getApp();

  function ruleFor(category: Category) {
    return settings.categories.find((r) => r.category === category);
  }

  async function pickCategoryFolder(category: Category) {
    const picked = await app.pickFolder(ruleFor(category)?.folder);
    if (!picked) return;
    save((d) => {
      const rule = d.categories.find((r) => r.category === category);
      if (rule) rule.folder = picked;
      else d.categories.push({ category, extensions: [], folder: picked });
    });
  }

  async function pickDefaultFolder() {
    const picked = await app.pickFolder(settings.default_folder);
    if (picked) save((d) => void (d.default_folder = picked));
  }

  function setExtensions(category: Category, exts: string[]) {
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

<SettingRows
  {items}
  {settings}
  {flash}
  {controls}
  stacked={['categories']}
  unlabelled={['default_folder', 'categories']}
/>

{#snippet defaultFolder({ s, control }: Row)}
  <span
    class="max-w-72 truncate text-ui text-ink-muted"
    {@attach tooltip(() => displayPath(s.default_folder))}>{displayPath(s.default_folder)}</span
  >
  <Button
    size="sm"
    aria-label="Change the download folder"
    aria-describedby={control['aria-describedby']}
    onclick={pickDefaultFolder}>Change</Button
  >
{/snippet}

{#snippet categories({ control }: Row)}
  <div
    role="group"
    aria-label="Folders by file type"
    aria-describedby={control['aria-describedby']}
    class="flex w-full flex-col divide-y divide-line rounded-md border border-line"
  >
    {#each CATEGORIES.filter((c) => c !== 'other') as category (category)}
      {@const rule = ruleFor(category)}
      {@const name = CATEGORY_LABEL[category]}
      <div class="flex flex-wrap items-center gap-x-3 gap-y-2 px-3 py-2">
        <span class="w-32 shrink-0 truncate text-ui font-medium text-ink">{name}</span>
        <TagInput
          class="min-w-48 flex-1"
          label="Extensions for {name.toLowerCase()}"
          values={rule?.extensions ?? []}
          placeholder="Add extensions"
          normalize={typedExtension}
          onchange={(exts: string[]) => setExtensions(category, exts)}
        />
        <span class="ml-auto flex w-56 min-w-0 items-center justify-end gap-2">
          <span
            class="truncate text-small text-ink-muted"
            {@attach tooltip(() => textIfCut(rule ? displayPath(rule.folder) : 'Download folder'))}
            >{rule ? displayPath(rule.folder) : 'Download folder'}</span
          >
          <Button
            size="sm"
            aria-label="Change folder for {name.toLowerCase()}"
            onclick={() => pickCategoryFolder(category)}>Change</Button
          >
        </span>
      </div>
    {/each}
  </div>
{/snippet}
