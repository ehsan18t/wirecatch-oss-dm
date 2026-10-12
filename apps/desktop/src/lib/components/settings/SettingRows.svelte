<script lang="ts">
  // The rows of one settings section: each setting's name and help, and its control. A section
  // passes a control for each of its settings, typed so that none can be left out.
  import type { Snippet } from 'svelte';
  import { settingAnchor, type SettingId } from '$lib/settings-schema';
  import type { Row, SectionProps } from './row';

  let {
    items,
    settings,
    flash,
    controls,
  }: SectionProps & {
    /** Each setting's control. */
    controls: Partial<Record<SettingId, Snippet<[Row]>>>;
  } = $props();
</script>

{#each items as def (def.id)}
  {@const help = `help-${def.id}`}
  {@const control = controls[def.id]}
  <div class="setting" class:flash={flash === def.id} id={settingAnchor(def.id)}>
    <div class="text">
      <span class="label" id="label-{def.id}">{def.label}</span>
      <span class="help" id={help}>{def.help}</span>
    </div>
    <div class="control">
      {#if control}{@render control({ def, s: settings, help })}{/if}
    </div>
  </div>
{/each}

<style>
  .setting {
    display: grid;
    grid-template-columns: minmax(220px, 1fr) minmax(0, 1.2fr);
    align-items: center;
    gap: 8px 28px;
    margin: 0 -10px;
    padding: 10px;
    border-radius: var(--radius-m);
    transition: background 400ms var(--ease);
  }

  .setting.flash {
    background: var(--accent-soft);
  }

  .setting:has(:global(table)) {
    grid-template-columns: 1fr;
  }

  .text {
    display: grid;
    gap: 2px;
  }

  .label {
    font-weight: 600;
  }

  .help {
    color: var(--ink-2);
    font-size: var(--text-sm);
  }

  .control {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 8px;
  }

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
