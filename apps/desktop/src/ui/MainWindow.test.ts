import { fireEvent, screen, within } from '@testing-library/svelte';
import userEvent from '@testing-library/user-event';
import { describe, expect, it, vi } from 'vitest';
import { TOAST_DURATION } from '@wirecatch/ui';
import { renderApp, settle } from '$lib/testing/setup';

const toasts = () => screen.getByRole('region', { name: 'Notifications' });

describe('the main window', () => {
  it('says how the app started when the person must know, until they dismiss it (CORE-1)', async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    const { app } = await renderApp({ damagedList: true });
    await settle();
    const notice = within(toasts()).getByText(/started a new download list/);
    expect(within(toasts()).getByText(/The download list was damaged/)).toBeInTheDocument();
    expect(app.toasts.items.at(-1)?.kind).toBe('notice');
    // It stays: the system notification may be long gone.
    vi.advanceTimersByTime(TOAST_DURATION.danger * 2);
    await settle();
    expect(notice.isConnected).toBe(true);
    const user = userEvent.setup({ advanceTimers: vi.advanceTimersByTime.bind(vi) });
    await user.click(within(toasts()).getByRole('button', { name: /Dismiss|Close/ }));
    await settle();
    expect(within(toasts()).queryByText(/started a new download list/)).toBeNull();
  });

  it('starts without the notice when nothing went wrong', async () => {
    const { app } = await renderApp();
    expect(app.toasts.items.some((t) => t.kind === 'notice')).toBe(false);
  });

  it('frames the content on a layer of its own, beside the navigation', async () => {
    await renderApp();
    const main = screen.getByRole('main', { name: 'Downloads' });
    expect(main).toHaveClass('bg-layer');
    expect(screen.getByRole('navigation', { name: 'Views and queues' })).toBeInTheDocument();
    expect(screen.getByRole('banner')).toContainElement(
      screen.getByRole('searchbox', { name: 'Search downloads' }),
    );
  });

  it('offers to show the damaged list it set aside, rather than only naming its path', async () => {
    const { backend } = await renderApp({ damagedList: true });
    const show = vi.spyOn(backend, 'showSetAsideList');
    await settle();
    const user = userEvent.setup();
    await user.click(within(toasts()).getByRole('button', { name: 'Show in folder' }));
    expect(show).toHaveBeenCalledOnce();
  });

  it('shows every notice it started with, offering the folder only for the list set aside', async () => {
    await renderApp({ damagedList: true, dataFolderNotPrivate: true });
    await settle();
    const toastOf = (text: RegExp) => {
      const toast = within(toasts()).getByText(text).closest<HTMLElement>('[data-toast]');
      if (!toast) throw new Error(`no toast holds ${text}`);
      return toast;
    };
    const damaged = toastOf(/started a new download list/);
    const notPrivate = toastOf(/Other accounts may be able to read/);
    expect(damaged).not.toBe(notPrivate);
    expect(within(damaged).getByRole('button', { name: 'Show in folder' })).toBeInTheDocument();
    expect(within(notPrivate).queryByRole('button', { name: 'Show in folder' })).toBeNull();
  });

  it('moves between the title bar, the navigation and the list with F6', async () => {
    await renderApp();
    const user = userEvent.setup();
    const search = screen.getByRole('searchbox', { name: 'Search downloads' });
    const grid = screen.getByRole('grid', { name: 'Downloads' });
    const current = within(screen.getByRole('navigation', { name: 'Views and queues' })).getByRole(
      'button',
      { name: /^All downloads/ },
    );
    search.focus();
    await user.keyboard('{F6}');
    expect(document.activeElement).toBe(current);
    await user.keyboard('{F6}');
    expect(document.activeElement).toBe(grid);
    await user.keyboard('{F6}');
    expect(document.activeElement).toBe(search);
    await user.keyboard('{Shift>}{F6}{/Shift}');
    expect(document.activeElement).toBe(grid);
  });

  it('shows where a link dragged over the window will go, until it is dropped or leaves', async () => {
    const { app } = await renderApp();
    const link = {
      types: ['text/uri-list'],
      getData: (t: string) => (t === 'text/uri-list' ? 'https://example.org/big.iso' : ''),
    };
    const zone = () => screen.queryByText('Drop to download');
    expect(zone()).toBeNull();
    await fireEvent.dragOver(window, { dataTransfer: link });
    expect(zone()).not.toBeNull();
    // Leaving the window, not just one element for another inside it, takes it away.
    await fireEvent.dragLeave(window, { dataTransfer: link, relatedTarget: null });
    expect(zone()).toBeNull();
    await fireEvent.dragOver(window, { dataTransfer: link });
    await fireEvent.drop(window, { dataTransfer: link });
    await settle();
    expect(zone()).toBeNull();
    expect(app.dialog).toEqual({ kind: 'add', urls: ['https://example.org/big.iso'] });
  });
});
