<script lang="ts">
  // The rows of one settings section: each setting's name and help, and its control. This takes
  // the controls by any setting id; each section builds them as a Record of its own settings
  // (SettingIn), which is what makes leaving one out a type error.
  import type { Snippet } from 'svelte';
  import { settingAnchor, type SettingId } from '$lib/settings-schema';
  import type { Row, SectionProps } from './row';

  let {
    items,
    settings,
    flash,
    controls,
  }: Omit<SectionProps, 'save'> & {
    /** Each setting's control. */
    controls: Partial<Record<SettingId, Snippet<[Row]>>>;
  } = $props();
</script>

{#each items as def (def.id)}
  {@const helpId = `help-${def.id}`}
  {@const labelId = `label-${def.id}`}
  {@const control = controls[def.id]}
  <div
    class={[
      'setting flex flex-wrap items-center justify-between gap-x-6 gap-y-2 border-t border-line px-4 py-3 transition-colors duration-slower ease-out first:border-t-0 has-[table]:flex-col has-[table]:items-stretch has-[table]:*:basis-auto has-[.list-editor]:flex-col has-[.list-editor]:items-stretch has-[.list-editor]:*:basis-auto',
      flash === def.id && 'flash bg-accent-soft',
    ]}
    id={settingAnchor(def.id)}
  >
    <div class="flex min-w-0 flex-1 basis-64 flex-col gap-0.5">
      <span class="font-medium text-ink" id={labelId}>{def.label}</span>
      <span class="max-w-prose text-small text-ink-subtle" id={helpId}>{def.help}</span>
    </div>
    <div
      class="control flex flex-wrap items-center justify-end gap-2 [:has(>.list-editor,>table)]:justify-start"
    >
      {#if control}{@render control({ def, s: settings, labelId, helpId })}{/if}
    </div>
  </div>
{/each}

<style>
  /* Shared by the controls the sections pass in. */
  .control :global(.path),
  .control :global(.unit) {
    color: var(--ink-2);
    font-size: var(--text-sm);
  }

  .control :global(.path) {
    overflow-wrap: anywhere;
  }

  .control :global(.narrow) {
    width: 90px;
  }
</style>
