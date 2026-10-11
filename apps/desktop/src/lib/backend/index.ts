import type { MockOptions } from './mock';
import { TauriBackend } from './tauri';
import type { Backend } from './types';

export type { Backend, ShellSignal } from './types';
export { BackendError } from './types';

/** True inside the Tauri webview. */
export function inTauri(): boolean {
  return typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;
}

/** `?at=12:00` runs the sample clock from that time today, so screenshots look the same. */
function clockFrom(at: string | null): (() => number) | undefined {
  const m = at ? /^(\d{1,2}):(\d{2})$/.exec(at) : null;
  if (!m) return undefined;
  const start = new Date();
  start.setHours(Number(m[1]), Number(m[2]), 0, 0);
  const offset = start.getTime() - Date.now();
  return () => Date.now() + offset;
}

/**
 * Reads mock options from the address bar, so reviewers and tests can set up a scenario:
 * `?count=10000&active=60`, `?empty`, `?capture=0`, `?seed=3`, `?at=12:00`.
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
    now: clockFrom(params.get('at')),
    empty: params.has('empty'),
    autoCapture: params.get('capture') !== '0',
  };
}

/**
 * Picks the backend. The mock exists only in development and in `--mode mock` builds: in a
 * production build the condition below is false at build time, so the mock module is left out
 * of the bundle entirely.
 */
export async function createBackend(): Promise<Backend> {
  const mockAllowed = import.meta.env.MODE === 'mock' || import.meta.env.DEV;
  if (mockAllowed && (import.meta.env.MODE === 'mock' || !inTauri())) {
    const { MockBackend } = await import('./mock');
    return new MockBackend(mockOptionsFromUrl(window.location.search));
  }
  return new TauriBackend();
}
