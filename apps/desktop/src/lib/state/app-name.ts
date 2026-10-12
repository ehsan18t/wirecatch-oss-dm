// The running app's name for titles and labels.

import { PRODUCT_NAME } from '@wirecatch/product';
import type { Backend } from '../backend';

/**
 * The running app's name, or the product name when the shell cannot say (a capability without
 * `core:app:allow-name`, say). The name only labels things, so its failure must never keep the
 * downloads from loading; the shell's capability test keeps the permission in place.
 */
export async function runningAppName(backend: Backend): Promise<string> {
  try {
    return await backend.appName();
  } catch {
    return PRODUCT_NAME;
  }
}
