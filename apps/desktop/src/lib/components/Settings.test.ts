// The settings screen accepts the limits the core accepts and reads sizes and speeds in the
// units it shows them in.

import { fireEvent, screen } from '@testing-library/svelte';
import { MAX_CONCURRENT_DOWNLOADS } from '@wirecatch/protocol';
import { KB, MB } from '@wirecatch/units';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { settingAnchor } from '$lib/settings-schema';
import { renderApp, settle } from '$lib/testing/setup';

/** More downloads at once than the desktop used to allow (32), and fewer than the core's limit. */
const MANY_AT_ONCE = 50;
/** A little over half the time a setting opened from elsewhere stays highlighted (1.6 s). */
const FLASH_HALF = 900;

afterEach(() => {
  vi.useRealTimers();
});

async function openSettings() {
  const r = await renderApp();
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

  it('says when no setting matches the search, and clears it', async () => {
    await openSettings();
    const search = screen.getByRole('searchbox', { name: 'Search settings' });
    await fireEvent.input(search, { target: { value: 'zzzz' } });
    await settle();
    expect(screen.getByText('No setting matches "zzzz".')).toBeInTheDocument();
    await fireEvent.click(screen.getByRole('button', { name: 'Clear search' }));
    await settle();
    expect(screen.getByRole('heading', { name: 'General' })).toBeInTheDocument();
  });
});

describe('a setting opened from elsewhere', () => {
  const highlighted = () =>
    document.getElementById(settingAnchor('default_folder'))?.classList.contains('flash');

  it('stays highlighted for its full time when opened again', async () => {
    const { app } = await renderApp();
    vi.useFakeTimers();
    app.openSettings('default_folder');
    await vi.advanceTimersByTimeAsync(0);
    expect(highlighted()).toBe(true);
    await vi.advanceTimersByTimeAsync(FLASH_HALF);
    app.openSettings('default_folder');
    await vi.advanceTimersByTimeAsync(FLASH_HALF);
    // The first jump's time is up, but the second one's is not.
    expect(highlighted()).toBe(true);
    await vi.advanceTimersByTimeAsync(FLASH_HALF);
    expect(highlighted()).toBe(false);
  });

  it('leaves no timer behind when the screen closes', async () => {
    const { app } = await renderApp();
    vi.useFakeTimers();
    app.openSettings('default_folder');
    await vi.advanceTimersByTimeAsync(0);
    expect(vi.getTimerCount()).toBe(1);
    app.screen = 'downloads';
    await vi.advanceTimersByTimeAsync(0);
    expect(vi.getTimerCount()).toBe(0);
  });
});
