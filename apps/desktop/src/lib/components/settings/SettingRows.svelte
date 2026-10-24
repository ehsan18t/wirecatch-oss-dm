<script lang="ts">
  // The rows of one settings section, on the design system's setting rows: each setting's name and help, and its control at the row's end, or under them for a control that needs the width (a list, a table). Settings with a group sit on a card of their own under its heading. This takes the controls by any setting id; each section builds them as a Record of its own settings (SettingIn), which is what makes leaving one out a type error.
  import type { Snippet } from 'svelte';
  import { cn, SettingRow, type FieldControl } from '@wirecatch/ui';
  import { settingAnchor, type SettingDef, type SettingId } from '$lib/settings-schema';
  import type { Row, SectionProps } from './row';

  let {
    items,
    settings,
    flash,
    controls,
    stacked = [],
    unlabelled = [],
  }: Omit<SectionProps, 'save'> & {
    /** Each setting's control. */
    controls: Partial<Record<SettingId, Snippet<[Row]>>>;
    /** The settings whose control goes under their name, across the row. */
    stacked?: readonly SettingId[];
    /** The settings whose end is a button with its own name (Change, Export settings…), which the setting's name must not replace. */
    unlabelled?: readonly SettingId[];
  } = $props();

  /** The settings in runs that share a group, in order. */
  const groups = $derived.by(() => {
    const runs: { group: string | undefined; defs: SettingDef[] }[] = [];
    for (const def of items) {
      const last = runs.at(-1);
      if (last && last.group === def.group) last.defs.push(def);
      else runs.push({ group: def.group, defs: [def] });
    }
    return runs;
  });
</script>

<div class="flex flex-col gap-5">
  {#each groups as run, index (`${String(index)}-${run.group ?? ''}`)}
    <div class="flex flex-col gap-2">
      {#if run.group}
        <h3 class="px-1 text-ui font-semibold text-ink">{run.group}</h3>
      {/if}
      <div
        class="flex flex-col divide-y divide-line rounded-lg border border-line bg-raised shadow-raised forced-colors:border-sys-text"
      >
        {#each run.defs as def (def.id)}
          {const control = $derived(controls[def.id])}
          <div
            id={settingAnchor(def.id)}
            class={cn(
              'setting transition-colors duration-slower ease-out first:rounded-t-lg last:rounded-b-lg',
              flash === def.id && 'flash bg-accent-soft',
            )}
          >
            <SettingRow
              label={def.label}
              description={def.help}
              stacked={stacked.includes(def.id)}
              labelled={!unlabelled.includes(def.id)}
            >
              {#snippet children(field: FieldControl)}
                {#if control}{@render control({ def, s: settings, control: field })}{/if}
              {/snippet}
            </SettingRow>
          </div>
        {/each}
      </div>
    </div>
  {/each}
</div>
