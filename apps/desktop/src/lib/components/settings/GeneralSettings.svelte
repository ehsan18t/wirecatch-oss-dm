<script lang="ts">
  // General: the theme, and how Wirecatch starts, closes and tells about finished files.
  import type { AppSettings, Theme } from '@wirecatch/protocol';
  import { tick, type Snippet } from 'svelte';
  import type { SettingIn } from '$lib/settings-schema';
  import { getApp } from '$lib/state/app.svelte';
  import Toggle from '../Toggle.svelte';
  import type { Row, SectionProps } from './row';
  import SettingRows from './SettingRows.svelte';

  let { items, settings, flash }: SectionProps = $props();

  const app = getApp();

  const THEMES: { value: Theme; label: string }[] = [
    { value: 'system', label: 'Match system' },
    { value: 'light', label: 'Light' },
    { value: 'dark', label: 'Dark' },
  ];

  function save(change: (d: AppSettings) => void) {
    void app.updateSettings(change);
  }

  /** Arrow keys move the theme choice along, as in any group of radio buttons. */
  function onThemeKey(e: KeyboardEvent & { currentTarget: HTMLElement }, current: Theme) {
    if (e.key !== 'ArrowRight' && e.key !== 'ArrowLeft') return;
    e.preventDefault();
    const i = THEMES.findIndex((x) => x.value === current);
    const next = THEMES[(i + (e.key === 'ArrowRight' ? 1 : THEMES.length - 1)) % THEMES.length];
    if (!next) return;
    save((d) => void (d.theme = next.value));
    const group = e.currentTarget.parentElement;
    void tick().then(() => group?.querySelector<HTMLElement>('[aria-checked="true"]')?.focus());
  }

  const controls: Record<SettingIn<'general'>, Snippet<[Row]>> = {
    theme,
    start_with_system: startWithSystem,
    close_to_tray: closeToTray,
    notify_on_complete: notifyOnComplete,
  };
</script>

<SettingRows {items} {settings} {flash} {controls} />

{#snippet theme({ s, help }: Row)}
  <div class="segmented" role="radiogroup" aria-labelledby="label-theme" aria-describedby={help}>
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

{#snippet startWithSystem({ def, s, help }: Row)}
  <Toggle
    label={def.label}
    describedby={help}
    checked={s.start_with_system}
    onchange={(v: boolean) => save((d) => void (d.start_with_system = v))}
  />
{/snippet}

{#snippet closeToTray({ def, s, help }: Row)}
  <Toggle
    label={def.label}
    describedby={help}
    checked={s.close_to_tray}
    onchange={(v: boolean) => save((d) => void (d.close_to_tray = v))}
  />
{/snippet}

{#snippet notifyOnComplete({ def, s, help }: Row)}
  <Toggle
    label={def.label}
    describedby={help}
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
