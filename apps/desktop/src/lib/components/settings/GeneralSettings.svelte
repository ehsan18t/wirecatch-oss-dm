<script lang="ts">
  // General: the theme and accent, and how Wirecatch starts, closes and tells about finished files.
  import Check from '~icons/lucide/check';
  import { ACCENTS, type Accent, type Theme } from '@wirecatch/protocol';
  import type { Snippet } from 'svelte';
  import { cn, Icon, SegmentedControl, tooltip } from '@wirecatch/ui';
  import { findElement } from '$lib/dom';
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
    { value: 'system', label: 'System' },
    { value: 'light', label: 'Light' },
    { value: 'dark', label: 'Dark' },
  ];

  /** What each accent is called, for screen readers and the swatch's tooltip. */
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

  /** Arrow keys move the accent along, as in any group of radio buttons. */
  function onAccentKey(e: KeyboardEvent & { currentTarget: HTMLElement }, current: Accent) {
    const step = { ArrowRight: 1, ArrowDown: 1, ArrowLeft: -1, ArrowUp: -1 }[e.key];
    if (step === undefined) return;
    e.preventDefault();
    const offered = offeredAccents(current);
    const next = offered[(offered.indexOf(current) + step + offered.length) % offered.length];
    if (!next) return;
    save((d) => void (d.accent = next));
    const group = e.currentTarget.parentElement;
    queueMicrotask(() => findElement(group, `[data-accent="${next}"]`)?.focus());
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

{#snippet theme({ def, s }: Row)}
  <SegmentedControl
    value={s.theme}
    options={THEMES}
    label={def.label}
    size="sm"
    onchange={(value: Theme) => save((d) => void (d.theme = value))}
  />
{/snippet}

{#snippet accent({ s, labelId, helpId }: Row)}
  <div
    role="radiogroup"
    aria-labelledby={labelId}
    aria-describedby={helpId}
    class="flex flex-wrap items-center gap-2"
  >
    {#each offeredAccents(s.accent) as a (a)}
      {@const on = s.accent === a}
      <button
        type="button"
        role="radio"
        aria-checked={on}
        aria-label={ACCENT_NAMES[a]}
        tabindex={on ? 0 : -1}
        data-accent={a}
        class={cn(
          'flex size-5.5 items-center justify-center rounded-full bg-accent text-accent-fg ring-offset-2 ring-offset-raised transition duration-fast ease-out active:scale-pressed',
          on ? 'ring-2 ring-ink' : 'ring-1 ring-line-strong hover:ring-ink-muted',
        )}
        {@attach tooltip(() => ACCENT_NAMES[a])}
        onclick={() => save((d) => void (d.accent = a))}
        onkeydown={(e) => onAccentKey(e, s.accent)}
      >
        {#if on}<Icon icon={Check} size="xs" />{/if}
      </button>
    {/each}
  </div>
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
