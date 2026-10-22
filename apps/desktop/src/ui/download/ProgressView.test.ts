// A download's progress window while the download changes under it: an inline question goes when
// its answer no longer means what it says, and the focus never falls out of the window when the
// control holding it goes away, so Escape still closes it and a finished one stays while used.

import type { DownloadId } from '@wirecatch/protocol';
import { fireEvent, render, screen } from '@testing-library/svelte';
import userEvent from '@testing-library/user-event';
import { describe, expect, it, vi } from 'vitest';
import type { MockBackend } from '$lib/backend/mock';
import { mockBackend, settle, windowState } from '$lib/testing/setup';
import { CLOSE_AFTER_MS } from '$lib/window/download-window.svelte';
import DownloadWindow from './DownloadWindow.svelte';

async function started(url = 'https://example.com/small.zip') {
  const b = mockBackend({ empty: true });
  const id = await b.addDownload({ url, queue: null, folder: null, file_name: null, start: true });
  const { win, host } = windowState(b, { download: id });
  await win.init();
  render(DownloadWindow, { props: { win } });
  await settle();
  return { b, id, win, host };
}

async function finish(b: MockBackend, id: DownloadId) {
  for (let i = 0; i < 400 && (await b.getDownload(id)).status !== 'completed'; i++) b.tick(1000);
  await settle();
}

/** Where the focus is, in words a failure message can show. */
function focusedName(): string {
  const active = document.activeElement;
  if (!active || active === document.body) return 'the page';
  return active.getAttribute('aria-label') ?? active.textContent.trim();
}

describe('"Cancel this download?"', () => {
  it('goes when the download finishes, so the finished file cannot be deleted from it', async () => {
    const { b, id } = await started();
    const user = userEvent.setup();
    await user.click(screen.getByRole('button', { name: 'Cancel' }));
    await settle();
    expect(screen.getByRole('button', { name: 'Keep downloading' })).toHaveFocus();
    const remove = vi.spyOn(b, 'remove');
    await finish(b, id);
    expect(screen.getByRole('status')).toHaveTextContent('Finished');
    expect(screen.queryByRole('button', { name: 'Cancel download' })).toBeNull();
    expect(remove).not.toHaveBeenCalled();
    expect((await b.listDownloads()).some((r) => r.id === id)).toBe(true);
  });

  it('goes when the download fails, so the error shows instead', async () => {
    const { win } = await started();
    await userEvent.setup().click(screen.getByRole('button', { name: 'Cancel' }));
    await settle();
    const row = win.row!;
    win.handle({
      type: 'downloads_changed',
      data: [
        {
          ...row,
          status: 'failed',
          error: { message: 'The connection dropped.', fix: 'retry' },
        },
      ],
    });
    await settle();
    expect(screen.queryByRole('button', { name: 'Keep downloading' })).toBeNull();
  });

  it('gives the focus back to Cancel when the person keeps downloading', async () => {
    await started('https://example.com/big.iso');
    const user = userEvent.setup();
    await user.click(screen.getByRole('button', { name: 'Cancel' }));
    await settle();
    await user.keyboard('{Enter}');
    await settle();
    expect(screen.queryByRole('button', { name: 'Keep downloading' })).toBeNull();
    expect(focusedName()).toBe('Cancel');
  });
});

describe('the focus in a progress window', () => {
  it('stays on the main button through Pause and Resume, and Escape still closes the window', async () => {
    const { host } = await started('https://example.com/big.iso');
    const user = userEvent.setup();
    screen.getByRole('button', { name: 'Pause' }).focus();
    await user.keyboard('{Enter}');
    await settle();
    expect(screen.getByRole('button', { name: 'Resume' })).toHaveFocus();
    await user.keyboard('{Enter}');
    await settle();
    expect(screen.getByRole('button', { name: 'Pause' })).toHaveFocus();
    await user.keyboard('{Escape}');
    expect(host.close).toHaveBeenCalledTimes(1);
  });

  it('stays in the window when the buttons change as the download finishes, which then stays open', async () => {
    const { b, id, host } = await started();
    screen.getByRole('button', { name: 'Hide' }).focus();
    vi.useFakeTimers({ shouldAdvanceTime: true });
    await finish(b, id);
    expect(screen.getByRole('button', { name: 'Open' })).toBeInTheDocument();
    expect(focusedName()).not.toBe('the page');
    vi.advanceTimersByTime(CLOSE_AFTER_MS * 3);
    await settle();
    expect(host.close).not.toHaveBeenCalled();
    await fireEvent.keyDown(document.activeElement!, { key: 'Escape' });
    expect(host.close).toHaveBeenCalledTimes(1);
  });

  it('stays in the window when the inline question it was in goes', async () => {
    const { b, id } = await started();
    await userEvent.setup().click(screen.getByRole('button', { name: 'Cancel' }));
    await settle();
    expect(screen.getByRole('button', { name: 'Keep downloading' })).toHaveFocus();
    await finish(b, id);
    expect(focusedName()).not.toBe('the page');
  });
});

describe('a progress window’s Cancel', () => {
  it('never deletes a finished file, even when the window has not heard yet', async () => {
    const { b, id, win } = await started();
    const shown = win.row!;
    await finish(b, id);
    // The window still shows the download as running.
    win.handle({ type: 'downloads_changed', data: [shown] });
    const remove = vi.spyOn(b, 'remove');
    await win.progress.cancelDownload();
    for (const [, deleteFile] of remove.mock.calls) expect(deleteFile).toBe(false);
  });
});
