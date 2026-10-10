import { MockBackend, type MockOptions } from './mock';
import { TauriBackend } from './tauri';
import type { Backend } from './types';

export type { Backend } from './types';
export { BackendError } from './types';

/** True inside the Tauri webview. */
export function inTauri(): boolean {
  return typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;
}

/**
 * Reads mock options from the address bar, so reviewers and tests can set up a scenario:
 * `?count=10000&active=60`, `?empty`, `?capture=0`, `?seed=3`.
 */
export function mockOptionsFromUrl(search: string): MockOptions {
  const params = new URLSearchParams(search);
  const num = (key: string) => {
    const v = params.get(key);
    return v === null || v === '' || Number.isNaN(Number(v)) ? undefined : Number(v);
  };
  return {
    count: num('count'),
    active: num('active'),
    seed: num('seed'),
    empty: params.has('empty'),
    autoCapture: params.get('capture') !== '0',
  };
}

/** The mock runs in `pnpm dev:mock` and in any normal browser; Tauri gets the real backend. */
export function createBackend(): Backend {
  if (import.meta.env.VITE_BACKEND === 'mock' || !inTauri()) {
    return new MockBackend(mockOptionsFromUrl(window.location.search));
  }
  return new TauriBackend();
}
