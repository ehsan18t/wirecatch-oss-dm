import './style.css';
import { mount } from 'svelte';
import { browser } from 'wxt/browser';
import { installDiagnostics } from '@/lib/diag/install';
import { fallback } from '@/lib/diag/log';
import {
  connectPage,
  firstState,
  hasSiteAccess,
  mediaShortcut,
  requestSiteAccess,
} from '@/lib/popup/page';
import Options from '@/ui/options/Options.svelte';

/** Chromium's page for extension shortcuts. Firefox has none a page can open: its shortcuts are under the add-ons manager's menu. */
const CHROMIUM_SHORTCUTS = 'chrome://extensions/shortcuts';

function openShortcuts(): void {
  void browser.tabs
    .create({ url: CHROMIUM_SHORTCUTS })
    .catch(fallback('settings', 'Could not open the shortcuts page', undefined));
}

async function start(): Promise<void> {
  const model = connectPage();
  const [siteAccess, shortcut] = await Promise.all([
    hasSiteAccess(),
    mediaShortcut(),
    firstState(model, false),
  ]);
  mount(Options, {
    target: document.body,
    props: {
      model,
      siteAccess,
      requestSiteAccess,
      mediaShortcut: shortcut,
      openShortcuts: import.meta.env.FIREFOX ? null : openShortcuts,
    },
  });
}

installDiagnostics('options', window);
void start();
