<script lang="ts">
  // The settings screen: its search, and each section that has a setting matching it. Every
  // section shows its own settings, and must show each one (see SettingRows).
  import ArrowLeft from '@lucide/svelte/icons/arrow-left';
  import Search from '@lucide/svelte/icons/search';
  import { tick, type Component } from 'svelte';
  import {
    SECTIONS,
    searchSettings,
    settingAnchor,
    type SectionId,
    type SettingDef,
    type SettingId,
  } from '$lib/settings-schema';
  import { getApp } from '$lib/state/app.svelte';
  import Button from './Button.svelte';
  import Icon from './Icon.svelte';
  import AddonSettings from './settings/AddonSettings.svelte';
  import AdvancedSettings from './settings/AdvancedSettings.svelte';
  import BrowserSettings from './settings/BrowserSettings.svelte';
  import FileSettings from './settings/FileSettings.svelte';
  import GeneralSettings from './settings/GeneralSettings.svelte';
  import MediaSettings from './settings/MediaSettings.svelte';
  import SpeedSettings from './settings/SpeedSettings.svelte';
  import type { SectionProps } from './settings/row';
  import TransferSettings from './settings/TransferSettings.svelte';

  const app = getApp();

  /** How long a setting opened from elsewhere stays highlighted, so the eye finds it. */
  const FLASH_MS = 1600;

  /** The component that shows each section's settings. */
  const SECTION_VIEW: Record<SectionId, Component<SectionProps>> = {
    general: GeneralSettings,
    files: FileSettings,
    browser: BrowserSettings,
    media: MediaSettings,
    addons: AddonSettings,
    speed: SpeedSettings,
    transfer: TransferSettings,
    advanced: AdvancedSettings,
  };

  let query = $state('');
  /** The setting highlighted, as a new object for each jump, so opening it again starts over. */
  let flash = $state<{ id: SettingId } | null>(null);

  const matches = $derived(searchSettings(query));

  function itemsIn(section: SectionId): SettingDef[] {
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
    {#if !app.settings}
      <p class="muted">Loading settings.</p>
    {:else if matches.length === 0}
      <div class="none">
        <p>No setting matches "{query}".</p>
        <Button onclick={() => (query = '')}>Clear search</Button>
      </div>
    {:else}
      {#each SECTIONS as section (section.id)}
        {@const items = itemsIn(section.id)}
        {@const View = SECTION_VIEW[section.id]}
        {#if items.length > 0}
          <section aria-labelledby="sec-{section.id}">
            <h2 id="sec-{section.id}">{section.title}</h2>
            <p class="blurb">{section.blurb}</p>
            <View
              {items}
              settings={app.settings}
              flash={flash?.id ?? null}
              save={(change) => void app.updateSettings(change)}
            />
          </section>
        {/if}
      {/each}
    {/if}
  </div>
</div>

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

  .none {
    padding: 40px 0;
    color: var(--ink-2);
  }
</style>
