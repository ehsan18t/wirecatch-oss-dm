<script lang="ts">
  // Add-ons: the media add-on, and whether it keeps itself up to date.
  import type { Snippet } from 'svelte';
  import { MEDIA_ADDON } from '$lib/addons';
  import type { SettingIn } from '$lib/settings-schema';
  import { getApp } from '$lib/state/app.svelte';
  import AddonCard from '../AddonCard.svelte';
  import Button from '../Button.svelte';
  import Toggle from '../Toggle.svelte';
  import type { Row, SectionProps } from './row';
  import SettingRows from './SettingRows.svelte';

  let { items, settings, flash, save }: SectionProps = $props();

  const app = getApp();

  const addon = $derived(app.addons.list?.find((a) => a.id === MEDIA_ADDON));

  const controls: Record<SettingIn<'addons'>, Snippet<[Row]>> = {
    media_addon: mediaAddon,
    auto_update_addons: autoUpdate,
  };
</script>

<SettingRows {items} {settings} {flash} {controls} />

{#snippet mediaAddon({ help }: Row)}
  {#if addon}
    <AddonCard {addon} describedby={help} />
  {:else if app.addons.error}
    <p class="addon-note">
      {app.addons.error}
      <Button small onclick={() => app.addons.load()}>Check again</Button>
    </p>
  {:else}
    <p class="addon-note">Checking.</p>
  {/if}
{/snippet}

{#snippet autoUpdate({ def, s, help }: Row)}
  <Toggle
    label={def.label}
    describedby={help}
    checked={s.auto_update_addons}
    onchange={(v: boolean) => save((d) => void (d.auto_update_addons = v))}
  />
{/snippet}

<style>
  .addon-note {
    display: flex;
    align-items: center;
    gap: 8px;
    max-width: 320px;
    margin: 0;
    color: var(--ink-2);
    font-size: var(--text-sm);
  }
</style>
