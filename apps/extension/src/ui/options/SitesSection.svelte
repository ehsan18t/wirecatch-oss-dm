<script lang="ts">
  import { normalizeSite, type ExtensionSettings } from '@wirecatch/protocol';
  import {
    Button,
    IconButton,
    Input,
    SettingRow,
    SettingsSection,
    type FieldControl,
  } from '@wirecatch/ui';
  import X from '~icons/lucide/x';
  import type { PopupModel } from '../../lib/popup/model.svelte';
  import { buttonsIn, RemovalFocus } from '../removal-focus.svelte';
  import { SETTINGS, type SettingKey } from '../settings';

  interface Props {
    title: string;
    model: PopupModel;
    settings: ExtensionSettings;
    show: (key: SettingKey) => boolean;
  }

  let { title, model, settings, show }: Props = $props();

  const editable = $derived(model.editable);
  const sites = $derived(settings.ignored_sites);
  let draft = $state('');
  let error = $state<string | null>(null);
  let list = $state<HTMLUListElement | null>(null);
  let field = $state<HTMLInputElement | null>(null);
  const focus = new RemovalFocus({
    has: (site) => sites.includes(site),
    buttons: () => buttonsIn(list),
    fallback: () => field,
  });

  function remove(site: string, index: number): void {
    focus.removing(site, index);
    void model.update({ remove_ignored_sites: [site] });
  }

  function add(event: SubmitEvent): void {
    event.preventDefault();
    const site = normalizeSite(draft);
    if (site === null) {
      error = 'Type a site such as example.com.';
      return;
    }
    error = null;
    draft = '';
    // One site added rather than the list replaced, so an edit made at the same time in another browser is not overwritten.
    if (!sites.includes(site)) void model.update({ add_ignored_sites: [site] });
  }
</script>

<SettingsSection {title}>
  {#if show('ignoredSites')}
    <SettingRow
      stacked
      label={SETTINGS.ignoredSites.label}
      description={SETTINGS.ignoredSites.description}
      {error}
    >
      {#snippet children(control: FieldControl)}
        <div class="flex w-full flex-col gap-2.5">
          {#if sites.length === 0}
            <p class="text-ui text-ink-subtle">No sites ignored.</p>
          {:else}
            <ul
              bind:this={list}
              aria-label={SETTINGS.ignoredSites.label}
              class="flex max-h-48 flex-col divide-y divide-line overflow-y-auto rounded-md border border-line"
            >
              {#each sites as site, index (site)}
                <li class="flex items-center gap-2 py-1 pr-1 pl-3">
                  <span class="min-w-0 flex-1 truncate text-ui text-ink">{site}</span>
                  <IconButton
                    icon={X}
                    size="sm"
                    label="Stop ignoring {site}"
                    disabled={!editable}
                    onclick={() => {
                      remove(site, index);
                    }}
                  />
                </li>
              {/each}
            </ul>
          {/if}
          <form class="flex gap-2" onsubmit={add}>
            <Input
              {...control}
              bind:ref={field}
              label="Site to ignore"
              class="flex-1"
              inputmode="url"
              autocomplete="off"
              spellcheck="false"
              placeholder="example.com"
              disabled={!editable}
              value={draft}
              onchange={(value: string) => {
                draft = value;
                error = null;
              }}
            />
            <Button type="submit" disabled={!editable}>Add</Button>
          </form>
        </div>
      {/snippet}
    </SettingRow>
  {/if}
</SettingsSection>
