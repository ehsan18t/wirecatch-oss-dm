// Regression tests for the WP4 review findings. Each test fails without its fix.

import { queueId } from '@wirecatch/protocol';
import { fireEvent, screen, within } from '@testing-library/svelte';
import userEvent from '@testing-library/user-event';
import { describe, expect, it, vi } from 'vitest';
import type { MockBackend } from './backend/mock';
import { buildCommands, searchCommands } from './commands';
import {
  ASSUMED_GROUP_HEIGHT as GROUP_HEIGHT,
  ASSUMED_ROW_HEIGHT as ROW_HEIGHT,
} from '../ui/list/viewport';
import {
  appState,
  downloadWindow,
  mockBackend,
  renderApp,
  rowByName,
  settle,
} from './testing/setup';

async function setup(options: Parameters<typeof renderApp>[0] = {}) {
  const r = await renderApp(options);
  const user = userEvent.setup();
  const click = async (el: HTMLElement) => {
    await user.click(el);
    await settle();
  };
  return { ...r, user, click };
}

const button = (name: RegExp | string) => screen.getByRole('button', { name });
/** The details dialog, open for the download whose name starts with `name`. */
const details = (name: string) =>
  within(screen.getByRole('dialog', { name: new RegExp(`^${name}`) }));
/** Opens a download's details: a click on its row. */
const showDetails = (name: string) => rowByName(name);

describe('2. the list does not jump back to the keyboard row', () => {
  it('keeps the scroll position when a status changes elsewhere', async () => {
    const { app, backend } = await setup();
    app.selection.only(app.visible[0]!);
    await settle();
    const grid = screen.getByRole('grid', { name: 'Downloads' });
    const writes = vi.fn();
    Object.defineProperty(grid, 'scrollTop', { configurable: true, get: () => 2000, set: writes });
    // Another running download, not the keyboard row: it moves to the Paused group.
    const running = app.visible.find(
      (id, i) => i > 1 && app.downloads.get(id)?.status === 'downloading',
    );
    await backend.pause(running!);
    await settle();
    expect(writes).not.toHaveBeenCalled();
    // Moving the keyboard row still brings it into view.
    app.selection.move(1, false);
    await settle();
    // Under the first group's heading.
    expect(writes).toHaveBeenCalledWith(GROUP_HEIGHT + ROW_HEIGHT);
  });
});

describe('3. closing a download window keeps the download waiting', () => {
  it('only Cancel discards; Escape closes the window and Waiting for you reopens it', async () => {
    const { app, backend, user, click } = await setup();
    const reject = vi.spyOn(backend, 'rejectCapture');
    backend.simulateConfirmation();
    const window = await downloadWindow();
    expect(window).toHaveTextContent('Download this file?');
    // The window has the focus, on Download now, so Enter would download.
    expect(within(window).getByRole('button', { name: 'Download now' })).toHaveFocus();
    await user.keyboard('{Escape}');
    await settle();
    expect(screen.queryByRole('region', { name: /^Wirecatch download window/ })).toBeNull();
    expect(app.questions.pending).toHaveLength(1);
    expect(reject).not.toHaveBeenCalled();
    expect(buildCommands(app).some((c) => c.id === 'questions.show')).toBe(true);
    await click(button(/waiting for you/i));
    const again = await downloadWindow();
    expect(again).toHaveTextContent('Download this file?');
    await click(within(again).getByRole('button', { name: 'Cancel' }));
    expect(reject).toHaveBeenCalledTimes(1);
    expect(app.questions.pending).toHaveLength(0);
    expect(screen.queryByRole('region', { name: /^Wirecatch download window/ })).toBeNull();
  });
});

