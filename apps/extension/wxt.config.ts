import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { defineConfig } from 'wxt';
import { SITE_ORIGINS } from './src/lib/site-access';

/** Shared identities, owned by the lead and read by both the bridge and the extension. */
interface ExtensionIds {
  chromium_public_key: string;
  firefox_id: string;
  native_host_name: string;
}

function readExtensionIds(): ExtensionIds {
  const path = fileURLToPath(new URL('../../config/extension-ids.json', import.meta.url));
  const parsed: unknown = JSON.parse(readFileSync(path, 'utf8'));
  const ids = parsed as Partial<ExtensionIds>;
  for (const field of ['chromium_public_key', 'firefox_id', 'native_host_name'] as const) {
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
  manifest: ({ browser, mode }) => {
    const isFirefox = browser === 'firefox';
    // The development key gives unpacked builds the fixed id the bridge allows. Store builds
    // must not carry it, so it is only added in development mode (`pnpm dev`, `pnpm build:dev`).
    const devKey = mode === 'development' ? { key: ids.chromium_public_key } : {};
    return {
      name: 'Wirecatch',
      description: 'Sends your downloads to the Wirecatch download manager.',
      // webRequest: observe responses (never block or change them) to spot video and audio,
      // that is HLS and DASH manifests and media files, so the media button and the popup can
      // offer them. No blocking, declarativeNetRequest or webRequestBlocking permission is asked for.
      permissions: ['downloads', 'nativeMessaging', 'cookies', 'contextMenus', 'storage', 'webRequest'],
      // Needed by the cookies API to read the cookies of the site a download comes from,
      // to read the address of the current tab for "Ignore this site", for webRequest to
      // see media responses, and for the media button's content script.
      host_permissions: SITE_ORIGINS,
      action: { default_title: 'Wirecatch' },
      // A keyboard way to download a page's media; people can change or remove it in the
      // browser's extension shortcuts page.
      commands: {
        'download-media': {
          suggested_key: { default: 'Alt+Shift+D' },
          description: "Download this page's video or audio with Wirecatch",
        },
      },
      ...(isFirefox
        ? {
            browser_specific_settings: {
              gecko: {
                id: ids.firefox_id,
                // storage.session, MV3 host permission prompts and data_collection_permissions.
                strict_min_version: '140.0',
                data_collection_permissions: { required: ['none'] },
              },
            },
          }
        : {
            // Needs storage.session (102), a native messaging port keeping the service worker
            // alive (105) and promise-based extension APIs; 116 is a conservative floor.
            minimum_chrome_version: '116',
            ...devKey,
          }),
    };
  },
});
