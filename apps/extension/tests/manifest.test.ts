/**
 * @vitest-environment node
 *
 * The manifest's `key` decides the Chromium extension id. Every Chromium build carries it, so an
 * unpacked load gets the one id every bridge trusts (D22); only the Chrome Web Store package
 * leaves it out, because the store does not accept it.
 */
import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { describe, expect, it } from 'vitest';
import type { ConfigEnv, UserManifest } from 'wxt';
import config, { STORE_MODE } from '../wxt.config';

const ids = JSON.parse(readFileSync(resolve(__dirname, '../../../config/extension-ids.json'), 'utf8')) as {
  chromium_public_key: string;
  firefox_id: string;
};

async function manifest(env: Omit<ConfigEnv, 'manifestVersion'>): Promise<UserManifest> {
  const fn = config.manifest;
  if (typeof fn !== 'function') throw new Error('wxt.config.ts should build the manifest in a function');
  return await fn({ ...env, manifestVersion: 3 });
}

describe('the manifest key', () => {
  it.each([
    ['development', 'serve'],
    ['development', 'build'],
    ['production', 'build'],
  ] as const)('is in every Chromium build (%s, %s)', async (mode, command) => {
    const built = await manifest({ browser: 'chrome', mode, command });
    expect(built.key).toBe(ids.chromium_public_key);
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
      expect(built.browser_specific_settings?.gecko?.id).toBe(ids.firefox_id);
    }
  });
});
