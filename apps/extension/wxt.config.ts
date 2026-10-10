import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { defineConfig } from 'wxt';

/** Shared identities, owned by the lead and read by both the bridge and the extension. */
interface ExtensionIds {
  chromium_dev_public_key: string;
  firefox_id: string;
  native_host_name: string;
}

function readExtensionIds(): ExtensionIds {
  const path = fileURLToPath(new URL('../../config/extension-ids.json', import.meta.url));
  const parsed: unknown = JSON.parse(readFileSync(path, 'utf8'));
  const ids = parsed as Partial<ExtensionIds>;
  for (const field of ['chromium_dev_public_key', 'firefox_id', 'native_host_name'] as const) {
    if (typeof ids[field] !== 'string' || ids[field] === '') {
      throw new Error(`config/extension-ids.json is missing "${field}". Restore it from git.`);
    }
  }
  return ids as ExtensionIds;
}

const ids = readExtensionIds();

export default defineConfig({
  modules: ['@wxt-dev/module-svelte'],
  srcDir: 'src',
  manifestVersion: 3,
  // Everything is imported explicitly, so the code reads the same in tests and in editors.
  imports: false,
  vite: () => ({
    define: {
      // The bridge registers itself under this name; the background script connects to it.
      __NATIVE_HOST_NAME__: JSON.stringify(ids.native_host_name),
    },
  }),
  manifest: ({ browser }) => {
    const isFirefox = browser === 'firefox';
    return {
      name: 'Wirecatch',
      description: 'Sends your downloads to the Wirecatch download manager.',
      permissions: ['downloads', 'nativeMessaging', 'cookies', 'contextMenus', 'storage'],
      // Needed by the cookies API to read the cookies of the site a download comes from,
      // and to read the address of the current tab for "Ignore this site".
      host_permissions: ['http://*/*', 'https://*/*'],
      action: { default_title: 'Wirecatch' },
      ...(isFirefox
        ? {
            browser_specific_settings: {
              gecko: {
                id: ids.firefox_id,
                strict_min_version: '140.0',
                data_collection_permissions: { required: ['none'] },
              },
            },
          }
        : { key: ids.chromium_dev_public_key }),
    };
  },
});
