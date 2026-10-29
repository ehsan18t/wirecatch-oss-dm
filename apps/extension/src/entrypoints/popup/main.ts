import './style.css';
import { mount } from 'svelte';
import { browser } from 'wxt/browser';
import { installDiagnostics } from '@/lib/diag/install';
import { fallback } from '@/lib/diag/log';
import { readDiagnostics } from '@/lib/diag/persist';
import { diagnosticsReport } from '@/lib/diag/report';
import { helloFromExtension, sessionArea } from '@/lib/browser/adapters';
import {
  activeTab,
  connectPage,
  firstState,
  hasSiteAccess,
  mediaShortcut,
  requestSiteAccess,
} from '@/lib/popup/page';
import Popup from '@/ui/popup/Popup.svelte';

/** Chromium's page for extension shortcuts. Firefox has none a page can open: its shortcuts are under the add-ons manager's menu. */
const CHROMIUM_SHORTCUTS = 'chrome://extensions/shortcuts';

async function diagnostics(connection: string): Promise<string> {
  const area = sessionArea();
  const [identity, entries] = await Promise.all([
    helloFromExtension(),
    area ? readDiagnostics(area) : Promise.resolve([]),
  ]);
  return diagnosticsReport({ identity, connection, now: Date.now(), entries });
}

function openShortcuts(): void {
  void browser.tabs
    .create({ url: CHROMIUM_SHORTCUTS })
    .catch(fallback('popup', 'Could not open the shortcuts page', undefined));
}

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
      openShortcuts: import.meta.env.FIREFOX ? null : openShortcuts,
      diagnostics,
      // The app's settings open in its window; the popup closes, as it would when the person clicks away.
      close: () => {
        window.close();
      },
    },
  });
}

installDiagnostics('popup', window);
void start();
