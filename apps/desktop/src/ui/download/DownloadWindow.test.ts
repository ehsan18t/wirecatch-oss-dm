// A download window with the keyboard (D23): Enter is Download now, Escape closes without
// answering, and a finished download's window closes by itself unless the person is using it.

import { questionId } from '@wirecatch/protocol';
import { fireEvent, render, screen } from '@testing-library/svelte';
import userEvent from '@testing-library/user-event';
import { describe, expect, it, vi } from 'vitest';
import type { MockBackend } from '$lib/backend/mock';
import { BackendError } from '$lib/backend';
import { mockBackend, NOON, settle, windowState } from '$lib/testing/setup';
import { HOURS_PER_DAY, MS_PER_HOUR } from '$lib/time';
import { CLOSE_AFTER_MS, type Target } from '$lib/window/download-window.svelte';
import DownloadWindow from './DownloadWindow.svelte';

async function show(b: MockBackend, target: Target) {
  const { win, host } = windowState(b, target);
  await win.init();
  render(DownloadWindow, { props: { win } });
  await settle();
  return { win, host };
}

describe('a download window with the keyboard', () => {
  it('starts with the focus on Download now, so Enter downloads', async () => {
    const b = mockBackend({ empty: true });
    b.simulateConfirmation();
    const [asked] = await b.pendingConfirmations();
    const { win, host } = await show(b, { question: asked?.id ?? questionId(0) });
    expect(screen.getByRole('button', { name: 'Download now' })).toHaveFocus();
    // Every control has a name a screen reader can say.
    expect(screen.getByRole('textbox', { name: 'File name' })).toBeInTheDocument();
    expect(screen.getByRole('combobox', { name: 'Queue' })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Change' })).toBeInTheDocument();
    expect(
      screen.getByRole('checkbox', { name: 'Don’t ask again for files from the browser' }),
    ).not.toBeChecked();
    // Worded as Settings words it: on, as it is by default.
    expect(
      screen.getByRole('checkbox', { name: 'Keep the window open to show progress' }),
    ).toBeChecked();
    // Enter in the file name field downloads too.
    await userEvent.setup().type(screen.getByRole('textbox', { name: 'File name' }), '{Enter}');
    await settle();
    expect(host.becameDownload).toHaveBeenCalledTimes(1);
    expect(win.phase).toBe('progress');
  });

  it('has a title bar whose one caption button closes the window and leaves the question waiting', async () => {
    const b = mockBackend({ empty: true });
    b.simulateConfirmation();
    const [asked] = await b.pendingConfirmations();
    const { host } = await show(b, { question: asked?.id ?? questionId(0) });
    expect(screen.getByText('Wirecatch')).toBeInTheDocument();
    expect(screen.queryByRole('button', { name: 'Minimize' })).toBeNull();
    expect(screen.queryByRole('button', { name: /^(Maximize|Restore)$/ })).toBeNull();
    await userEvent.setup().click(screen.getByRole('button', { name: 'Close' }));
    expect(host.close).toHaveBeenCalledTimes(1);
    expect(await b.pendingConfirmations()).toHaveLength(1);
  });

  it('closes with Escape and leaves the question waiting', async () => {
    const b = mockBackend({ empty: true });
    b.simulateMediaChoice();
    const [choice] = await b.pendingMediaChoices();
    const { host } = await show(b, { question: choice?.id ?? questionId(0) });
    expect(screen.getByRole('heading', { name: 'Pick a quality' })).toBeInTheDocument();
    // The best quality needs the media add-on, and the main button says so.
    const main = screen.getByRole('button', { name: 'Install and download' });
    expect(main).toHaveFocus();
    await fireEvent.keyDown(main, {
      key: 'Escape',
    });
    expect(host.close).toHaveBeenCalledTimes(1);
    expect(await b.pendingMediaChoices()).toHaveLength(1);
  });

  it('downloads with Enter on a checkbox, which Space checks', async () => {
    const b = mockBackend({ empty: true });
    b.simulateConfirmation();
    const [asked] = await b.pendingConfirmations();
    const { win, host } = await show(b, { question: asked?.id ?? questionId(0) });
    const user = userEvent.setup();
    const keepOpen = screen.getByRole('checkbox', {
      name: 'Keep the window open to show progress',
    });
    keepOpen.focus();
    await user.keyboard(' ');
    await settle();
    expect(keepOpen).not.toBeChecked();
    expect(host.becameDownload).not.toHaveBeenCalled();
    await user.keyboard('{Enter}');
    await settle();
    expect(host.becameDownload).toHaveBeenCalledTimes(1);
    expect(win.phase).not.toBe('asking');
  });

  it('says what went wrong next to the buttons, outside the part that scrolls', async () => {
    const b = mockBackend({ empty: true });
    b.simulateConfirmation();
    const [asked] = await b.pendingConfirmations();
    await show(b, { question: asked?.id ?? questionId(0) });
    vi.spyOn(b, 'confirmCapture').mockRejectedValue(new BackendError('The disk is full.'));
    const now = screen.getByRole('button', { name: 'Download now' });
    await userEvent.setup().click(now);
    await settle();
    const alert = screen.getByRole('alert');
    expect(alert).toHaveTextContent('The disk is full.');
    expect(alert.closest('.overflow-y-auto')).toBeNull();
    expect(alert.compareDocumentPosition(now) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
  });

  it('asks before Cancel removes a download, and Escape keeps it', async () => {
    const b = mockBackend({ empty: true });
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
    await user.click(screen.getByRole('button', { name: 'Cancel download' }));
    await settle();
    expect(host.close).toHaveBeenCalledTimes(1);
    expect((await b.listDownloads()).some((r) => r.id === id)).toBe(false);
  });
});

describe('a finished download’s window', () => {
  async function finished() {
    const b = mockBackend({ empty: true });
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

  it('stays while the shell asks whether to open the file, which takes the focus away', async () => {
    const { b, id } = await finished();
    let answer: () => void = () => undefined;
    vi.spyOn(b, 'openFile').mockReturnValue(
      new Promise<void>((resolve) => (answer = () => resolve())),
    );
    vi.useFakeTimers({ shouldAdvanceTime: true });
    const { host } = await show(b, { download: id });
    await fireEvent.click(screen.getByRole('button', { name: 'Open' }));
    await settle();
    vi.advanceTimersByTime(CLOSE_AFTER_MS * 3);
    expect(host.close).not.toHaveBeenCalled();
    answer();
    await settle();
    vi.advanceTimersByTime(CLOSE_AFTER_MS + 100);
    expect(host.close).toHaveBeenCalledTimes(1);
  });

  it('stays while the pointer is over it', async () => {
    const { b, id } = await finished();
    vi.useFakeTimers({ shouldAdvanceTime: true });
    const { host } = await show(b, { download: id });
    const window = screen.getByRole('button', { name: 'Open' }).closest('[data-progress-window]');
    await fireEvent.mouseEnter(window!);
    await settle();
    vi.advanceTimersByTime(CLOSE_AFTER_MS * 3);
    expect(host.close).not.toHaveBeenCalled();
    await fireEvent.mouseLeave(window!);
    await settle();
    vi.advanceTimersByTime(CLOSE_AFTER_MS + 100);
    expect(host.close).toHaveBeenCalledTimes(1);
  });
});

describe('a download window and the clock', () => {
  it('says when the file was downloaded before by the clock the app follows', async () => {
    // The sample backend runs at NOON on 10 October 2026; the machine's clock says ten days later.
    vi.setSystemTime(NOON + 10 * HOURS_PER_DAY * MS_PER_HOUR);
    const b = mockBackend({ empty: true });
    b.simulateConfirmation();
    const [first] = await b.pendingConfirmations();
    const id = await b.confirmCapture(first?.id ?? questionId(0), {
      file_name: null,
      folder: null,
      queue: null,
      later: false,
    });
    for (let i = 0; i < 2000 && (await b.getDownload(id)).status !== 'completed'; i++) b.tick(5000);
    // The mock sends four sample files in turn; the fifth is the first one again.
    for (let i = 0; i < 4; i++) b.simulateConfirmation();
    const again = (await b.pendingConfirmations()).at(-1);
    await show(b, { question: again?.id ?? questionId(0) });
    expect(screen.getByRole('note')).toHaveTextContent(/^You downloaded this today at /);
  });
});

describe('why a download goes to its queue', () => {
  it("says the core's reason, the person's pick once changed, and keeps it while it runs", async () => {
    const b = mockBackend({ empty: true });
    b.simulateConfirmation();
    const [asked] = await b.pendingConfirmations();
    await show(b, { question: asked?.id ?? questionId(0) });
    const queue = screen.getByRole('combobox', { name: 'Queue' });
    expect(queue).toHaveAccessibleDescription("Queue: Main. It is the browser's queue.");
    const user = userEvent.setup();
    await user.click(queue);
    await user.click(screen.getByRole('option', { name: /^Overnight/ }));
    await settle();
    expect(queue).toHaveAccessibleDescription('Queue: Overnight. You picked it.');
    await user.click(screen.getByRole('button', { name: 'Download now' }));
    await settle();
    expect(screen.getByText('Overnight').closest('dd')).toHaveTextContent('You picked it.');
  });
});
