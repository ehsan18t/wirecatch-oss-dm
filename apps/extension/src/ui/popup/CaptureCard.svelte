<script lang="ts">
  // What people come to the popup for first: whether downloads go to Wirecatch at all, whether a window asks about each file and each video first (they decide what the next click on a download does, so D4 keeps them in the popup, and turning one back on needs no trip to the app, D61), and whether this site's downloads go.
  import type { BrowserControls, ExtensionSettings } from '@wirecatch/protocol';
  import { Card, SettingSwitch } from '@wirecatch/ui';
  import type { PopupModel } from '../../lib/popup/model.svelte';
  import { matchingSite, siteForPage } from '../../lib/sites';
  import { SETTINGS } from '../settings';

  interface Props {
    model: PopupModel;
    settings: ExtensionSettings;
    /** The app's own settings; null for an app from before the popup could show them. */
    controls: BrowserControls | null;
    /** Address of the active tab, for its site's switch. */
    currentUrl: string | null;
  }

  let { model, settings, controls, currentUrl }: Props = $props();

  const editable = $derived(model.editable);
  const pageSite = $derived(siteForPage(currentUrl));
  // The entry that ignores this page, which may be a parent of its site: the switch's description names it, since turning it off removes that entry.
  const ignoredBy = $derived(
    currentUrl === null ? null : matchingSite(currentUrl, settings.ignored_sites),
  );
  const site = $derived(ignoredBy ?? pageSite);

  function toggleSite(ignore: boolean): void {
    // Add and remove one site rather than replacing the list, so an edit made at the same time in another browser is not overwritten.
    if (!ignore && ignoredBy !== null) void model.update({ remove_ignored_sites: [ignoredBy] });
    else if (ignore && pageSite !== null) void model.update({ add_ignored_sites: [pageSite] });
  }
</script>

<Card padding="none" class="divide-y divide-line">
  <SettingSwitch
    id="capture-enabled"
    label={SETTINGS.captureEnabled.label}
    description={SETTINGS.captureEnabled.description}
    checked={settings.capture_enabled}
    disabled={!editable}
    onchange={(checked: boolean) => void model.update({ capture_enabled: checked })}
  />
  <SettingSwitch
    id="ask-before"
    label={SETTINGS.askBeforeDownload.label}
    description={SETTINGS.askBeforeDownload.description}
    checked={settings.ask_before_download}
    disabled={!editable}
    onchange={(checked: boolean) => void model.update({ ask_before_download: checked })}
  />
  {#if controls !== null}
    <SettingSwitch
      id="ask-each-video"
      label={SETTINGS.askEachVideo.label}
      description={SETTINGS.askEachVideo.description}
      checked={controls.media.ask_each_time}
      disabled={!editable}
      onchange={(checked: boolean) =>
        void model.updateControls({ media: { ask_each_time: checked } })}
    />
  {/if}
  <SettingSwitch
    id="ignore-site"
    label="Ignore this site"
    checked={ignoredBy !== null}
    disabled={!editable || site === null}
    onchange={toggleSite}
  >
    {#snippet description()}
      {#if site === null}
        Open a web page to ignore its site.
      {:else}
        Downloads from <span class="font-medium text-ink">{site}</span> stay in the browser.
      {/if}
    {/snippet}
  </SettingSwitch>
</Card>
