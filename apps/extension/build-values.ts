/**
 * Values read from their sources when the extension is built and baked into its code. The build
 * (wxt.config.ts) and the tests (vitest.config.ts) both take them from here, so the code sees the
 * same values in both, and a source that is missing stops either at once.
 */
import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { BADGE_TOKEN, cssColor } from './src/lib/theme';
import { isRecord, isString, object, refine, type Guarded } from './src/lib/validate';

const nonEmpty = refine(isString, (value) => value !== '');

/** Shared identities, owned by the lead and read by both the bridge and the extension. */
const EXTENSION_IDS = {
  chromium_public_key: nonEmpty,
  firefox_id: nonEmpty,
  native_host_name: nonEmpty,
};
const isExtensionIds = object(EXTENSION_IDS);

function readExtensionIds(): Guarded<typeof isExtensionIds> {
  const path = fileURLToPath(new URL('../../config/extension-ids.json', import.meta.url));
  const parsed: unknown = JSON.parse(readFileSync(path, 'utf8'));
  if (isExtensionIds(parsed)) return parsed;
  const fields = isRecord(parsed) ? parsed : {};
  const missing = Object.keys(EXTENSION_IDS).find((field) => !nonEmpty(fields[field]));
  throw new Error(
    `config/extension-ids.json is missing "${missing ?? 'its fields'}". Restore it from git.`,
  );
}

export const extensionIds = readExtensionIds();

/** The toolbar badge's color, read from the popup's stylesheet; a missing token stops the build here. */
const badgeColor = cssColor(
  BADGE_TOKEN,
  readFileSync(fileURLToPath(new URL('src/entrypoints/popup/style.css', import.meta.url)), 'utf8'),
);

/** The globals `src/env.d.ts` declares, as Vite's `define` takes them. */
export function buildDefines(): Record<string, string> {
  return {
    // The bridge registers itself under this name; the background script connects to it.
    __NATIVE_HOST_NAME__: JSON.stringify(extensionIds.native_host_name),
    __BADGE_COLOR__: JSON.stringify(badgeColor),
  };
}
