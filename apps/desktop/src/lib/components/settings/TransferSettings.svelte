<script lang="ts">
  // Move to another PC: settings saved to a file, and loaded from one.
  import type { Snippet } from 'svelte';
  import type { SettingIn } from '$lib/settings-schema';
  import { getApp } from '$lib/state/app.svelte';
  import Button from '../Button.svelte';
  import type { Row, SectionProps } from './row';
  import SettingRows from './SettingRows.svelte';

  let { items, settings, flash }: SectionProps = $props();

  const app = getApp();

  const controls: Record<SettingIn<'transfer'>, Snippet<[Row]>> = {
    export: exportButton,
    import: importButton,
  };
</script>

<SettingRows {items} {settings} {flash} {controls} />

{#snippet exportButton({ help }: Row)}
  <Button aria-describedby={help} onclick={() => app.openDialog({ kind: 'export' })}
    >Export settings</Button
  >
{/snippet}

{#snippet importButton({ help }: Row)}
  <Button aria-describedby={help} onclick={() => app.openDialog({ kind: 'import' })}
    >Import settings</Button
  >
{/snippet}
