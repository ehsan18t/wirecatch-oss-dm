/** What the extension keeps of the app's settings in the browser, and how the media buttons hear of their setting (D61). */
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { browser } from 'wxt/browser';
import { fakeBrowser } from 'wxt/testing/fake-browser';
import { publishButtonSetting, retireLocalSettings } from '../src/lib/browser/adapters';
import { RETIRED_LOCAL_KEYS } from '../src/lib/cache';
import { MEDIA_BUTTON_SETTING } from '../src/lib/media/messages';
import { flush } from './fakes';

beforeEach(() => {
  fakeBrowser.reset();
});

describe('settings in browser storage', () => {
  it('removes what earlier versions kept for good, and nothing else', async () => {
    await browser.storage.local.set({
      ...Object.fromEntries(RETIRED_LOCAL_KEYS.map((key) => [key, { old: true }])),
      unrelated: 1,
    });
    retireLocalSettings();
    await flush();
    expect(await browser.storage.local.get(null)).toEqual({ unrelated: 1 });
  });
});

describe('the media button setting', () => {
  it('reaches every open page, and a page without the button changes nothing', async () => {
    vi.spyOn(browser.tabs, 'query').mockResolvedValue([{ id: 1 }, { id: 2 }, {}] as never);
    const sent: [number, unknown][] = [];
    vi.spyOn(browser.tabs, 'sendMessage').mockImplementation((id: number, message: unknown) => {
      sent.push([id, message]);
      // The fake's typings take the callback form; the extension uses the promise form.
      return (
        id === 2 ? Promise.reject(new Error('No receiving end')) : Promise.resolve()
      ) as never;
    });
    await publishButtonSetting(false);
    const told = { type: MEDIA_BUTTON_SETTING, on: false };
    expect(sent).toEqual([
      [1, told],
      [2, told],
    ]);
  });
});
