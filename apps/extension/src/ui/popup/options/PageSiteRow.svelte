<script lang="ts">
  // "Ignore this site" for the tab's page (D279). The app names the site: the page's own, or the entry that already ignores it, a parent site included, which turning the switch off removes. Only the page's host name is sent, and never from a private tab while private windows are not known to be on.
  import { SHELL_ACTIONS, type PageOptions } from '@wirecatch/protocol';
  import { SettingSwitch } from '@wirecatch/ui';
  import type { PopupModel } from '../../../lib/popup/model.svelte';
  import { siteForPage } from '../../../lib/sites';

  interface Props {
    model: PopupModel;
    /** Address of the active tab. */
    currentUrl: string | null;
    /** The popup is in a private window. */
    privateTab: boolean;
  }

  let { model, currentUrl, privateTab }: Props = $props();

  const host = $derived(
    privateTab && model.snapshot?.privateWindows !== true ? null : siteForPage(currentUrl),
  );
  let page = $state<PageOptions | null>(null);

  /** Asks the app about the page; `_revision` names the options it answers for, so a new list reads again. */
  async function read(_revision: number | null = revision): Promise<void> {
    page = host === null || !model.connected ? null : await model.pageOptions(host);
  }

  // Read again whenever the app sends new options: a site ignored elsewhere shows here too.
  const revision = $derived(model.snapshot?.options?.revision.counter ?? null);
  $effect(() => {
    void read(revision);
  });

  async function toggle(ignore: boolean): Promise<void> {
    if (host === null) return;
    const action = ignore ? SHELL_ACTIONS.ignore_site : SHELL_ACTIONS.take_site;
    if (await model.invokeAction(action, host)) await read();
  }
</script>

<SettingSwitch
  id="ignore-site"
  label={page === null ? 'Ignore this site' : `Ignore ${page.site}`}
  help="Downloads from this site stay in the browser."
  checked={page?.ignored ?? false}
  disabled={!model.editable || page === null}
  onchange={(ignore: boolean) => void toggle(ignore)}
/>
