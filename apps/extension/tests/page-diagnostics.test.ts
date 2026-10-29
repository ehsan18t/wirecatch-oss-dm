/** The extension's own page records its diagnostics from the moment it loads (W5, D40). */
import { beforeAll, describe, expect, it, vi } from 'vitest';
import { fakeBrowser } from 'wxt/testing/fake-browser';
import { PATIENCE_MS } from '@wirecatch/ui/testing/jsdom';
import { log, type DiagEntry } from '../src/lib/diag/log';
import { DIAG_KEY_PREFIX } from '../src/lib/diag/persist';

// The pages' connection and first state are not what this is about: the page never gets past
// waiting for its first state, so nothing is mounted, and its component is not even compiled.
vi.mock('@/ui/popup/Popup.svelte', () => ({ default: {} }));
vi.mock('@/lib/popup/page', () => ({
  activeTab: () => Promise.resolve({ id: null, url: null }),
  connectPage: () => ({ watchMedia: () => undefined }),
  firstState: () => new Promise<never>(() => undefined),
  hasSiteAccess: () => Promise.resolve(true),
  mediaShortcut: () => Promise.resolve(null),
  requestSiteAccess: () => Promise.resolve(true),
}));

const PAGES = {
  popup: () => import('../src/entrypoints/popup/main'),
} as const;

async function kept(page: keyof typeof PAGES): Promise<DiagEntry[]> {
  const key = `${DIAG_KEY_PREFIX}${page}`;
  const entries: unknown = (await fakeBrowser.storage.session.get(key))[key];
  return Array.isArray(entries) ? (entries as DiagEntry[]) : [];
}

// The page installs its diagnostics as it loads; loading it here keeps the module loading out of
// the tests' own time.
beforeAll(async () => {
  await Promise.all(Object.values(PAGES).map((load) => load()));
});

describe.each(Object.keys(PAGES) as (keyof typeof PAGES)[])('the %s page', (page) => {
  it('keeps the errors it handles past its own close, under a key of its own', async () => {
    log.warn('settings', `Handled on the ${page} page`);
    await vi.waitFor(async () => {
      expect((await kept(page)).map((e) => e.message)).toContain(`Handled on the ${page} page`);
    }, PATIENCE_MS);
  });

  it('logs what nothing caught, as its own', async () => {
    window.dispatchEvent(new ErrorEvent('error', { error: new Error('thrown in a handler') }));
    await vi.waitFor(async () => {
      expect(await kept(page)).toContainEqual(
        expect.objectContaining({ scope: page, message: 'Uncaught error' }),
      );
    }, PATIENCE_MS);
  });
});
