/**
 * @vitest-environment node
 *
 * The manifest's `key` decides the Chromium extension id. Every Chromium build carries it, so an
 * unpacked load gets the one id every bridge trusts (D22); only the Chrome Web Store package
 * leaves it out, because the store does not accept it.
 */
import { CHROMIUM_PUBLIC_KEY, FIREFOX_EXTENSION_ID } from '@wirecatch/product';
import { describe, expect, it } from 'vitest';
import type { ConfigEnv, UserManifest } from 'wxt';
import config, { STORE_MODE } from '../wxt.config';

async function manifest(env: Omit<ConfigEnv, 'manifestVersion'>): Promise<UserManifest> {
  const fn = config.manifest;
  if (typeof fn !== 'function')
    throw new Error('wxt.config.ts should build the manifest in a function');
  return await fn({ ...env, manifestVersion: 3 });
}

describe('the manifest key', () => {
  it.each([
    ['development', 'serve'],
    ['development', 'build'],
    ['production', 'build'],
  ] as const)('is in every Chromium build (%s, %s)', async (mode, command) => {
    const built = await manifest({ browser: 'chrome', mode, command });
    expect(built.key).toBe(CHROMIUM_PUBLIC_KEY);
  });

  it('is left out of the Chrome Web Store package, which is otherwise the same', async () => {
    const store = await manifest({ browser: 'chrome', mode: STORE_MODE, command: 'build' });
    const unpacked = await manifest({ browser: 'chrome', mode: 'production', command: 'build' });
    expect(store).not.toHaveProperty('key');
    expect(store).toEqual({ ...unpacked, key: undefined });
  });

  it('is never in a Firefox build, which uses the gecko id instead', async () => {
    for (const mode of ['development', 'production', STORE_MODE]) {
      const built = await manifest({ browser: 'firefox', mode, command: 'build' });
      expect(built).not.toHaveProperty('key');
      expect(built.browser_specific_settings?.gecko?.id).toBe(FIREFOX_EXTENSION_ID);
    }
  });
});

describe('what the extension asks for', () => {
  it.each(['chrome', 'firefox'])('is the same in %s, and changes only here', async (browser) => {
    const built = await manifest({ browser, mode: 'production', command: 'build' });
    // A new permission shows a warning at install and disables existing installs until people approve it.
    expect(built.permissions).toEqual([
      'downloads',
      'nativeMessaging',
      'cookies',
      'contextMenus',
      'storage',
      'webRequest',
    ]);
    expect(built.host_permissions).toEqual(['http://*/*', 'https://*/*']);
    expect(built.web_accessible_resources).toEqual([
      { resources: ['notices.html'], matches: ['http://*/*', 'https://*/*'] },
    ]);
    expect(Object.keys(built.commands ?? {})).toEqual(['download-media']);
    expect(built).not.toHaveProperty('optional_permissions');
  });
});