describe('4 and 12. destructive actions ask first, with focus on the safe choice', () => {
  it('cancel asks, focuses the choice to keep it, and only then discards', async () => {
    const { app, click } = await setup();
    await click(showDetails('LibreOffice'));
    const id = app.selection.rows[0]!.id;
    await click(details('LibreOffice').getByRole('button', { name: 'More actions' }));
    await click(screen.getByRole('menuitem', { name: 'Cancel download' }));
    // It waits in line, so nothing downloads to keep: the safe choice keeps it.
    expect(document.activeElement).toBe(button('Keep it'));
    expect(app.downloads.get(id)?.status).not.toBe('cancelled');
    await click(button('Cancel download'));
    expect(app.downloads.get(id)?.status).toBe('cancelled');
  });

  it('remove focuses Keep', async () => {
    const { click } = await setup();
    await click(showDetails('LibreOffice'));
    await click(details('LibreOffice').getByRole('button', { name: 'Remove' }));
    expect(document.activeElement).toBe(button('Keep'));
  });
});

describe('5. waiting downloads say why', () => {
  it('in the row (short) and the details (full)', async () => {
    const { app, click } = await setup();
    const debian = rowByName('debian-13.1.0');
    expect(debian).toHaveTextContent('Starts 01:00');
    await click(showDetails('debian-13.1.0'));
    expect(
      details('debian-13.1.0').getByText('Waiting for Overnight, which starts at 01:00.'),
    ).toBeInTheDocument();
    const reasons = app.visible.map((id) => app.downloads.get(id)?.waiting?.type).filter(Boolean);
    expect(reasons).toContain('queue_full');
    expect(reasons).toContain('in_line');
    expect(screen.getAllByRole('row').some((r) => r.textContent.includes('2nd in line'))).toBe(
      true,
    );
  });
});

describe('6. queues pause for real and scheduled queues can start now', () => {
  it('pauses the queue itself, then resumes it', async () => {
    const { app, backend, click } = await setup();
    const setPaused = vi.spyOn(backend, 'setQueuePaused');
    await click(button('Actions for Main'));
    await click(screen.getByRole('menuitem', { name: 'Pause the queue' }));
    expect(setPaused).toHaveBeenCalledWith(1, true);
    expect(app.queues.byId(queueId(1))?.status.paused).toBe(true);
    const main = screen.getByRole('button', { name: /^Main/ });
    expect(main).toHaveTextContent('Paused');
    const inMain = app.downloads.idsInQueue(queueId(1)).map((id) => app.downloads.get(id));
    expect(inMain.some((r) => r?.status === 'downloading' || r?.status === 'connecting')).toBe(
      false,
    );
    expect(inMain.some((r) => r?.waiting?.type === 'queue_paused')).toBe(true);
    // Downloads paused one by one are not touched by the queue.
    expect(inMain.filter((r) => r?.status === 'paused').map((r) => r?.file_name)).toEqual([
      'android-studio-2026.1.1-windows.exe',
    ]);
    await click(button('Actions for Main'));
    await click(screen.getByRole('menuitem', { name: 'Resume the queue' }));
    expect(app.queues.byId(queueId(1))?.status.paused).toBe(false);
  });

  it('starts a scheduled queue now and shows it runs outside its hours', async () => {
    const { app, backend, click } = await setup();
    expect(app.queues.isOutsideSchedule(app.queues.byId(queueId(2)) as never)).toBe(true);
    await click(button('Actions for Overnight'));
    await click(screen.getByRole('menuitem', { name: 'Start now (ignore schedule)' }));
    expect(app.queues.byId(queueId(2))?.status.running_outside_schedule).toBe(true);
    expect(screen.getByRole('button', { name: /^Overnight/ })).toHaveTextContent(
      'Started manually, ignoring schedule',
    );
    backend.tick(250);
    await settle();
    const overnight = app.downloads
      .idsInQueue(queueId(2))
      .map((id) => app.downloads.get(id)?.status);
    expect(overnight).toContain('connecting');
  });

  it('the mock ends "start now" once the queue is empty', async () => {
    const backend = mockBackend({ empty: true });
    const q = (await backend.listQueues()).find((x) => x.id === 2);
    await backend.addDownload({
      url: 'https://x.org/a.zip',
      queue: queueId(2),
      folder: null,
      file_name: null,
      start: true,
    });
    await backend.startQueueNow(queueId(2));
    expect(
      (await backend.listQueues()).find((x) => x.id === 2)?.status.running_outside_schedule,
    ).toBe(true);
    for (let i = 0; i < 2000; i++) backend.tick(1000);
    expect(q?.settings.name).toBe('Overnight');
    expect(
      (await backend.listQueues()).find((x) => x.id === 2)?.status.running_outside_schedule,
    ).toBe(false);
  });
});

