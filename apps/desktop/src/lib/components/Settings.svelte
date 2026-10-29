<script lang="ts" module>
  // The settings page (D47): one section at a time, picked in the navigation, which becomes the list of sections while settings show. Its search, the title bar's while settings show, spans every section: matches show grouped by section, and clearing it returns to the section that was open. Every section shows its own settings, and must show each one (see SettingRows).

  /** How long a setting opened from elsewhere stays highlighted, so the eye finds it. */
  export const FLASH_MS = 1600;
</script>

<script lang="ts">
  import SearchX from '~icons/lucide/search-x';
  import { tick, type Component } from 'svelte';
  import { Button, cn, EmptyState } from '@wirecatch/ui';
  import { findElement } from '$lib/dom';
  import {
    searchSettings,
    settingAnchor,
    type SettingDef,
    type SettingId,
  } from '$lib/settings-schema';
  import { SECTIONS, type SectionId } from '$lib/settings-sections';
  import { getApp } from '$lib/state/app.svelte';
  import BrowserSettings from './settings/BrowserSettings.svelte';
  import FileSettings from './settings/FileSettings.svelte';
  import GeneralSettings from './settings/GeneralSettings.svelte';
  import MediaSettings from './settings/MediaSettings.svelte';
  import NetworkSettings from './settings/NetworkSettings.svelte';
  import NotificationSettings from './settings/NotificationSettings.svelte';
  import SpeedSettings from './settings/SpeedSettings.svelte';
  import { setSettingsSearching, type SectionProps } from './settings/row';

  const app = getApp();

  /** The component that shows each section's settings. */
  const SECTION_VIEW: Record<SectionId, Component<SectionProps>> = {
    general: GeneralSettings,
    notifications: NotificationSettings,
    files: FileSettings,
    browser: BrowserSettings,
    media: MediaSettings,
    speed: SpeedSettings,
    network: NetworkSettings,
  };

  /** The page has scrolled under its header, which then draws its edge. */
  let scrolled = $state(false);

  /** The setting highlighted, as a new object for each jump, so opening it again starts over. */
  let flash = $state<{ id: SettingId } | null>(null);

  const query = $derived(app.settingsQuery.trim());
  const searching = $derived(query !== '');
  setSettingsSearching(() => searching);
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
      findElement(el, 'button:not([data-info-tip]), input, select, textarea')?.focus({
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
  <div
    class={cn(
      'flex shrink-0 flex-col gap-0.5 border-b px-5 pt-4 pb-3 transition-colors duration-fast',
      scrolled ? 'border-line' : 'border-transparent',
    )}
  >
    <h1
      id="settings-title"
      class="truncate font-display text-title font-semibold tracking-display text-ink"
    >
      {searching ? 'Search results' : section?.title}
    </h1>
    <p class="truncate text-small text-ink-muted">
      {#if searching}
        {matches.length === 1 ? '1 setting matches' : `${String(matches.length)} settings match`}
        “{query}”
      {:else}
        {section?.blurb}
      {/if}
    </p>
  </div>

  <div
    class="min-h-0 flex-1 overflow-y-auto px-5 pt-3 pb-12"
    onscroll={(e) => (scrolled = e.currentTarget.scrollTop > 0)}
  >
    {#if !app.settings}
      <p class="text-small text-ink-muted">Loading settings.</p>
    {:else if matches.length === 0}
      <EmptyState
        icon={SearchX}
        tone="neutral"
        title="No setting matches “{query}”"
        description="Try a word like folder, speed or subtitles."
      >
        {#snippet actions()}
          <Button onclick={() => (app.settingsQuery = '')}>Clear the search</Button>
        {/snippet}
      </EmptyState>
    {:else}
      {#key searching ? 'search' : app.settingsSection}
        <div class="flex max-w-page animate-rise-in flex-col gap-6">
          {#each SECTIONS as s (s.id)}
            {const items = $derived(itemsIn(s.id))}
            {const View = $derived(SECTION_VIEW[s.id])}
            {#if items.length > 0}
              <section
                aria-labelledby={searching ? `sec-${s.id}` : 'settings-title'}
                class="flex flex-col gap-2"
              >
                {#if searching}
                  <h2 id="sec-{s.id}" class="px-1 text-ui font-semibold text-ink">
                    {s.title}
                  </h2>
                {/if}
                <View
                  {items}
                  settings={app.settings}
                  flash={flash?.id ?? null}
                  save={(change) => void app.updateSettings(change)}
                />
              </section>
            {/if}
          {/each}
        </div>
      {/key}
    {/if}
  </div>
</div>
