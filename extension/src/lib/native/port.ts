import { NATIVE_HOST_NAME } from '@wirecatch/product';
import { browser } from 'wxt/browser';
import type { NativePort, NativeTransport } from './client';
import { disconnectMessage } from './status';

/**
 * The browser's native messaging connection to the bridge (`runtime.connectNative`).
 * The only place the extension opens one, so `NativeClient` owns its whole lifecycle.
 */
export function browserTransport(): NativeTransport {
  return {
    connect: (): NativePort => browser.runtime.connectNative(NATIVE_HOST_NAME),
    disconnectReason: (port) => disconnectMessage(browser.runtime.lastError, port),
  };
}
