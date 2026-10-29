// The download list as people use it: where it keeps the keyboard, what waiting rows say, pasting and dropping links, bulk and right-click actions, and links that carry a password.

import { fireEvent, screen, within } from '@testing-library/svelte';
import { describe, expect, it, vi } from 'vitest';
import {
  ASSUMED_GROUP_HEIGHT as GROUP_HEIGHT,
  ASSUMED_ROW_HEIGHT as ROW_HEIGHT,
} from '../ui/list/viewport';
import { button, detailsOf, rowByName, settle, setupWithUser } from './testing/setup';

describe('the list does not jump back to the keyboard row', () => {
  it('keeps the scroll position when a status changes elsewhere', async () => {
    const { app, backend } = await setupWithUser();
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

describe('waiting downloads say why', () => {
  it('in the row (short) and the details (full)', async () => {
    const { app, click } = await setupWithUser();
    const debian = rowByName('debian-13.1.0');
    expect(debian).toHaveTextContent('Starts 01:00');
    await click(rowByName('debian-13.1.0'));
    expect(
      detailsOf('debian-13.1.0').getByText('Waiting for Overnight, which starts at 01:00.'),
    ).toBeInTheDocument();
    const reasons = app.visible.map((id) => app.downloads.get(id)?.waiting?.type).filter(Boolean);
    expect(reasons).toContain('queue_full');
    expect(reasons).toContain('in_line');
    expect(screen.getAllByRole('row').some((r) => r.textContent.includes('2nd in line'))).toBe(
      true,
    );
  });
});

describe('Pause all and Resume all show only when they would do something', () => {
  it('is one button: Pause all while anything runs, then Resume all', async () => {
    const { click } = await setupWithUser();
    expect(screen.queryByRole('button', { name: 'Resume all' })).toBeNull();
    await click(button('Pause all'));
    expect(screen.queryByRole('button', { name: 'Pause all' })).toBeNull();
    await click(button('Resume all'));
    expect(button('Pause all')).toBeInTheDocument();
  });
});

describe('pasting into a text field is just pasting', () => {
  it('Ctrl+V in the search box does not open the Add dialog', async () => {
    const { app } = await setupWithUser();
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

describe('bulk actions run together and report once', () => {
  it('starts every request at once and sums up failures in one note', async () => {
    const { app, backend } = await setupWithUser();
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

describe('a link dropped on the window', () => {
  it('opens the Add dialog with it', async () => {
    const { app } = await setupWithUser();
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

describe('right-click menu on rows', () => {
  it('mirrors the visible actions, by mouse and by keyboard', async () => {
    const { app, user, click } = await setupWithUser();
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
    const { app, backend, click } = await setupWithUser({ empty: true });
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
