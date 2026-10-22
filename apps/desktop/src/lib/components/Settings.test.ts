// The settings screen accepts the limits the core accepts and reads sizes and speeds in the
// units it shows them in.

import { fireEvent, screen } from '@testing-library/svelte';
import { MAX_CONCURRENT_DOWNLOADS } from '@wirecatch/protocol';
import { KB, MB } from '@wirecatch/units';
import { describe, expect, it, vi } from 'vitest';
import { SPEED_FIELD } from '$lib/limits';
import { settingAnchor, type SectionId } from '$lib/settings-schema';
import { renderApp, settle } from '$lib/testing/setup';
import { FLASH_MS } from './Settings.svelte';

/** More downloads at once than the desktop used to allow (32), and fewer than the core's limit. */
const MANY_AT_ONCE = 50;
/** A little over half the time a setting opened from elsewhere stays highlighted. */
const FLASH_HALF = FLASH_MS / 2 + 100;

async function openSettings(section: SectionId = 'general') {
  const r = await renderApp();
  r.app.openSettingsSection(section);
  await settle();
  return r;
}

/** Types `value` into a field and commits it, as leaving the field does. */
async function change(field: HTMLElement, value: string) {
  await fireEvent.input(field, { target: { value } });
  await fireEvent.blur(field);
  await settle();
}

const spin = (name: string) => screen.getByRole('spinbutton', { name });

describe('the settings screen', () => {
  it('runs as many downloads at once as the core allows', async () => {
    const { app } = await openSettings('speed');
    const field = spin('Downloads at the same time');
    expect(field).toHaveAttribute('aria-valuemax', String(MAX_CONCURRENT_DOWNLOADS));
    await change(field, String(MANY_AT_ONCE));
    expect(app.settings?.max_concurrent_downloads).toBe(MANY_AT_ONCE);
    await change(field, String(MAX_CONCURRENT_DOWNLOADS + 1));
    expect(app.settings?.max_concurrent_downloads).toBe(MAX_CONCURRENT_DOWNLOADS);
    expect(field).toHaveValue(String(MAX_CONCURRENT_DOWNLOADS));
  });

  it('shows the downloads at once it kept when what was typed reads as the same', async () => {
    const { app } = await openSettings('speed');
    const field = spin('Downloads at the same time');
    await change(field, '1');
    for (const typed of ['0', '-3', '1.2']) {
      await change(field, typed);
      expect(app.settings?.max_concurrent_downloads).toBe(1);
      expect(field).toHaveValue('1');
    }
    await change(field, String(MAX_CONCURRENT_DOWNLOADS));
    await change(field, String(MAX_CONCURRENT_DOWNLOADS + 500));
    expect(field).toHaveValue(String(MAX_CONCURRENT_DOWNLOADS));
  });

  it('keeps the downloads at once when the field is emptied', async () => {
    const { app } = await openSettings('speed');
    const field = spin('Downloads at the same time');
    await change(field, String(MANY_AT_ONCE));
    await change(field, '');
    expect(app.settings?.max_concurrent_downloads).toBe(MANY_AT_ONCE);
    expect(field).toHaveValue(String(MANY_AT_ONCE));
  });

  it('reads the smallest file to catch in the unit shown', async () => {
    const { app } = await openSettings('browser');
    await change(spin('Skip files smaller than'), '1.5');
    expect(app.settings?.extension.min_capture_size).toBe(1.5 * KB);
    await fireEvent.click(screen.getByRole('radio', { name: 'MB' }));
    await settle();
    expect(app.settings?.extension.min_capture_size).toBe(1.5 * MB);
    expect(spin('Skip files smaller than')).toHaveValue('1.5');
  });

  it('shows the smallest file to catch it kept when what was typed reads as the same', async () => {
    const { app } = await openSettings('browser');
    const field = spin('Skip files smaller than');
    await change(field, '0');
    await change(field, '-5');
    expect(app.settings?.extension.min_capture_size).toBe(0);
    expect(field).toHaveValue('0');
  });

  it('reads the total speed limit in megabytes per second', async () => {
    const { app } = await openSettings('speed');
    await fireEvent.click(screen.getByRole('switch', { name: 'Limit total speed' }));
    await settle();
    const field = spin('Total speed limit');
    await change(field, '2.5');
    expect(app.settings?.global_speed_limit).toBe(2.5 * MB);
    for (const typed of ['', 'fast']) {
      await change(field, typed);
      expect(app.settings?.global_speed_limit).toBe(2.5 * MB);
      expect(field).toHaveValue('2.5');
    }
    // Below the smallest limit is the smallest limit: a limit of nothing is the switch turned off.
    await change(field, '0');
    expect(app.settings?.global_speed_limit).toBe(Math.round(SPEED_FIELD.MB.min * MB));
  });

  it('keeps a site on the network as its host, the way the app keeps it', async () => {
    const { app } = await openSettings('advanced');
    const field = screen.getByRole('textbox', { name: 'Sites on your own network' });
    await fireEvent.input(field, { target: { value: 'https://Wiki.Corp.Example.com/start' } });
    await fireEvent.keyDown(field, { key: 'Enter' });
    await settle();
    expect(app.settings?.local_sites).toEqual(['wiki.corp.example.com']);
    expect(
      screen.getByRole('button', {
        name: 'Remove wiki.corp.example.com from sites on your own network',
      }),
    ).toBeInTheDocument();
  });

  it('says when no setting matches the search, and clears it', async () => {
    await openSettings();
    const search = screen.getByRole('searchbox', { name: 'Search settings' });
    await fireEvent.input(search, { target: { value: 'zzzz' } });
    await settle();
    expect(screen.getByRole('heading', { name: 'No setting matches “zzzz”' })).toBeInTheDocument();
    const clear = screen.getAllByRole('button', { name: 'Clear the search' }).at(-1);
    if (!clear) throw new Error('no button to clear the search');
    await fireEvent.click(clear);
    await settle();
    expect(screen.getByRole('heading', { name: 'General' })).toBeInTheDocument();
  });
});

describe('how finished downloads are told', () => {
  it('is picked from a list, and names when the default queue tells, with a way to change it', async () => {
    const { app } = await openSettings('general');
    const pick = screen.getByRole('combobox', { name: 'When a download finishes' });
    expect(pick).toHaveTextContent('Show a notification');
    expect(screen.getByText('Main tells you each time a download finishes.')).toBeVisible();
    await fireEvent.click(pick);
    await settle();
    await fireEvent.click(screen.getByRole('option', { name: /Open a window/ }));
    await settle();
    expect(app.settings?.finish_notice).toBe('window');

    await fireEvent.click(screen.getByRole('button', { name: 'Change when Main tells you' }));
    await settle();
    expect(app.dialog).toEqual({ kind: 'queue', id: app.queues.defaultQueue?.id });
  });

  it('leaves out when the queue tells while nothing is told', async () => {
    const { app } = await openSettings('general');
    await app.updateSettings((d) => void (d.finish_notice = 'nothing'));
    await settle();
    expect(screen.queryByText(/tells you each time/)).toBeNull();
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
    const started = vi.spyOn(globalThis, 'setTimeout');
    const stopped = vi.spyOn(globalThis, 'clearTimeout');
    app.openSettings('default_folder');
    await vi.advanceTimersByTimeAsync(0);
    const highlight: unknown = started.mock.results.find(
      (_, i) => started.mock.calls[i]?.[1] === FLASH_MS,
    )?.value;
    expect(highlight).toBeDefined();
    app.screen = 'downloads';
    await vi.advanceTimersByTimeAsync(0);
    expect(stopped).toHaveBeenCalledWith(highlight);
  });
});
