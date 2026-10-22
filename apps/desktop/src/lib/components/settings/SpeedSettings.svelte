<script lang="ts">
  // Speed and limits: a limit on all downloads together, how many run at once across the queues, and how many connections each opens, in the ranges the core accepts.
  import { MAX_CONCURRENT_DOWNLOADS, MAX_CONNECTIONS_PER_DOWNLOAD } from '@wirecatch/protocol';
  import { inUnit, MB, toBytes } from '@wirecatch/units';
  import type { Snippet } from 'svelte';
  import { NumberInput } from '@wirecatch/ui';
  import { SPEED_FIELD } from '$lib/limits';
  import type { SettingIn } from '$lib/settings-schema';
  import type { Row, SectionProps } from './row';
  import SettingRows from './SettingRows.svelte';
  import SettingToggle from './SettingToggle.svelte';

  let { items, settings, flash, save }: SectionProps = $props();

  /** The total speed limit a person starts from when turning it on; a suggestion, not a default of the core. */
  const FIRST_SPEED_LIMIT = 5 * MB;

  const controls: Record<SettingIn<'speed'>, Snippet<[Row]>> = {
    global_speed_limit: speedLimit,
    max_concurrent_downloads: concurrent,
    connections_per_download: connections,
  };
</script>

<SettingRows {items} {settings} {flash} {controls} />

{#snippet speedLimit({ def, s, control }: Row)}
  {#if s.global_speed_limit !== null}
    <NumberInput
      label="Total speed limit"
      class="w-32"
      unit="MB/s"
      min={SPEED_FIELD.MB.min}
      step={SPEED_FIELD.MB.step}
      value={inUnit(s.global_speed_limit, 'MB')}
      onchange={(value: number | null) => {
        const bytes = value === null ? null : toBytes(value, 'MB');
        if (bytes !== null && bytes > 0) save((d) => void (d.global_speed_limit = bytes));
      }}
    />
  {/if}
  <SettingToggle
    {control}
    label={def.label}
    checked={s.global_speed_limit !== null}
    onchange={(v: boolean) =>
      save((d) => void (d.global_speed_limit = v ? FIRST_SPEED_LIMIT : null))}
  />
{/snippet}

{#snippet concurrent({ s, control }: Row)}
  <NumberInput
    {...control}
    class="w-28"
    min={1}
    max={MAX_CONCURRENT_DOWNLOADS}
    value={s.max_concurrent_downloads}
    onchange={(value: number | null) => {
      if (value !== null) save((d) => void (d.max_concurrent_downloads = value));
    }}
  />
{/snippet}

{#snippet connections({ s, control }: Row)}
  <NumberInput
    {...control}
    class="w-28"
    min={1}
    max={MAX_CONNECTIONS_PER_DOWNLOAD}
    value={s.connections_per_download}
    onchange={(value: number | null) => {
      if (value !== null) save((d) => void (d.connections_per_download = value));
    }}
  />
{/snippet}
