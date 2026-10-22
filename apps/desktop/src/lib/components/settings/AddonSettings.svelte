<script lang="ts">
  // Add-ons: the media add-on, and whether it keeps itself up to date (which only means something once it is installed).
  import type { Snippet } from 'svelte';
  import { Button } from '@wirecatch/ui';
  import { isInstalled, MEDIA_ADDON } from '$lib/addons';
  import type { SettingIn } from '$lib/settings-schema';
  import { getApp } from '$lib/state/app.svelte';
  import AddonCard from '../AddonCard.svelte';
  import type { Row, SectionProps } from './row';
  import SettingRows from './SettingRows.svelte';
  import SettingToggle from './SettingToggle.svelte';

  let { items, settings, flash, save }: SectionProps = $props();

  const app = getApp();

  const addon = $derived(app.addons.list?.find((a) => a.id === MEDIA_ADDON));

  const controls: Record<SettingIn<'addons'>, Snippet<[Row]>> = {
    media_addon: mediaAddon,
    auto_update_addons: autoUpdate,
  };
</script>

<SettingRows {items} {settings} {flash} {controls} />

{#snippet mediaAddon({ control }: Row)}
  {#if addon}
    <AddonCard {addon} describedby={control['aria-describedby']} />
  {:else if app.addons.error}
    <span class="text-small text-danger">{app.addons.error}</span>
    <Button size="sm" onclick={() => app.addons.load()}>Check again</Button>
  {:else}
    <span class="text-small text-ink-muted">Checking.</span>
  {/if}
{/snippet}

{#snippet autoUpdate({ def, s, control }: Row)}
  <SettingToggle
    {control}
    label={def.label}
    checked={s.auto_update_addons}
    disabled={!isInstalled(addon)}
    onchange={(v: boolean) => save((d) => void (d.auto_update_addons = v))}
  />
{/snippet}
