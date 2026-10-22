<script lang="ts">
  // Advanced: the person's sites on their network (D39), and the list of public domains that decides which sites are one owner's (D44).
  import { normalizeSite } from '@wirecatch/protocol';
  import type { Snippet } from 'svelte';
  import { Button, TagInput } from '@wirecatch/ui';
  import { suffixListNote } from '$lib/format';
  import type { SettingIn } from '$lib/settings-schema';
  import { getApp } from '$lib/state/app.svelte';
  import type { Row, SectionProps } from './row';
  import SettingRows from './SettingRows.svelte';
  import SettingToggle from './SettingToggle.svelte';

  let { items, settings, flash, save }: SectionProps = $props();

  const app = getApp();
  const suffixes = $derived(app.suffixList);

  const controls: Record<SettingIn<'advanced'>, Snippet<[Row]>> = {
    local_sites: localSites,
    update_suffix_list: suffixList,
  };
</script>

<SettingRows
  {items}
  {settings}
  {flash}
  {controls}
  stacked={['local_sites', 'update_suffix_list']}
/>

{#snippet localSites({ def, s, control }: Row)}
  <TagInput
    {...control}
    label={def.label}
    placeholder="Add a site, like wiki.example.com"
    values={s.local_sites}
    normalize={normalizeSite}
    onchange={(sites: string[]) => save((d) => void (d.local_sites = sites))}
  />
{/snippet}

{#snippet suffixList({ def, s, control }: Row)}
  <div class="flex w-full flex-wrap items-center gap-x-3 gap-y-2">
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
    <SettingToggle
      {control}
      label={def.label}
      checked={s.update_suffix_list}
      onchange={(v: boolean) => save((d) => void (d.update_suffix_list = v))}
    />
  </div>
{/snippet}
