/**
 * What the tests of the extension's own page (the popup) share: a page's
 * model wired to the real background hub and native client, which talk to a fake app through a
 * fake native port, and the checks every page gets.
 */
import axe from 'axe-core';
import { Hub } from '../src/lib/hub/hub';
import type { BackgroundMessage, PopupRequest } from '../src/lib/popup-port';
import { NativeClient } from '../src/lib/native/client';
import { PopupModel } from '../src/lib/popup/model.svelte';
import { FakeApp, FakeDownloads, MemoryCache, channelPair, flush } from './fakes';

export interface Wired {
  app: FakeApp;
  hub: Hub;
  client: NativeClient;
  model: PopupModel;
}

const wired: Wired[] = [];

/** Starts a hub against a fake app (set up by `configure` first) and connects a page's model to it. */
export async function wirePage(
  configure?: (app: FakeApp, cache: MemoryCache) => void,
): Promise<Wired> {
  const app = new FakeApp();
  const cache = new MemoryCache();
  configure?.(app, cache);
  const client = new NativeClient({
    transport: app,
    hello: () => Promise.resolve({ client: 'extension', client_version: '0.1.0', browser: null }),
    backoffMs: [60_000],
  });
  const hub = new Hub({
    client,
    cache,
    downloads: new FakeDownloads(),
    cookieHeader: () => Promise.resolve(null),
    hasSiteAccess: () => Promise.resolve(true),
    userAgent: null,
  });
  await hub.start();
  await flush();
  const [pageEnd, backgroundEnd] = channelPair<PopupRequest, BackgroundMessage>();
  hub.addPopup(backgroundEnd);
  const model = new PopupModel(pageEnd);
  await flush();
  const result = { app, hub, client, model };
  wired.push(result);
  return result;
}

/** Stops every client `wirePage` started; call after each test. */
export function unwireAll(): void {
  for (const w of wired.splice(0)) w.client.stop();
}

export async function axeViolations(container: HTMLElement): Promise<string[]> {
  const results = await axe.run(container, {
    // jsdom does no layout, so colour contrast is checked by the design system's token tests and in the browser.
    rules: { 'color-contrast': { enabled: false } },
  });
  return results.violations.map((v) => `${v.id}: ${v.nodes.map((n) => n.html).join(' | ')}`);
}

/** True for a control that cannot be used: disabled, or disabled with a reason (focusable, aria-disabled). */
export function isUnavailable(el: HTMLElement): boolean {
  return el.matches(':disabled') || el.getAttribute('aria-disabled') === 'true';
}

/** The element's role (or tag) and name, as a Tab walk records it. */
export function describeFocus(el: Element | null): string {
  if (!(el instanceof HTMLElement)) return 'nothing';
  return `${el.getAttribute('role') ?? el.tagName.toLowerCase()}:${accessibleName(el)}`;
}

function accessibleName(el: HTMLElement): string {
  const labelledBy = el.getAttribute('aria-labelledby');
  if (labelledBy !== null) return document.getElementById(labelledBy)?.textContent.trim() ?? '';
  const aria = el.getAttribute('aria-label');
  if (aria !== null) return aria;
  if (el.id !== '') {
    const label = document.querySelector(`label[for="${el.id}"]`);
    if (label !== null) return label.textContent.trim();
  }
  return el.textContent.trim();
}
