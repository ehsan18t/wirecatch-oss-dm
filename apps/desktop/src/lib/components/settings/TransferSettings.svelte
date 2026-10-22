<script lang="ts">
  // Move to another PC: settings saved to a file, and loaded from one (which shows what it would change first).
  import type { Snippet } from 'svelte';
  import { Button } from '@wirecatch/ui';
  import type { SettingIn } from '$lib/settings-schema';
  import { getApp } from '$lib/state/app.svelte';
  import type { Row, SectionProps } from './row';
  import SettingRows from './SettingRows.svelte';

  let { items, settings, flash }: SectionProps = $props();

  const app = getApp();

  const controls: Record<SettingIn<'transfer'>, Snippet<[Row]>> = {
    export: exportButton,
    import: importButton,
  };
</script>

<SettingRows {items} {settings} {flash} {controls} unlabelled={['export', 'import']} />

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
