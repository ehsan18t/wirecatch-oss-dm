import './style.css';
import { mount } from 'svelte';
import { browser } from 'wxt/browser';
import { installDiagnostics } from '@/lib/diag/install';
import { fallback } from '@/lib/diag/log';
import {
  activeTab,
  connectPage,
  firstState,
  hasSiteAccess,
  mediaShortcut,
  requestSiteAccess,
} from '@/lib/popup/page';
import Popup from '@/ui/popup/Popup.svelte';

async function start(): Promise<void> {
  const model = connectPage();
  const tab = await activeTab();
  if (tab.id !== null) model.watchMedia(tab.id);
  // Mount once the first state is in, so the popup opens at its final size.
  const [siteAccess, shortcut] = await Promise.all([
    hasSiteAccess(),
    mediaShortcut(),
    firstState(model, tab.id !== null),
  ]);
  mount(Popup, {
    target: document.body,
    props: {
      model,
      currentUrl: tab.url,
      siteAccess,
      requestSiteAccess,
      mediaShortcut: shortcut,
      openSettings,
    },
  });
}

/** Opens the options page in a tab; the popup closes, as it would when the person clicks away. */
function openSettings(): void {
  void browser.runtime
    .openOptionsPage()
    .catch(fallback('popup', 'Could not open the settings page', undefined))
    .then(() => {
      window.close();
    });
}

installDiagnostics('popup', window);
void start();
