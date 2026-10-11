// Regression tests for the WP4 review findings. Each test fails without its fix.

import { fireEvent, screen, within } from '@testing-library/svelte';
import userEvent from '@testing-library/user-event';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { MockBackend } from './backend/mock';
import { buildCommands, searchCommands } from './commands';
import { AppState } from './state/app.svelte';
import { NOON, renderApp, rowByName, settle } from './testing/setup';

let current: { app: AppState; unmount: () => void } | undefined;
afterEach(() => {
  current?.app.dispose();
  current?.unmount();
  current = undefined;
});

async function setup(options: Parameters<typeof renderApp>[0] = {}) {
  const r = await renderApp(options);
  current = r;
  const user = userEvent.setup();
  const click = async (el: HTMLElement) => {
    await user.click(el);
    await settle();
  };
  return { ...r, user, click };
}

const button = (name: RegExp | string) => screen.getByRole('button', { name });

describe('2. the list does not jump back to the keyboard row', () => {
  it('keeps the scroll position when a status changes elsewhere', async () => {
    const { app, backend } = await setup();
    app.selectOnly(app.visible[0] as number);
    await settle();
    const grid = screen.getByRole('grid', { name: 'Downloads' });
    const writes = vi.fn();
    Object.defineProperty(grid, 'scrollTop', { configurable: true, get: () => 2000, set: writes });
    const running = app.visible.find((id) => app.downloads.get(id)?.status === 'downloading');
    await backend.pause(running as number);
    await settle();
    expect(writes).not.toHaveBeenCalled();
    // Moving the keyboard row still brings it into view.
    app.moveActive(1, false);
    await settle();
    expect(writes).toHaveBeenCalledWith(44);
  });
});

describe('3. closing the ask-first dialog keeps the download waiting', () => {
  it('only "Don\'t download" rejects; Escape puts it aside and it can be reopened', async () => {
    const { app, backend, user, click } = await setup();
    const reject = vi.spyOn(backend, 'rejectCapture');
    backend.simulateConfirmation();
    await settle();
    expect(screen.getByRole('dialog')).toHaveTextContent('Download this file?');
    await user.keyboard('{Escape}');
    await settle();
    expect(screen.queryByRole('dialog')).not.toBeInTheDocument();
    expect(app.pending).toHaveLength(1);
    expect(reject).not.toHaveBeenCalled();
    expect(buildCommands(app).some((c) => c.id === 'pending.answer')).toBe(true);
    await click(button('1 browser download waits for your answer'));
    expect(screen.getByRole('dialog')).toHaveTextContent('Download this file?');
    await click(button("Don't download"));
    expect(reject).toHaveBeenCalledTimes(1);
    expect(app.pending).toHaveLength(0);
  });
});

describe('4 and 12. destructive actions ask first, with focus on the safe choice', () => {
  it('cancel asks, focuses Keep downloading, and only then discards', async () => {
    const { app, click } = await setup();
    await click(rowByName('LibreOffice'));
    const id = app.selectedRows[0]?.id as number;
    await click(button('Cancel and discard the partial file'));
    expect(document.activeElement).toBe(button('Keep downloading'));
    expect(app.downloads.get(id)?.status).not.toBe('cancelled');
    await click(button('Cancel and delete the partial file'));
    expect(app.downloads.get(id)?.status).toBe('cancelled');
  });

  it('remove focuses Keep', async () => {
    const { click } = await setup();
    await click(rowByName('LibreOffice'));
    await click(button('Remove'));
    expect(document.activeElement).toBe(button('Keep'));
  });
});

describe('5. waiting downloads say why', () => {
  it('in the row (short) and the detail panel (full)', async () => {
    const { app, click } = await setup();
    const debian = rowByName('debian-13.1.0');
    expect(debian).toHaveTextContent('Starts 01:00');
    await click(debian);
    expect(screen.getByText('Waiting for Overnight, which starts at 01:00.')).toBeInTheDocument();
    const reasons = app.visible.map((id) => app.downloads.get(id)?.waiting?.type).filter(Boolean);
    expect(reasons).toContain('queue_full');
    expect(reasons).toContain('in_line');
    expect(screen.getAllByRole('row').some((r) => /2nd in line/.test(r.textContent ?? ''))).toBe(
      true,
    );
  });
});

