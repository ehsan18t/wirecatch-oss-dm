import { downloadId, type DownloadId } from '@wirecatch/protocol';
import { fireEvent, screen, within } from '@testing-library/svelte';
import userEvent from '@testing-library/user-event';
import { describe, expect, it, vi } from 'vitest';
import { renderApp, rowByName, settle } from '$lib/testing/setup';
import { ASSUMED_ROW_HEIGHT as ROW_HEIGHT } from './viewport';

const grid = () => screen.getByRole('grid', { name: 'Downloads' });
const rendered = () =>
  within(grid())
    .getAllByRole('row')
    .filter((r) => r.dataset.id !== undefined);
const idOf = (row: HTMLElement) => downloadId(Number(row.dataset.id));

async function setup(options: Parameters<typeof renderApp>[0] = {}) {
  const r = await renderApp(options);
  const user = userEvent.setup();
  const press = async (keys: string) => {
    await user.keyboard(keys);
    await settle();
  };
  return { ...r, user, press };
}

async function scrollTo(top: number) {
  grid().scrollTop = top;
  await fireEvent.scroll(grid());
  await settle();
}

describe('the download list', () => {
  it('is a named grid with column headers, every row counted after them', async () => {
    const { app } = await setup();
    const list = grid();
    expect(list).toHaveAttribute('aria-multiselectable', 'true');
    // Every download and every group heading, after the column headings.
    const headings = app.layout.sections.filter((x) => x.group !== null).length;
    expect(list).toHaveAttribute('aria-rowcount', String(app.visible.length + headings + 1));
    const headers = within(list)
      .getAllByRole('columnheader')
      .map((h) => h.textContent.trim());
    expect(headers).toEqual(['#', 'Type', 'Name', 'Progress', 'Size', 'Speed', 'Left', 'Actions']);
    // The first group's heading is the first row after the column headings.
    expect(rendered()[0]).toHaveAttribute('aria-rowindex', '3');
  });

  it('moves and selects with the keyboard: arrows, Shift to extend, Home and End, Ctrl+A', async () => {
    const { app, press } = await setup();
    grid().focus();
    await press('{ArrowDown}');
    const first = app.visible[0];
    expect(app.selection.active).toBe(first);
    expect(grid()).toHaveAttribute('aria-activedescendant', `row-${String(first)}`);
    expect(rowByName(app.downloads.get(first!)!.file_name)).toHaveAttribute(
      'aria-selected',
      'true',
    );
    await press('{Shift>}{ArrowDown}{ArrowDown}{/Shift}');
    expect(app.selection.size).toBe(3);
    await press('{End}');
    expect(app.selection.active).toBe(app.visible.at(-1));
    expect(app.selection.size).toBe(1);
    await press('{Home}');
    expect(app.selection.active).toBe(first);
    await press('{Control>}a{/Control}');
    expect(app.selection.size).toBe(app.visible.length);
  });

  it('points at the first row on screen when it gets the keyboard with no row of its own', async () => {
    const { app } = await setup();
    expect(app.selection.active).toBeNull();
    grid().focus();
    await settle();
    const first = rendered()[0]!;
    expect(grid()).toHaveAttribute('aria-activedescendant', first.id);
    // Pointed at, as Explorer does, not selected.
    expect(app.selection.size).toBe(0);
  });

  it('tells a screen reader what Space paused or resumed, since those rows may not be the one it reads', async () => {
    const { app, press } = await setup();
    const running = app.visible.filter((id) => app.downloads.get(id)?.status === 'downloading');
    app.selection.only(running[0]!);
    app.selection.toggle(running[1]!);
    grid().focus();
    await press(' ');
    await settle();
    expect(app.toasts.toaster.announcement).toMatchObject({
      text: 'Paused 2 downloads',
      assertive: false,
    });
    await press(' ');
    await settle();
    expect(app.toasts.toaster.announcement?.text).toBe('Resumed 2 downloads');
    // Nothing shows on screen for it: the rows already say so.
    expect(app.toasts.items).toHaveLength(0);
  });

  it('says nothing on Space when nothing in the selection can pause or resume', async () => {
    const { app, press } = await setup();
    app.folded.clear();
    await settle();
    const finished = app.visible.find((id) => app.downloads.get(id)?.status === 'completed');
    app.selection.only(finished!);
    grid().focus();
    await press(' ');
    await settle();
    expect(app.toasts.toaster.announcement).toBeNull();
    expect(app.downloads.get(finished!)?.status).toBe('completed');
  });

  it('counts on Space only the downloads that took the action', async () => {
    const { app, backend, press } = await setup();
    app.folded.clear();
    await settle();
    const running = app.visible.filter((id) => app.downloads.get(id)?.status === 'downloading');
    const finished = app.visible.find((id) => app.downloads.get(id)?.status === 'completed');
    const [a, b, c] = running;
    app.selection.only(a!);
    app.selection.toggle(b!);
    app.selection.toggle(c!);
    app.selection.toggle(finished!);
    const pause = backend.pause.bind(backend);
    vi.spyOn(backend, 'pause').mockImplementation((id) =>
      id === c ? Promise.reject(new Error('The download could not be paused.')) : pause(id),
    );
    grid().focus();
    await press(' ');
    await settle();
    expect(app.toasts.toaster.announcement?.text).toBe('Paused 2 downloads');
  });

  it('selects one with a click, a range with Shift, and one more or less with Ctrl', async () => {
    const { app, user } = await setup();
    const rows = rendered();
    const [a, b, c, d] = rows;
    if (!a || !b || !c || !d) throw new Error('too few rows');
    await user.click(a);
    expect(app.selection.list).toEqual([idOf(a)]);
    await user.keyboard('{Shift>}');
    await user.click(c);
    await user.keyboard('{/Shift}');
    expect(app.selection.list.toSorted()).toEqual([idOf(a), idOf(b), idOf(c)].toSorted());
    await user.keyboard('{Control>}');
    await user.click(b);
    await user.click(d);
    await user.keyboard('{/Control}');
    expect(app.selection.list.toSorted()).toEqual([idOf(a), idOf(c), idOf(d)].toSorted());
    expect(b).toHaveAttribute('aria-selected', 'false');
  });

  it('acts on a row from its own buttons without changing what is selected', async () => {
    const { app, user } = await setup();
    const libre = rowByName('LibreOffice');
    await user.click(rowByName('ubuntu-24.04.3'));
    const selected = app.selection.list;
    await user.click(within(libre).getByRole('button', { name: 'Pause' }));
    await settle();
    expect(app.downloads.get(idOf(libre))?.status).toBe('paused');
    expect(app.selection.list).toEqual(selected);
    // The buttons are for the pointer: the keys and the menu reach the same actions.
    expect(within(libre).getByRole('button', { name: 'More actions' })).toHaveAttribute(
      'tabindex',
      '-1',
    );
  });

  it('opens its menu from the keyboard about the keyboard row', async () => {
    const { app, press } = await setup();
    grid().focus();
    await press('{ArrowDown}{ArrowDown}');
    const active = app.selection.active;
    await press('{Shift>}{F10}{/Shift}');
    const menu = screen.getByRole('menu', { name: 'Actions for the selected downloads' });
    expect(app.selection.list).toEqual([active]);
    await press('{Escape}');
    expect(screen.queryByRole('menu')).toBeNull();
    expect(document.activeElement).toBe(grid());
    expect(menu.isConnected).toBe(false);
  });

  it('draws only the rows on screen, and keeps a row that stays on screen while scrolling', async () => {
    const { app } = await setup({ count: 500 });
    app.folded.clear();
    await settle();
    expect(app.visible.length).toBe(500);
    const before = rendered();
    expect(before.length).toBeLessThan(40);
    const staying = before[8];
    if (!staying) throw new Error('too few rows');
    await scrollTo(10 * ROW_HEIGHT);
    const after = rendered();
    expect(after.map(idOf)).toContain(app.visible[10]);
    expect(after.length).toBeLessThan(40);
    // Keyed by download: the same element, not a new one with other contents.
    expect(after.find((r) => r.dataset.id === staying.dataset.id)).toBe(staying);
    const far = 300;
    await scrollTo(far * ROW_HEIGHT);
    expect(rendered().map(idOf)).toContain(app.visible[far]);
    // Each drawn row still says where it is in the whole list, the headings above it counted.
    const top = rendered()[0]!;
    const index = app.visible.indexOf(idOf(top));
    const above = app.layout.sections.filter((x) => x.group !== null && x.start <= index).length;
    expect(top).toHaveAttribute('aria-rowindex', String(index + above + 2));
  });

  it('points at the keyboard row only while it is drawn (DUI-9)', async () => {
    const { app } = await setup({ count: 500 });
    app.selection.only(app.visible[0]!);
    await settle();
    expect(grid()).toHaveAttribute('aria-activedescendant', `row-${String(app.visible[0])}`);
    await scrollTo(300 * ROW_HEIGHT);
    expect(grid()).not.toHaveAttribute('aria-activedescendant');
  });

  it('redraws only the row whose progress moved', async () => {
    const { app } = await setup();
    const running = app.visible
      .map((id) => app.downloads.get(id))
      .filter((r) => r?.status === 'downloading');
    const [moving] = running;
    if (!moving || running.length < 2) throw new Error('too few running downloads');
    const touched = new Set<DownloadId | undefined>();
    const observer = new MutationObserver((records) => {
      for (const record of records) {
        const node = record.target instanceof Element ? record.target : record.target.parentElement;
        const row = node?.closest<HTMLElement>('[data-id]');
        touched.add(row ? idOf(row) : undefined);
      }
    });
    observer.observe(grid(), {
      subtree: true,
      childList: true,
      characterData: true,
      attributes: true,
    });
    app.downloads.apply([{ ...moving, done: moving.done + 1_000_000, speed: moving.speed + 1 }]);
    await settle();
    observer.disconnect();
    expect([...touched]).toEqual([moving.id]);
  });

  it('groups by state under headings, Finished folded until opened', async () => {
    const { app, user } = await setup();
    const headings = within(grid())
      .getAllByRole('button', { expanded: true })
      .concat(within(grid()).getAllByRole('button', { expanded: false }))
      .map((b) => b.textContent.trim().split(' ')[0]);
    expect(headings).toContain('Downloading');
    const finished = within(grid()).getByRole('button', { name: /^Finished/ });
    expect(finished).toHaveAttribute('aria-expanded', 'false');
    const done = app.downloads.order.filter((id) => app.downloads.get(id)?.status === 'completed');
    expect(app.visible.some((id) => done.includes(id))).toBe(false);
    await user.click(finished);
    expect(finished).toHaveAttribute('aria-expanded', 'true');
    expect(app.visible.filter((id) => done.includes(id))).toHaveLength(done.length);
  });

  it('sorts inside each group from a heading: ascending, descending, then back to queue order', async () => {
    const { app, user } = await setup();
    const name = within(grid()).getByRole('columnheader', { name: /Name/ });
    await user.click(within(name).getByRole('button'));
    expect(app.sort).toEqual({ column: 'name', descending: false });
    expect(name).toHaveAttribute('aria-sort', 'ascending');
    const downloading = app.layout.sections.find((x) => x.group === 'active')!;
    const names = app.visible
      .slice(downloading.start, downloading.start + downloading.size)
      .map((id) => app.downloads.get(id)!.file_name);
    expect(names).toEqual(
      names.toSorted((a, b) => a.localeCompare(b, undefined, { numeric: true })),
    );
    await user.click(within(name).getByRole('button'));
    expect(name).toHaveAttribute('aria-sort', 'descending');
    await user.click(within(name).getByRole('button'));
    expect(app.sort.column).toBe('position');
  });

  it("numbers each download by its place in its queue's line", async () => {
    const { app } = await setup();
    const first = app.visible[0]!;
    const row = rowByName(app.downloads.get(first)!.file_name);
    expect(within(row).getAllByRole('gridcell')[0]).toHaveTextContent(
      String(app.places.position.get(first)),
    );
  });

  it('opens the details of a row clicked, and keeps it selected', async () => {
    const { app, user } = await setup();
    const row = rendered()[0]!;
    await user.click(row);
    expect(app.details).toBe(idOf(row));
    expect(app.selection.list).toEqual([idOf(row)]);
    expect(
      screen.getByRole('dialog', { name: app.downloads.get(idOf(row))!.file_name }),
    ).toBeTruthy();
  });

  it('lets go of what a group folds away, so no action reaches a download nobody sees', async () => {
    const { app } = await setup();
    app.folded.clear();
    await settle();
    app.selection.all();
    const done = app.visible.filter((id) => app.downloads.get(id)?.status === 'completed');
    expect(done.length).toBeGreaterThan(0);
    app.toggleGroup('finished');
    await settle();
    expect(app.selection.list.some((id) => done.includes(id))).toBe(false);
    expect(app.selection.size).toBe(app.visible.length);
  });

  it('shows the selection bar for a selection built on purpose, not for one row chosen', async () => {
    const { app, user } = await setup();
    const bar = () => screen.queryByRole('toolbar', { name: 'Selected downloads' });
    const [a, b] = rendered();
    if (!a || !b) throw new Error('too few rows');
    await user.click(a);
    app.closeDetails();
    await settle();
    expect(app.selection.size).toBe(1);
    expect(bar()).toBeNull();
    await user.click(within(b).getByRole('checkbox'));
    await settle();
    expect(bar()).not.toBeNull();
    app.selection.clear();
    await user.click(within(a).getByRole('checkbox'));
    await settle();
    expect(app.selection.size).toBe(1);
    expect(bar()).not.toBeNull();
  });
});
