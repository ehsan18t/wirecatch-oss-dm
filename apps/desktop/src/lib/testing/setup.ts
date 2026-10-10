import { render, screen } from '@testing-library/svelte';
import { flushSync, tick } from 'svelte';
import { MockBackend, type MockOptions } from '$lib/backend/mock';
import { AppState } from '$lib/state/app.svelte';
import TestHost from './TestHost.svelte';

/** Noon on Saturday 10 October 2026, so schedules behave the same on every run. */
export const NOON = new Date(2026, 9, 10, 12, 0).getTime();

export async function renderApp(options: MockOptions = {}) {
  const backend = new MockBackend({ autoTick: false, now: () => NOON, ...options });
  const app = new AppState(backend);
  const result = render(TestHost, { props: { app } });
  await app.init();
  await settle();
  return { app, backend, ...result };
}

/** Lets promises and Svelte updates finish. */
export async function settle(): Promise<void> {
  for (let i = 0; i < 3; i++) {
    await tick();
    await new Promise((r) => setTimeout(r, 0));
    flushSync();
  }
}

export function rowByName(name: string): HTMLElement {
  const el = screen.getAllByRole('option').find((o) => o.textContent?.includes(name));
  if (!el) throw new Error(`no row named ${name}`);
  return el;
}