describe('6. queues pause for real and scheduled queues can start now', () => {
  it('pauses the queue itself, then resumes it', async () => {
    const { app, backend, click } = await setup();
    const setPaused = vi.spyOn(backend, 'setQueuePaused');
    await click(button('Pause the Main queue'));
    expect(setPaused).toHaveBeenCalledWith(1, true);
    expect(app.queueById(1)?.paused).toBe(true);
    const main = screen.getByRole('button', { name: /^Main/ });
    expect(main).toHaveTextContent('Paused');
    const inMain = app.downloads.idsInQueue(1).map((id) => app.downloads.get(id));
    expect(inMain.some((r) => r?.status === 'downloading' || r?.status === 'connecting')).toBe(
      false,
    );
    expect(inMain.some((r) => r?.waiting?.type === 'queue_paused')).toBe(true);
    // Downloads paused one by one are not touched by the queue.
    expect(inMain.filter((r) => r?.status === 'paused').map((r) => r?.file_name)).toEqual([
      'android-studio-2026.1.1-windows.exe',
    ]);
    await click(button('Resume the Main queue'));
    expect(app.queueById(1)?.paused).toBe(false);
  });

  it('starts a scheduled queue now and shows it runs outside its hours', async () => {
    const { app, backend, click } = await setup();
    expect(app.isOutsideSchedule(app.queueById(2) as never)).toBe(true);
    await click(button('Start now'));
    expect(app.queueById(2)?.running_outside_schedule).toBe(true);
    expect(screen.getByRole('button', { name: /^Overnight/ })).toHaveTextContent(
      'Started by hand, outside its hours',
    );
    backend.tick(250);
    await settle();
    const overnight = app.downloads.idsInQueue(2).map((id) => app.downloads.get(id)?.status);
    expect(overnight).toContain('connecting');
  });

  it('the mock ends "start now" once the queue is empty', async () => {
    const backend = new MockBackend({ autoTick: false, now: () => NOON, empty: true });
    const q = (await backend.listQueues()).find((x) => x.id === 2);
    await backend.addDownload({
      url: 'https://x.org/a.zip',
      queue: 2,
      folder: null,
      file_name: null,
      start: true,
    });
    await backend.startQueueNow(2);
    expect((await backend.listQueues()).find((x) => x.id === 2)?.running_outside_schedule).toBe(
      true,
    );
    for (let i = 0; i < 2000; i++) backend.tick(1000);
    expect(q?.name).toBe('Overnight');
    expect((await backend.listQueues()).find((x) => x.id === 2)?.running_outside_schedule).toBe(
      false,
    );
  });
});

describe('7. Try again does not double the listeners', () => {
  it('init twice still handles each event once', async () => {
    const { app, backend } = await setup();
    await app.init();
    await settle();
    backend.simulateCapture();
    await settle();
    expect(app.toasts.filter((t) => t.kind === 'capture')).toHaveLength(1);
  });

  it('dispose before the backend is ready leaves nothing listening', async () => {
    const backend = new MockBackend({ autoTick: false, now: () => NOON });
    let release: (b: MockBackend) => void = () => {};
    const app = new AppState(() => new Promise((r) => (release = r)));
    const started = app.init();
    app.dispose();
    release(backend);
    await started;
    backend.simulateCapture();
    expect(app.toasts).toHaveLength(0);
    expect(app.ready).toBe(false);
  });
});

describe('8. the days picker says what it does', () => {
  it('nothing picked means every day; picking limits to those days', async () => {
    const { app, click } = await setup();
    app.openDialog({ kind: 'queue', id: 2 });
    await settle();
    const days = screen.getByRole('group', { name: 'Only on these days' });
    expect(within(days).queryAllByRole('button', { pressed: true })).toHaveLength(0);
    expect(screen.getByText('No days picked, so it runs every day.')).toBeInTheDocument();
    await click(within(days).getByRole('button', { name: 'Mon' }));
    expect(
      within(days)
        .getAllByRole('button', { pressed: true })
        .map((b) => b.textContent?.trim()),
    ).toEqual(['Mon']);
    expect(screen.getByText('Runs only on Mon.')).toBeInTheDocument();
  });
});

describe('10. the palette', () => {
  it('can retry all failed downloads and finds settings by common words', async () => {
    const { app } = await setup();
    const all = buildCommands(app);
    expect(all.some((c) => c.id === 'dl.retryfailed')).toBe(true);
    for (const word of ['windows', 'startup', 'login']) {
      expect(searchCommands(all, word).some((c) => c.id === 'setting.start_with_system')).toBe(
        true,
      );
    }
    await app.retryAllFailed();
    expect(app.counts.status.failed).toBe(0);
  });

  it('does not match letters scattered across a word', async () => {
    const { app } = await setup();
    app.selectOnly(app.visible[0] as number);
    const titles = searchCommands(buildCommands(app), 'move').map((c) => c.title);
    expect(titles.length).toBeGreaterThan(0);
    for (const t of titles) expect(t).not.toMatch(/^Resume|^Remove/);
    expect(titles.every((t) => /\bmove/i.test(t))).toBe(true);
  });
});

describe('11. Pause all and Resume all only work when they would do something', () => {
  it('are disabled with nothing to pause or resume', async () => {
    const { click } = await setup();
    expect(button('Resume all')).toBeEnabled();
    await click(button('Pause all'));
    expect(button('Pause all')).toBeDisabled();
    await click(button('Resume all'));
    expect(button('Resume all')).toBeDisabled();
  });
});

