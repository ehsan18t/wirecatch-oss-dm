<script lang="ts" module>
  // The settings page (D47): one section at a time, picked in the navigation, which becomes the list of sections while settings show. Its search spans every section: matches show grouped by section, and clearing it returns to the section that was open. Every section shows its own settings, and must show each one (see SettingRows).

  /** How long a setting opened from elsewhere stays highlighted, so the eye finds it. */
  export const FLASH_MS = 1600;
</script>

<script lang="ts">
  import Search from '~icons/lucide/search';
  import { tick, type Component } from 'svelte';
  import { Button, Input } from '@wirecatch/ui';
  import { findElement } from '$lib/dom';
  import {
    SECTIONS,
    searchSettings,
    settingAnchor,
    type SectionId,
    type SettingDef,
    type SettingId,
  } from '$lib/settings-schema';
  import { getApp } from '$lib/state/app.svelte';
  import AddonSettings from './settings/AddonSettings.svelte';
  import AdvancedSettings from './settings/AdvancedSettings.svelte';
  import BrowserSettings from './settings/BrowserSettings.svelte';
  import FileSettings from './settings/FileSettings.svelte';
  import GeneralSettings from './settings/GeneralSettings.svelte';
  import MediaSettings from './settings/MediaSettings.svelte';
  import QueueSettings from './settings/QueueSettings.svelte';
  import SpeedSettings from './settings/SpeedSettings.svelte';
  import type { SectionProps } from './settings/row';
  import TransferSettings from './settings/TransferSettings.svelte';

  const app = getApp();

  /** The component that shows each section's settings. */
  const SECTION_VIEW: Record<SectionId, Component<SectionProps>> = {
    general: GeneralSettings,
    files: FileSettings,
    browser: BrowserSettings,
    media: MediaSettings,
    addons: AddonSettings,
    speed: SpeedSettings,
    queues: QueueSettings,
    transfer: TransferSettings,
    advanced: AdvancedSettings,
  };

  /** The setting highlighted, as a new object for each jump, so opening it again starts over. */
  let flash = $state<{ id: SettingId } | null>(null);

  const query = $derived(app.settingsQuery.trim());
  const searching = $derived(query !== '');
  const matches = $derived(searchSettings(query));
  const section = $derived(SECTIONS.find((s) => s.id === app.settingsSection) ?? SECTIONS[0]);

  /** The settings each section shows: every match while searching, else the open section's all. */
  function itemsIn(id: SectionId): SettingDef[] {
    if (searching) return matches.filter((m) => m.section === id);
    return id === app.settingsSection ? matches.filter((m) => m.section === id) : [];
  }

  // Jump to a setting asked for from the command palette.
  $effect(() => {
    const target = app.settingsFocus;
    if (!target) return;
    void tick().then(() => {
      const el = document.getElementById(settingAnchor(target));
      el?.scrollIntoView({ block: 'center' });
      findElement(el, 'button, input, select, textarea')?.focus({
        preventScroll: true,
      });
      flash = { id: target };
      app.settingsFocus = null;
    });
  });

  // The highlight fades after a while; a newer jump or leaving the screen stops the old timer.
  $effect(() => {
    if (!flash) return;
    const timer = setTimeout(() => (flash = null), FLASH_MS);
    return () => clearTimeout(timer);
  });
</script>

<div class="flex min-h-0 flex-1 flex-col">
  <header class="flex shrink-0 flex-wrap items-end gap-x-6 gap-y-3 px-5 pt-4 pb-3">
    <div class="flex min-w-0 flex-1 flex-col gap-0.5">
      <h1
        id="settings-title"
        class="truncate font-display text-title font-semibold tracking-display text-ink"
      >
        {searching ? 'Search results' : section?.title}
      </h1>
      <p class="text-small text-ink-muted">
        {#if searching}
          {matches.length === 1 ? '1 setting matches' : `${String(matches.length)} settings match`}
          "{query}"
        {:else}
          {section?.blurb} Changes save as you make them.
        {/if}
      </p>
    </div>
    <Input
      id="settings-search"
      type="search"
      size="sm"
      icon={Search}
      label="Search settings"
      placeholder="Search all settings, for example proxy"
      class="w-full max-w-80"
      value={app.settingsQuery}
      onchange={(value: string) => (app.settingsQuery = value)}
    />
  </header>

  <div class="min-h-0 flex-1 overflow-y-auto px-5 pb-12">
    {#if !app.settings}
      <p class="text-small text-ink-muted">Loading settings.</p>
    {:else if matches.length === 0}
      <div class="flex flex-col items-start gap-3 py-8 text-ink-muted">
        <p>No setting matches "{query}". Try a word like folder, speed or subtitles.</p>
        <Button onclick={() => (app.settingsQuery = '')}>Clear the search</Button>
      </div>
    {:else}
      {#key searching ? 'search' : app.settingsSection}
        <div class="flex max-w-page animate-rise-in flex-col gap-5">
          {#each SECTIONS as s (s.id)}
            {@const items = itemsIn(s.id)}
            {@const View = SECTION_VIEW[s.id]}
            {#if items.length > 0}
              <section
                aria-labelledby={searching ? `sec-${s.id}` : 'settings-title'}
                class="flex flex-col gap-2"
              >
                {#if searching}
                  <h2
                    id="sec-{s.id}"
                    class="pt-2 text-caption font-semibold tracking-caps text-ink-subtle uppercase"
                  >
                    {s.title}
                  </h2>
                {/if}
                <div class="rounded-lg border border-line bg-raised shadow-raised">
                  <View
                    {items}
                    settings={app.settings}
                    flash={flash?.id ?? null}
                    save={(change) => void app.updateSettings(change)}
                  />
                </div>
              </section>
            {/if}
          {/each}
        </div>
      {/key}
    {/if}
  </div>
</div>
