<script lang="ts">
  // What people come to the popup for first: whether downloads go to Wirecatch at all, whether a window asks about each one first (it decides what the next click on a download does, so D4 keeps it in the popup), and whether this site's downloads go.
  import type { ExtensionSettings } from '@wirecatch/protocol';
  import { Button, Card, SettingSwitch, textIfCut, tooltip } from '@wirecatch/ui';
  import type { PopupModel } from '../../lib/popup/model.svelte';
  import { matchingSite, siteForPage } from '../../lib/sites';
  import { offlineReason } from '../connection';
  import { wholeLabel } from '../cut-label';
  import { SETTINGS } from '../settings';

  interface Props {
    model: PopupModel;
    settings: ExtensionSettings;
    /** Address of the active tab, for "Ignore this site". */
    currentUrl: string | null;
  }

  let { model, settings, currentUrl }: Props = $props();

  const uid = $props.id();
  const helpId = `${uid}-site`;
  const editable = $derived(model.editable);
  const pageSite = $derived(siteForPage(currentUrl));
  const ignoredBy = $derived(
    currentUrl === null ? null : matchingSite(currentUrl, settings.ignored_sites),
  );
  const reason = $derived(editable ? null : offlineReason(false));
  const labelTip = wholeLabel(() => reason);

  function toggleSite(): void {
    // Add and remove one site rather than replacing the list, so an edit made at the same time in another browser is not overwritten.
    if (ignoredBy !== null) void model.update({ remove_ignored_sites: [ignoredBy] });
    else if (pageSite !== null) void model.update({ add_ignored_sites: [pageSite] });
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
  <!-- The button drops under the text rather than squeeze it below about 10rem. -->
  <div class="flex flex-wrap items-center gap-x-3 gap-y-2 px-4 py-3">
    <div class="flex min-w-0 flex-1 basis-40 flex-col gap-0.5">
      <p class="truncate text-ui text-ink" {@attach tooltip(() => textIfCut(pageSite ?? ''))}>
        {pageSite ?? 'This site'}
      </p>
      <p id={helpId} class="text-small text-ink-muted">
        {#if pageSite === null && ignoredBy === null}
          Open a web page to ignore its site.
        {:else if ignoredBy !== null}
          Downloads from {pageSite ?? ignoredBy} stay in the browser.
        {:else}
          Downloads from {pageSite} go to Wirecatch.
        {/if}
      </p>
    </div>
    <Button
      size="sm"
      class="max-w-full min-w-0 shrink"
      disabled={!editable || (pageSite === null && ignoredBy === null)}
      tooltip={reason}
      aria-describedby={helpId}
      onclick={toggleSite}
      {@attach labelTip}
    >
      <span class="truncate">{ignoredBy !== null ? 'Stop ignoring' : 'Ignore this site'}</span>
    </Button>
  </div>
</Card>
