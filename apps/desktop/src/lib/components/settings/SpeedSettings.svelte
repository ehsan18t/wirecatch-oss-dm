<script lang="ts">
  // Speed: a limit on all downloads together.
  import { inUnit, MB, toBytes } from '@wirecatch/units';
  import type { Snippet } from 'svelte';
  import type { SettingIn } from '$lib/settings-schema';
  import Toggle from '../Toggle.svelte';
  import type { Row, SectionProps } from './row';
  import SettingRows from './SettingRows.svelte';

  let { items, settings, flash, save }: SectionProps = $props();

  /** The total speed limit a person starts from when turning it on; a suggestion, not a default of the core. */
  const FIRST_SPEED_LIMIT = 5 * MB;

  const controls: Record<SettingIn<'speed'>, Snippet<[Row]>> = {
    global_speed_limit: speedLimit,
  };
</script>

<SettingRows {items} {settings} {flash} {controls} />

{#snippet speedLimit({ s, helpId }: Row)}
  <Toggle
    label="Limit total speed"
    describedby={helpId}
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
{/snippet}
