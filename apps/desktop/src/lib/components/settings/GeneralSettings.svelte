<script lang="ts">
  // General: the theme and accent, and how Wirecatch starts, closes and tells about finished files.
  import { ACCENTS, type Accent, type Theme } from '@wirecatch/protocol';
  import { tick, type Snippet } from 'svelte';
  import { findElement } from '$lib/dom';
  import { offeredValue } from '$lib/select-value';
  import { getApp } from '$lib/state/app.svelte';
  import type { SettingIn } from '$lib/settings-schema';
  import Toggle from '../Toggle.svelte';
  import type { Row, SectionProps } from './row';
  import SettingRows from './SettingRows.svelte';

  let { items, settings, flash, save }: SectionProps = $props();
  const app = getApp();

  /** The accents offered: "Match system" only where the system has an accent colour (Windows), or while it is the choice. */
  function offeredAccents(current: Accent): readonly Accent[] {
    const system = app.appearance.systemAccent !== null || current === 'system';
    return ACCENTS.filter((a) => a !== 'system' || system);
  }

  const THEMES: { value: Theme; label: string }[] = [
    { value: 'system', label: 'Match system' },
    { value: 'light', label: 'Light' },
    { value: 'dark', label: 'Dark' },
  ];

  /** What each accent is called in the menu; the design unit's settings page replaces it with swatches. */
  const ACCENT_NAMES: Record<Accent, string> = {
    cyan: 'Cyan',
    purple: 'Purple',
    blue: 'Blue',
    green: 'Green',
    orange: 'Orange',
    pink: 'Pink',
    red: 'Red',
    teal: 'Teal',
    indigo: 'Indigo',
    amber: 'Amber',
    system: 'Match system',
  };

  /** Arrow keys move the theme choice along, as in any group of radio buttons. */
  function onThemeKey(e: KeyboardEvent & { currentTarget: HTMLElement }, current: Theme) {
    if (e.key !== 'ArrowRight' && e.key !== 'ArrowLeft') return;
    e.preventDefault();
    const i = THEMES.findIndex((x) => x.value === current);
    const next = THEMES[(i + (e.key === 'ArrowRight' ? 1 : THEMES.length - 1)) % THEMES.length];
    if (!next) return;
    save((d) => void (d.theme = next.value));
    const group = e.currentTarget.parentElement;
    void tick().then(() => findElement(group, '[aria-checked="true"]')?.focus());
  }

  const controls: Record<SettingIn<'general'>, Snippet<[Row]>> = {
    theme,
    accent,
    start_with_system: startWithSystem,
    close_to_tray: closeToTray,
    notify_on_complete: notifyOnComplete,
  };
</script>

<SettingRows {items} {settings} {flash} {controls} />

{#snippet theme({ s, labelId, helpId }: Row)}
  <div class="segmented" role="radiogroup" aria-labelledby={labelId} aria-describedby={helpId}>
    {#each THEMES as t (t.value)}
      <button
        type="button"
        role="radio"
        aria-checked={s.theme === t.value}
        tabindex={s.theme === t.value ? 0 : -1}
        onclick={() => save((d) => void (d.theme = t.value))}
        onkeydown={(e) => onThemeKey(e, s.theme)}
      >
        {t.label}
      </button>
    {/each}
  </div>
{/snippet}

{#snippet accent({ s, labelId, helpId }: Row)}
  <select
    class="field"
    aria-labelledby={labelId}
    aria-describedby={helpId}
    value={s.accent}
    onchange={(e) => {
      const chosen = offeredValue(offeredAccents(s.accent), e.currentTarget.value);
      if (chosen) save((d) => void (d.accent = chosen));
    }}
  >
    {#each offeredAccents(s.accent) as a (a)}
      <option value={a}>{ACCENT_NAMES[a]}</option>
    {/each}
  </select>
{/snippet}

{#snippet startWithSystem({ def, s, helpId }: Row)}
  <Toggle
    label={def.label}
    describedby={helpId}
    checked={s.start_with_system}
    onchange={(v: boolean) => save((d) => void (d.start_with_system = v))}
  />
{/snippet}

{#snippet closeToTray({ def, s, helpId }: Row)}
  <Toggle
    label={def.label}
    describedby={helpId}
    checked={s.close_to_tray}
    onchange={(v: boolean) => save((d) => void (d.close_to_tray = v))}
  />
{/snippet}

{#snippet notifyOnComplete({ def, s, helpId }: Row)}
  <Toggle
    label={def.label}
    describedby={helpId}
    checked={s.notify_on_complete}
    onchange={(v: boolean) => save((d) => void (d.notify_on_complete = v))}
  />
{/snippet}

<style>
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
</style>
