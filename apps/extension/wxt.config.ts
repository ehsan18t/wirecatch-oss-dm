import {
  CHROMIUM_PUBLIC_KEY,
  EXTENSION_DESCRIPTION,
  FIREFOX_EXTENSION_ID,
  PRODUCT_NAME,
  WEBSITE_URL,
} from '@wirecatch/product';
import tailwindcss from '@tailwindcss/vite';
import Icons from 'unplugin-icons/vite';
import { defineConfig } from 'wxt';
import { buildDefines } from './build-values';
import { SITE_ORIGINS } from './src/lib/site-access';

/**
 * The WXT mode of `pnpm build:store`: the package uploaded to the Chrome Web Store, the only
 * Chromium build without the `key` field (see PUBLISHING.md).
 */
export const STORE_MODE = 'store';

export default defineConfig({
  modules: ['@wxt-dev/module-svelte'],
  srcDir: 'src',
  manifestVersion: 3,
  // Everything is imported explicitly, so the code reads the same in tests and in editors.
  imports: false,
  vite: () => ({
    plugins: [tailwindcss(), Icons({ compiler: 'svelte', autoInstall: false })],
    define: buildDefines(),
  }),
  manifest: ({ browser, mode }) => {
    const isFirefox = browser === 'firefox';
    // The public key gives every unpacked Chromium build the one id every bridge trusts (D22).
    // The Chrome Web Store does not accept a manifest with a key, so the store package leaves it
    // out; the store gives it the same id because the first upload carried the private key.
    const key = mode === STORE_MODE ? {} : { key: CHROMIUM_PUBLIC_KEY };
    return {
      name: PRODUCT_NAME,
      description: EXTENSION_DESCRIPTION,
      homepage_url: WEBSITE_URL,
      // webRequest: observe responses (never block or change them) to spot video and audio,
      // that is HLS and DASH manifests and media files, so the media button and the popup can
      // offer them. No blocking, declarativeNetRequest or webRequestBlocking permission is asked for.
      permissions: [
        'downloads',
        'nativeMessaging',
        'cookies',
        'contextMenus',
        'storage',
        'webRequest',
      ],
      // Needed by the cookies API to read the cookies of the site a download comes from,
      // to read the address of the current tab for "Ignore this site", for webRequest to
      // see media responses, and for the media button's content script.
      host_permissions: SITE_ORIGINS,
      action: { default_title: PRODUCT_NAME },
      // A keyboard way to download a page's media; people can change or remove it in the
      // browser's extension shortcuts page.
      commands: {
        'download-media': {
          suggested_key: { default: 'Alt+Shift+D' },
          description: `Download this page's video or audio with ${PRODUCT_NAME}`,
        },
      },
      ...(isFirefox
        ? {
            browser_specific_settings: {
              gecko: {
                id: FIREFOX_EXTENSION_ID,
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
            ...key,
          }),
    };
  },
});
