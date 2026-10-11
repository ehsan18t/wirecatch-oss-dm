import { render, screen } from '@testing-library/svelte';
import { flushSync, tick } from 'svelte';
import { MockBackend, type MockOptions } from '$lib/backend/mock';
import { AppState } from '$lib/state/app.svelte';
import TestHost from './TestHost.svelte';

/** Noon on Saturday 10 October 2026, so schedules behave the same on every run. */
export const NOON = new Date(2026, 9, 10, 12, 0).getTime();

export async function renderApp(options: MockOptions = {}) {
  // The sample download windows load lazily; loading them first keeps `downloadWindow()` from
  // racing that import when the machine is busy.
  await import('$lib/window/MockWindows.svelte');
  const backend = new MockBackend({ autoTick: false, now: () => NOON, ...options });
  const app = new AppState(backend);
  // Schedules in labels follow the same fixed clock as the mock.
  app.now = new Date(NOON);
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

/**
 * The sample app's download windows (D23), which float in the page, newest last. Waits for at
 * least one: they load in the background.
 */
export async function downloadWindows(): Promise<HTMLElement[]> {
  const found = await screen.findAllByRole('region', { name: /^Wirecatch download window/ });
  await settle();
  return found;
}

/** The newest sample download window. */
export async function downloadWindow(): Promise<HTMLElement> {
  const found = await downloadWindows();
  const last = found.at(-1);
  if (!last) throw new Error('no download window');
  return last;
}

export function rowByName(name: string): HTMLElement {
  const el = screen.getAllByRole('row').find((o) => o.textContent.includes(name));
  if (!el) throw new Error(`no row named ${name}`);
  return el;
}