describe('7. Try again does not double the listeners', () => {
  it('init twice still handles each event once', async () => {
    const { app, backend } = await setup();
    await app.init();
    await settle();
    const asked = vi.spyOn(app.questions, 'confirmationRequested');
    backend.simulateCapture();
    await settle();
    expect(asked).toHaveBeenCalledTimes(1);
  });

  it('dispose before the backend is ready leaves nothing listening', async () => {
    const backend = mockBackend();
    const ready = Promise.withResolvers<MockBackend>();
    const app = appState(() => ready.promise);
    const started = app.init();
    app.dispose();
    ready.resolve(backend);
    await started;
    backend.simulateCapture();
    expect(app.toasts.items).toHaveLength(0);
    expect(app.ready).toBe(false);
  });
});

describe('8. the days picker says what it does', () => {
  it('shows every day pressed, and saves the days left pressed', async () => {
    const { app, click } = await setup();
    app.openDialog({ kind: 'queue', id: queueId(2) });
    await settle();
    const days = screen.getByRole('group', { name: 'Days' });
    expect(within(days).getAllByRole('button', { pressed: true })).toHaveLength(7);
    for (const day of ['Tue', 'Wed', 'Thu', 'Fri', 'Sat', 'Sun']) {
      await click(within(days).getByRole('button', { name: day }));
    }
    expect(
      within(days)
        .getAllByRole('button', { pressed: true })
        .map((b) => b.textContent.trim()),
    ).toEqual(['Mon']);
    await click(within(days).getByRole('button', { name: 'Mon' }));
    expect(screen.getByText('Pick at least one day.')).toBeInTheDocument();
  });

  it('stops at a time, or when the queue is empty', async () => {
    const { app, click } = await setup();
    app.openDialog({ kind: 'queue', id: queueId(2) });
    await settle();
    expect(screen.getByLabelText('Stop time')).toBeInTheDocument();
    await click(screen.getByRole('radio', { name: 'When the queue is empty' }));
    expect(screen.queryByLabelText('Stop time')).toBeNull();
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
    await app.actions.retryAllFailed();
    expect(app.counts.status.failed).toBe(0);
  });

  it('does not match letters scattered across a word', async () => {
    const { app } = await setup();
    app.selection.only(app.visible[0]!);
    const titles = searchCommands(buildCommands(app), 'move').map((c) => c.title);
    expect(titles.length).toBeGreaterThan(0);
    for (const t of titles) expect(t).not.toMatch(/^Resume|^Remove/);
    expect(titles.every((t) => /\bmove/i.test(t))).toBe(true);
  });
});

describe('11. Pause all and Resume all show only when they would do something', () => {
  it('is one button: Pause all while anything runs, then Resume all', async () => {
    const { click } = await setup();
    expect(screen.queryByRole('button', { name: 'Resume all' })).toBeNull();
    await click(button('Pause all'));
    expect(screen.queryByRole('button', { name: 'Pause all' })).toBeNull();
    await click(button('Resume all'));
    expect(button('Pause all')).toBeInTheDocument();
  });
});

