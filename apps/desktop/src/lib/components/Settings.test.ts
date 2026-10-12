// The settings screen accepts the limits the core accepts and reads sizes and speeds in the
// units it shows them in.

import { fireEvent, screen } from '@testing-library/svelte';
import { MAX_CONCURRENT_DOWNLOADS } from '@wirecatch/protocol';
import { KB, MB } from '@wirecatch/units';
import { afterEach, describe, expect, it } from 'vitest';
import type { AppState } from '$lib/state/app.svelte';
import { renderApp, settle } from '$lib/testing/setup';

/** More downloads at once than the desktop used to allow (32), and fewer than the core's limit. */
const MANY_AT_ONCE = 50;

let current: { app: AppState; unmount: () => void } | undefined;
afterEach(() => {
  current?.app.dispose();
  current?.unmount();
  current = undefined;
});

async function openSettings() {
  const r = await renderApp();
  current = r;
  r.app.openSettings();
  await settle();
  return r;
}

async function change(field: HTMLElement, value: string) {
  await fireEvent.change(field, { target: { value } });
  await settle();
}

describe('the settings screen', () => {
  it('runs as many downloads at once as the core allows', async () => {
    const { app } = await openSettings();
    const field = screen.getByLabelText('Downloads at the same time');
    expect(field).toHaveAttribute('max', String(MAX_CONCURRENT_DOWNLOADS));
    await change(field, String(MANY_AT_ONCE));
    expect(app.settings?.max_concurrent_downloads).toBe(MANY_AT_ONCE);
    await change(field, String(MAX_CONCURRENT_DOWNLOADS + 1));
    expect(app.settings?.max_concurrent_downloads).toBe(MAX_CONCURRENT_DOWNLOADS);
  });

  it('reads the smallest file to catch in the unit shown', async () => {
    const { app } = await openSettings();
    await change(screen.getByLabelText('Skip files smaller than'), '1.5');
    expect(app.settings?.extension.min_capture_size).toBe(1.5 * KB);
    await change(screen.getByLabelText('Unit'), 'MB');
    expect(app.settings?.extension.min_capture_size).toBe(1.5 * MB);
    expect(screen.getByLabelText('Skip files smaller than')).toHaveValue(1.5);
  });

  it('reads the total speed limit in megabytes per second', async () => {
    const { app } = await openSettings();
    await fireEvent.click(screen.getByRole('switch', { name: 'Limit total speed' }));
    await settle();
    const field = screen.getByLabelText('Speed limit in megabytes per second');
    await change(field, '2.5');
    expect(app.settings?.global_speed_limit).toBe(2.5 * MB);
    await change(field, '0');
    expect(app.settings?.global_speed_limit).toBe(2.5 * MB);
  });
});
