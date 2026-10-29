<script lang="ts">
  // General: the theme and accent, how Wirecatch starts and closes, what updates itself, and settings saved to or loaded from a file (D85).
  import Check from '~icons/lucide/check';
  import { ACCENTS, type Accent, type Theme } from '@wirecatch/protocol';
  import type { Snippet } from 'svelte';
  import { Button, cn, Icon, SegmentedControl, tooltip } from '@wirecatch/ui';
  import { findElement } from '$lib/dom';
  import { suffixListNote } from '$lib/format';
  import { getApp } from '$lib/state/app.svelte';
  import type { SettingIn } from '$lib/settings-schema';
  import type { Row, SectionProps } from './row';
  import SettingRows from './SettingRows.svelte';
  import SettingToggle from './SettingToggle.svelte';

  let { items, settings, flash, save }: SectionProps = $props();
  const app = getApp();
  const suffixes = $derived(app.suffixList);

  /** Accents too close to another to offer side by side (D85.9); one already chosen stays offered. */
  const LOOKALIKES: ReadonlySet<Accent> = new Set<Accent>(['teal', 'indigo']);

  /** The accents offered: "Match system" only where the system has an accent colour (Windows), or while it is the choice. */
  function offeredAccents(current: Accent): readonly Accent[] {
    const system = app.appearance.systemAccent !== null || current === 'system';
    return ACCENTS.filter((a) => a === current || (a === 'system' ? system : !LOOKALIKES.has(a)));
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
    auto_update_addons: autoUpdate,
    update_suffix_list: suffixList,
    export: exportButton,
    import: importButton,
    log_folder: logFolder,
  };
</script>

<SettingRows
  {items}
  {settings}
  {flash}
  {controls}
  notes={{ update_suffix_list: suffixListState }}
  unlabelled={['export', 'import', 'log_folder']}
/>

{#snippet theme({ def, s }: Row)}
  <SegmentedControl
    value={s.theme}
    options={THEMES}
    label={def.label}
    size="sm"
    onchange={(value: Theme) => save((d) => void (d.theme = value))}
  />
{/snippet}

{#snippet accent({ def, s, control }: Row)}
  <div
    role="radiogroup"
    aria-label={def.label}
    aria-describedby={control['aria-describedby']}
    class="flex flex-wrap items-center gap-2"
  >
    {#each offeredAccents(s.accent) as a (a)}
      {const on = $derived(s.accent === a)}
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

{#snippet startWithSystem({ def, s, control }: Row)}
  <SettingToggle
    {control}
    label={def.label}
    checked={s.start_with_system}
    onchange={(v: boolean) => save((d) => void (d.start_with_system = v))}
  />
{/snippet}

{#snippet closeToTray({ def, s, control }: Row)}
  <SettingToggle
    {control}
    label={def.label}
    checked={s.close_to_tray}
    onchange={(v: boolean) => save((d) => void (d.close_to_tray = v))}
  />
{/snippet}

{#snippet autoUpdate({ def, s, control }: Row)}
  <SettingToggle
    {control}
    label={def.label}
    checked={s.auto_update_addons}
    onchange={(v: boolean) => save((d) => void (d.auto_update_addons = v))}
  />
{/snippet}

{#snippet suffixList({ def, s, control }: Row)}
  <SettingToggle
    {control}
    label={def.label}
    checked={s.update_suffix_list}
    onchange={(v: boolean) => save((d) => void (d.update_suffix_list = v))}
  />
{/snippet}

{#snippet suffixListState()}
  <div class="flex flex-wrap items-center gap-x-3 gap-y-2 px-4 pb-3">
    <span class="mr-auto text-small text-ink-muted">
      {#if suffixes.info}
        {suffixListNote(suffixes.info, app.clock.now)}
      {:else if suffixes.error}
        {suffixes.error}
      {:else}
        Reading the list.
      {/if}
    </span>
    <Button size="sm" loading={suffixes.checking} onclick={() => suffixes.check()}>Check now</Button
    >
  </div>
{/snippet}

{#snippet exportButton({ control }: Row)}
  <Button
    size="sm"
    aria-describedby={control['aria-describedby']}
    onclick={() => app.openDialog({ kind: 'export' })}>Export settings…</Button
  >
{/snippet}

{#snippet importButton({ control }: Row)}
  <Button
    size="sm"
    aria-describedby={control['aria-describedby']}
    onclick={() => app.openDialog({ kind: 'import' })}>Import settings…</Button
  >
{/snippet}

{#snippet logFolder({ control }: Row)}
  <Button
    size="sm"
    aria-describedby={control['aria-describedby']}
    onclick={() => void app.openLogFolder()}>Open log folder</Button
  >
{/snippet}