describe('13. creating a queue is one atomic step', () => {
  it('a failed save retried does not create a second queue', async () => {
    const { app, backend, user, click } = await setup();
    const create = vi.spyOn(backend, 'createQueue');
    vi.spyOn(backend, 'updateQueue').mockRejectedValueOnce(new Error('Disk is busy.'));
    app.openDialog({ kind: 'queue', id: null });
    await settle();
    await user.type(screen.getByLabelText('Name'), 'Weekend');
    await click(button('Create queue'));
    expect(screen.getByRole('dialog')).toHaveTextContent('Save again to retry');
    await click(button('Save'));
    expect(create).toHaveBeenCalledTimes(1);
    expect(app.queues.filter((q) => q.name === 'Weekend')).toHaveLength(1);
    expect(screen.queryByRole('dialog')).not.toBeInTheDocument();
  });
});

describe('14. pasting into a text field is just pasting', () => {
  it('Ctrl+V in the search box does not open the Add dialog', async () => {
    const { app } = await setup();
    const search = screen.getByRole('searchbox', { name: 'Search downloads' });
    search.focus();
    fireEvent.paste(search, { clipboardData: { getData: () => 'https://example.com/a.zip' } });
    await settle();
    expect(app.dialog).toBeNull();
    fireEvent.paste(document.body, {
      clipboardData: { getData: () => 'https://example.com/a.zip' },
    });
    await settle();
    expect(app.dialog?.kind).toBe('add');
  });
});

describe('16. bulk actions run together and report once', () => {
  it('starts every request at once and sums up failures in one note', async () => {
    const { app, backend } = await setup();
    const ids = app.visible.slice(0, 3);
    let calls = 0;
    const gates: Array<() => void> = [];
    vi.spyOn(backend, 'pause').mockImplementation(async (id) => {
      calls += 1;
      await new Promise<void>((r) => gates.push(r));
      if (id !== ids[0]) throw new Error('The engine is busy. Try again.');
    });
    const done = app.pause(ids);
    await Promise.resolve();
    expect(calls).toBe(3);
    for (const g of gates) g();
    await done;
    const errors = app.toasts.filter((t) => t.kind === 'error');
    expect(errors).toHaveLength(1);
    expect(errors[0]?.message).toBe(
      'Could not pause 2 of 3 downloads. The engine is busy. Try again.',
    );
  });
});

describe('20. a link dropped on the window', () => {
  it('opens the Add dialog with it', async () => {
    const { app } = await setup();
    fireEvent.drop(window, {
      dataTransfer: {
        types: ['text/uri-list'],
        getData: (t: string) => (t === 'text/uri-list' ? 'https://example.org/big.iso' : ''),
      },
    });
    await settle();
    expect(app.dialog).toEqual({ kind: 'add', urls: ['https://example.org/big.iso'] });
  });
});

describe('21. right-click menu on rows', () => {
  it('mirrors the visible actions, by mouse and by keyboard', async () => {
    const { app, user, click } = await setup();
    fireEvent.contextMenu(rowByName('LibreOffice'));
    await settle();
    const menu = screen.getByRole('menu');
    const labels = within(menu)
      .getAllByRole('menuitem')
      .map((m) => m.textContent?.trim());
    expect(labels).toEqual(
      expect.arrayContaining([
        'Pause',
        'Show in folder',
        'Copy link',
        'Move to Work files',
        'Remove',
      ]),
    );
    expect(document.activeElement).toBe(within(menu).getAllByRole('menuitem')[0]);
    await user.keyboard('{Escape}');
    await settle();
    expect(screen.queryByRole('menu')).not.toBeInTheDocument();
    expect(document.activeElement).toBe(screen.getByRole('grid', { name: 'Downloads' }));

    await user.keyboard('{Shift>}{F10}{/Shift}');
    await settle();
    await click(
      within(screen.getByRole('menu')).getByRole('menuitem', { name: 'Move to Work files' }),
    );
    expect(app.selectedRows[0]?.queue).toBe(3);
  });
});

describe('a password in a link is never shown or copied', () => {
  it('the detail panel and Copy link use the hidden form', async () => {
    const { app, backend, click } = await setup({ empty: true });
    const writeText = vi.fn(async () => {});
    Object.defineProperty(navigator, 'clipboard', {
      configurable: true,
      value: { writeText },
    });
    const id = await backend.addDownload({
      url: 'https://ada:s3cret@example.com/files/locked.zip',
      queue: null,
      folder: null,
      file_name: null,
      start: false,
    });
    await settle();
    app.selectOnly(id);
    await settle();
    expect(screen.getByText('https://ada:***@example.com/files/locked.zip')).toBeInTheDocument();
    expect(document.body.textContent).not.toContain('s3cret');
    await click(button('Copy link'));
    expect(writeText).toHaveBeenLastCalledWith('https://ada:***@example.com/files/locked.zip');

    const selected = screen
      .getAllByRole('row')
      .find((r) => r.getAttribute('aria-selected') === 'true');
    if (!selected) throw new Error('the row is not shown');
    fireEvent.contextMenu(selected);
    await settle();
    await click(within(screen.getByRole('menu')).getByRole('menuitem', { name: 'Copy link' }));
    expect(writeText).toHaveBeenLastCalledWith('https://ada:***@example.com/files/locked.zip');
    expect(writeText.mock.calls.flat().join(' ')).not.toContain('s3cret');
  });
});