describe('13. creating a queue is one atomic step', () => {
  it('a refused save makes no queue, and saving again makes exactly one', async () => {
    const { app, backend, user, click } = await setup();
    const create = vi
      .spyOn(backend, 'createQueue')
      .mockRejectedValueOnce(new Error('Disk is busy.'));
    app.openDialog({ kind: 'queue', id: null });
    await settle();
    await user.type(screen.getByLabelText('Name'), 'Weekend');
    await click(button('Create queue'));
    expect(screen.getByRole('dialog')).toBeInTheDocument();
    expect(app.queues.list.filter((q) => q.settings.name === 'Weekend')).toHaveLength(0);
    await click(button('Create queue'));
    expect(create).toHaveBeenCalledTimes(2);
    expect(app.queues.list.filter((q) => q.settings.name === 'Weekend')).toHaveLength(1);
    expect(screen.queryByRole('dialog')).not.toBeInTheDocument();
  });
});

describe('14. pasting into a text field is just pasting', () => {
  it('Ctrl+V in the search box does not open the Add dialog', async () => {
    const { app } = await setup();
    const search = screen.getByRole('searchbox', { name: 'Search downloads' });
    search.focus();
    await fireEvent.paste(search, {
      clipboardData: { getData: () => 'https://example.com/a.zip' },
    });
    await settle();
    expect(app.dialog).toBeNull();
    await fireEvent.paste(document.body, {
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
    const gates: (() => void)[] = [];
    vi.spyOn(backend, 'pause').mockImplementation(async (id) => {
      calls += 1;
      await new Promise<void>((r) => gates.push(r));
      if (id !== ids[0]) throw new Error('The engine is busy. Try again.');
    });
    const done = app.actions.pause(ids);
    await Promise.resolve();
    expect(calls).toBe(3);
    for (const g of gates) g();
    await done;
    const errors = app.toasts.items.filter((t) => t.kind === 'error');
    expect(errors).toHaveLength(1);
    expect(errors[0]?.message).toBe(
      'Could not pause 2 of 3 downloads. The engine is busy. Try again.',
    );
  });
});

describe('20. a link dropped on the window', () => {
  it('opens the Add dialog with it', async () => {
    const { app } = await setup();
    await fireEvent.drop(window, {
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
    // A right press, as a mouse makes it: the list takes the focus, then the menu opens.
    await user.pointer({ keys: '[MouseRight]', target: rowByName('LibreOffice') });
    await settle();
    const menu = screen.getByRole('menu');
    for (const name of [
      'Pause',
      'Show details',
      'Show in folder',
      'Copy link',
      'Move to Work files',
      'Remove',
    ]) {
      expect(within(menu).getByRole('menuitem', { name })).toBeInTheDocument();
    }
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
    expect(app.selection.rows[0]?.queue).toBe(3);
  });
});

describe('a password in a link is never shown or copied', () => {
  it('the details and Copy link use the hidden form', async () => {
    const { app, backend, click } = await setup({ empty: true });
    // The clipboard is the one user-event puts in place for the test and takes away after it;
    // restoreMocks puts the spied method back.
    const writeText = vi.spyOn(navigator.clipboard, 'writeText');
    const id = await backend.addDownload({
      url: 'https://ada:s3cret@example.com/files/locked.zip',
      queue: null,
      folder: null,
      file_name: null,
      start: false,
    });
    await settle();
    app.openDetails(id);
    await settle();
    expect(screen.getByText('https://ada:***@example.com/files/locked.zip')).toBeInTheDocument();
    expect(document.body.textContent).not.toContain('s3cret');
    await click(
      within(screen.getByRole('dialog', { name: /locked\.zip/ })).getByRole('button', {
        name: 'More actions',
      }),
    );
    await click(screen.getByRole('menuitem', { name: 'Copy link' }));
    expect(writeText).toHaveBeenLastCalledWith('https://ada:***@example.com/files/locked.zip');

    const selected = screen
      .getAllByRole('row')
      .find((r) => r.getAttribute('aria-selected') === 'true');
    if (!selected) throw new Error('the row is not shown');
    await fireEvent.contextMenu(selected);
    await settle();
    await click(within(screen.getByRole('menu')).getByRole('menuitem', { name: 'Copy link' }));
    expect(writeText).toHaveBeenLastCalledWith('https://ada:***@example.com/files/locked.zip');
    expect(writeText.mock.calls.flat().join(' ')).not.toContain('s3cret');
  });
});
