import type { WindowFrame } from '@wirecatch/protocol';
import { render, screen, within } from '@testing-library/svelte';
import userEvent from '@testing-library/user-event';
import { describe, expect, it, vi } from 'vitest';
import { appState, mockBackend, settle } from '$lib/testing/setup';
import TestHost from '$lib/testing/TestHost.svelte';

/** The main window over a sample backend whose window frame the test drives. */
async function setup() {
  const backend = mockBackend();
  const control = vi.spyOn(backend, 'controlWindow');
  const place = vi.spyOn(backend, 'setMaximizeButton');
  const listeners: ((frame: WindowFrame) => void)[] = [];
  let latest: WindowFrame = { maximized: false, focused: true, maximize_hovered: false };
  vi.spyOn(backend, 'onWindowFrame').mockImplementation((listener) => {
    listeners.push(listener);
    return Promise.resolve(() => undefined);
  });
  vi.spyOn(backend, 'windowFrame').mockImplementation(() => Promise.resolve(latest));
  const app = appState(backend);
  render(TestHost, { props: { app } });
  await app.init();
  await settle();
  const frame = (change: Partial<WindowFrame>) => {
    latest = { ...latest, ...change };
    for (const listener of listeners) listener(latest);
  };
  return { app, control, place, frame, user: userEvent.setup() };
}

const bar = () => screen.getByRole('banner');

describe('the title bar', () => {
  it('sends its caption buttons to the window', async () => {
    const { control, user } = await setup();
    await user.click(within(bar()).getByRole('button', { name: 'Minimize' }));
    await user.click(within(bar()).getByRole('button', { name: 'Maximize' }));
    await user.click(within(bar()).getByRole('button', { name: 'Close' }));
    expect(control.mock.calls.map(([action]) => action)).toEqual([
      'minimize',
      'toggle_maximize',
      'close',
    ]);
  });

  it('tells the window where its maximize button is, and draws the hover the window reports', async () => {
    const { place, frame } = await setup();
    expect(place).toHaveBeenCalled();
    const maximize = within(bar()).getByRole('button', { name: 'Maximize' });
    frame({ maximize_hovered: true });
    await settle();
    expect(maximize).toHaveClass('bg-wash');
    frame({ maximized: true });
    await settle();
    expect(within(bar()).getByRole('button', { name: 'Restore' })).toBeInTheDocument();
  });

  it('drags the window from its empty parts only', async () => {
    await setup();
    expect(bar()).toHaveAttribute('data-tauri-drag-region');
    const search = screen.getByRole('searchbox', { name: 'Search downloads' });
    expect(search.closest('.no-drag')).not.toBeNull();
  });

  it('searches the list, clears with Escape, and hands the keyboard to the list with the down arrow', async () => {
    const { app, user } = await setup();
    const search = screen.getByRole('searchbox', { name: 'Search downloads' });
    await user.click(search);
    await user.keyboard('blender');
    await settle();
    expect(app.visible.length).toBe(1);
    await user.keyboard('{Escape}');
    expect(app.search).toBe('');
    await user.keyboard('{ArrowDown}');
    expect(document.activeElement).toBe(screen.getByRole('grid', { name: 'Downloads' }));
  });

  it("lands the down arrow on the first match, as the list's keyboard row, without selecting it", async () => {
    const { app, user } = await setup();
    await user.click(screen.getByRole('searchbox', { name: 'Search downloads' }));
    await user.keyboard('ubu');
    await settle();
    const first = app.visible[0]!;
    await user.keyboard('{ArrowDown}');
    await settle();
    const grid = screen.getByRole('grid', { name: 'Downloads' });
    expect(document.activeElement).toBe(grid);
    expect(grid).toHaveAttribute('aria-activedescendant', `row-${String(first)}`);
    expect(app.selection.has(first)).toBe(false);
    // The next arrow picks the row it points at.
    await user.keyboard('{ArrowDown}');
    await settle();
    expect(app.selection.list).toEqual([first]);
  });

  it('keeps the focus in the search when its clear button empties it', async () => {
    const { app, user } = await setup();
    const search = screen.getByRole('searchbox', { name: 'Search downloads' });
    await user.click(search);
    await user.keyboard('ubu');
    await settle();
    await user.click(within(bar()).getByRole('button', { name: 'Clear the search' }));
    await settle();
    expect(app.search).toBe('');
    expect(document.activeElement).toBe(search);
  });

  it('searches settings while they show, and downloads again once they close', async () => {
    const { app, user } = await setup();
    app.openSettings();
    await settle();
    await user.click(within(bar()).getByRole('searchbox', { name: 'Search settings' }));
    await user.keyboard('proxy');
    await settle();
    expect(app.settingsQuery).toBe('proxy');
    expect(app.search).toBe('');
    expect(app.screen).toBe('settings');
    app.screen = 'downloads';
    await settle();
    expect(within(bar()).getByRole('searchbox', { name: 'Search downloads' })).toHaveValue('');
  });

  it('says it runs on sample data', async () => {
    await setup();
    expect(within(bar()).getByText('Sample data')).toBeInTheDocument();
  });

  it('says which view it searches', async () => {
    const { app } = await setup();
    app.show({ kind: 'status', group: 'paused' });
    await settle();
    expect(within(bar()).getByRole('searchbox', { name: 'Search Paused' })).toBeInTheDocument();
    app.show({ kind: 'all' });
    await settle();
    expect(within(bar()).getByRole('searchbox', { name: 'Search downloads' })).toBeInTheDocument();
  });
});
