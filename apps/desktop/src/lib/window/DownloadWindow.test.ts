// A download window with the keyboard (D23): Enter is Download now, Escape closes without
// answering, and a finished download's window closes by itself unless the person is using it.

import { fireEvent, render, screen } from '@testing-library/svelte';
import userEvent from '@testing-library/user-event';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { MockBackend } from '$lib/backend/mock';
import { settle } from '$lib/testing/setup';
import DownloadWindow from './DownloadWindow.svelte';
import { DownloadWindowState, type Target } from './download-window.svelte';
import { CLOSE_AFTER_MS } from './ProgressView.svelte';

const NOON = new Date(2026, 9, 10, 12, 0).getTime();

let cleanup: (() => void) | undefined;
afterEach(() => {
  cleanup?.();
  cleanup = undefined;
  vi.useRealTimers();
});

async function show(b: MockBackend, target: Target) {
  const host = { close: vi.fn(), becameDownload: vi.fn() };
  const win = new DownloadWindowState(b, target, host);
  await win.init();
  const view = render(DownloadWindow, { props: { win } });
  cleanup = () => {
    win.dispose();
    view.unmount();
  };
  await settle();
  return { win, host };
}

describe('a download window with the keyboard', () => {
  it('starts with the focus on Download now, so Enter downloads', async () => {
    const b = new MockBackend({ autoTick: false, now: () => NOON, empty: true });
    b.simulateConfirmation();
    const [asked] = await b.pendingConfirmations();
    const { win, host } = await show(b, { question: asked?.id ?? -1 });
    expect(screen.getByRole('button', { name: 'Download now' })).toHaveFocus();
    // Every control has a name a screen reader can say.
    expect(screen.getByRole('textbox', { name: 'File name' })).toBeInTheDocument();
    expect(screen.getByRole('combobox', { name: 'Queue' })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Change the folder' })).toBeInTheDocument();
    expect(
      screen.getByRole('checkbox', { name: 'Start downloads without asking' }),
    ).not.toBeChecked();
    expect(
      screen.getByRole('checkbox', { name: 'Close this window when downloads start' }),
    ).not.toBeChecked();
    // Enter in the file name field downloads too.
    await userEvent.setup().type(screen.getByRole('textbox', { name: 'File name' }), '{Enter}');
    await settle();
    expect(host.becameDownload).toHaveBeenCalledTimes(1);
    expect(win.phase).toBe('progress');
  });

  it('closes with Escape and leaves the question waiting', async () => {
    const b = new MockBackend({ autoTick: false, now: () => NOON, empty: true });
    b.simulateMediaChoice();
    const [choice] = await b.pendingMediaChoices();
    const { host } = await show(b, { question: choice?.id ?? -1 });
    expect(screen.getByRole('heading', { name: 'Pick a quality' })).toBeInTheDocument();
    // The best quality needs the media add-on, and the main button says so.
    const main = screen.getByRole('button', { name: 'Install add-on and download' });
    expect(main).toHaveFocus();
    await fireEvent.keyDown(main, {
      key: 'Escape',
    });
    expect(host.close).toHaveBeenCalledTimes(1);
    expect(await b.pendingMediaChoices()).toHaveLength(1);
  });

  it('asks before Cancel removes a download, and Escape keeps it', async () => {
    const b = new MockBackend({ autoTick: false, now: () => NOON, empty: true });
    const id = await b.addDownload({
      url: 'https://example.com/big.iso',
      queue: null,
      folder: null,
      file_name: null,
      start: true,
    });
    const { host } = await show(b, { download: id });
    const user = userEvent.setup();
    await user.click(screen.getByRole('button', { name: 'Cancel' }));
    await settle();
    expect(screen.getByRole('button', { name: 'Keep downloading' })).toHaveFocus();
    await user.keyboard('{Escape}');
    await settle();
    expect(screen.queryByRole('button', { name: 'Keep downloading' })).toBeNull();
    // The question closed; the window did not.
    expect(host.close).not.toHaveBeenCalled();
    await user.click(screen.getByRole('button', { name: 'Cancel' }));
    await settle();
    await user.click(screen.getByRole('button', { name: 'Cancel and delete the partial file' }));
    await settle();
    expect(host.close).toHaveBeenCalledTimes(1);
    expect((await b.listDownloads()).some((r) => r.id === id)).toBe(false);
  });
});

describe('a finished download’s window', () => {
  async function finished() {
    const b = new MockBackend({ autoTick: false, now: () => NOON, empty: true });
    const id = await b.addDownload({
      url: 'https://example.com/small.zip',
      queue: null,
      folder: null,
      file_name: null,
      start: true,
    });
    for (let i = 0; i < 400 && (await b.getDownload(id)).status !== 'completed'; i++) b.tick(1000);
    return { b, id };
  }

  it('offers Open and Show in folder, then closes by itself', async () => {
    const { b, id } = await finished();
    vi.useFakeTimers({ shouldAdvanceTime: true });
    const { host } = await show(b, { download: id });
    expect(screen.getByRole('button', { name: 'Open' })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Show in folder' })).toBeInTheDocument();
    expect(screen.getByRole('status')).toHaveTextContent('Finished');
    vi.advanceTimersByTime(CLOSE_AFTER_MS - 100);
    expect(host.close).not.toHaveBeenCalled();
    vi.advanceTimersByTime(200);
    expect(host.close).toHaveBeenCalledTimes(1);
  });

  it('stays while the pointer is over it', async () => {
    const { b, id } = await finished();
    vi.useFakeTimers({ shouldAdvanceTime: true });
    const { host } = await show(b, { download: id });
    const window = screen.getByRole('button', { name: 'Open' }).closest('.progress-window');
    await fireEvent.mouseEnter(window as Element);
    await settle();
    vi.advanceTimersByTime(CLOSE_AFTER_MS * 3);
    expect(host.close).not.toHaveBeenCalled();
    await fireEvent.mouseLeave(window as Element);
    await settle();
    vi.advanceTimersByTime(CLOSE_AFTER_MS + 100);
    expect(host.close).toHaveBeenCalledTimes(1);
  });
});
