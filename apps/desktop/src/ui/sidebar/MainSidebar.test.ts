import { downloadId, queueId } from '@wirecatch/protocol';
import { fireEvent, screen, within } from '@testing-library/svelte';
import userEvent from '@testing-library/user-event';
import { describe, expect, it } from 'vitest';
import { DRAG_TYPE } from '$lib/dnd';
import { renderApp, rowByName, settle } from '$lib/testing/setup';
import { sidebarCollapsed } from '$lib/state/sidebar-preference';

const nav = () => screen.getByRole('navigation', { name: 'Views and queues' });

describe('the sidebar', () => {
  it('shows each view with how many it holds, and marks the one shown', async () => {
    const { app } = await renderApp();
    const user = userEvent.setup();
    const all = within(nav()).getByRole('button', { name: /^All downloads/ });
    expect(all).toHaveAttribute('aria-current', 'page');
    expect(all).toHaveTextContent(String(app.counts.all));
    const failed = within(nav()).getByRole('button', { name: /^Needs attention/ });
    expect(failed).toHaveTextContent(String(app.counts.status.failed));
    // Failures call for attention: their count is in the danger colour.
    expect(within(failed).getByText(String(app.counts.status.failed))).toHaveClass('text-danger');
    await user.click(within(nav()).getByRole('button', { name: /^Paused/ }));
    expect(app.view).toEqual({ kind: 'status', group: 'paused' });
    expect(within(nav()).getByRole('button', { name: /^Paused/ })).toHaveAttribute(
      'aria-current',
      'page',
    );
    expect(all).not.toHaveAttribute('aria-current');
  });

  it('gives each queue what it is doing as a second line, and one menu of its own actions', async () => {
    const { app } = await renderApp();
    const user = userEvent.setup();
    const overnight = within(nav()).getByRole('button', { name: /^Overnight/ });
    expect(overnight).toHaveTextContent(app.queues.describe(app.queues.byId(queueId(2))!));
    await user.click(within(nav()).getByRole('button', { name: 'Actions for Overnight' }));
    await user.click(screen.getByRole('menuitem', { name: 'Pause the queue' }));
    await settle();
    expect(app.queues.byId(queueId(2))?.status.paused).toBe(true);
    expect(within(nav()).getByRole('button', { name: /^Overnight/ })).toHaveTextContent('Paused');
    await user.click(within(nav()).getByRole('button', { name: 'Actions for Overnight' }));
    await user.click(screen.getByRole('menuitem', { name: 'Edit name, hours and limits' }));
    expect(app.dialog).toEqual({ kind: 'queue', id: queueId(2) });
  });

  it('moves a queue up and down below the default queue, which stays first', async () => {
    const { app } = await renderApp();
    const user = userEvent.setup();
    const order = () => app.queues.list.map((q) => q.settings.name);
    expect(order()).toEqual(['Main', 'Overnight', 'Work files']);
    const actions = async (name: string) => {
      await user.click(within(nav()).getByRole('button', { name: `Actions for ${name}` }));
      return screen.getByRole('menu');
    };

    const main = await actions('Main');
    expect(within(main).queryByRole('menuitem', { name: /^Move/ })).toBeNull();
    await user.keyboard('{Escape}');
    const overnight = await actions('Overnight');
    expect(within(overnight).queryByRole('menuitem', { name: 'Move up' })).toBeNull();
    await user.click(within(overnight).getByRole('menuitem', { name: 'Move down' }));
    await settle();
    expect(order()).toEqual(['Main', 'Work files', 'Overnight']);
    expect(within(nav()).getByText('Overnight is now 3 of 3 queues.')).toHaveClass('sr-only');

    const work = await actions('Work files');
    await user.click(within(work).getByRole('menuitem', { name: 'Move down' }));
    await settle();
    expect(order()).toEqual(['Main', 'Overnight', 'Work files']);
  });

  it('moves downloads dropped on a queue to it', async () => {
    const { app } = await renderApp();
    const libre = rowByName('LibreOffice');
    const id = Number(libre.dataset.id);
    const data = new Map<string, string>([[DRAG_TYPE, JSON.stringify([id])]]);
    const dataTransfer = {
      types: [DRAG_TYPE],
      getData: (type: string) => data.get(type) ?? '',
      dropEffect: 'none',
    };
    const target = within(nav()).getByRole('button', { name: /^Work files/ });
    await fireEvent.dragOver(target, { dataTransfer });
    await fireEvent.drop(target, { dataTransfer });
    await settle();
    expect(app.downloads.get(downloadId(id))?.queue).toBe(3);
  });

  it('moves a queue dragged onto another to its place, but never above the default queue', async () => {
    const { app } = await renderApp();
    const order = () => app.queues.list.map((q) => q.settings.name);
    const data = new Map<string, string>();
    const dataTransfer = {
      get types() {
        return [...data.keys()];
      },
      setData: (type: string, value: string) => data.set(type, value),
      getData: (type: string) => data.get(type) ?? '',
      effectAllowed: 'none',
      dropEffect: 'none',
    };
    const item = (name: string) => {
      const button = within(nav()).getByRole('button', { name: new RegExp(`^${name}`) });
      return button.closest('[draggable]') ?? button;
    };
    expect(item('Main')).toHaveAttribute('draggable', 'false');
    const drag = async (from: string, onto: string) => {
      data.clear();
      await fireEvent.dragStart(item(from), { dataTransfer });
      await fireEvent.dragOver(item(onto), { dataTransfer });
      await fireEvent.drop(item(onto), { dataTransfer });
      await fireEvent.dragEnd(item(from), { dataTransfer });
      await settle();
    };

    // While dragged, the queue fades; the line stays while the pointer crosses into the row's own button.
    data.clear();
    await fireEvent.dragStart(item('Work files'), { dataTransfer });
    expect(item('Work files')).toHaveClass('opacity-40');
    await fireEvent.dragOver(item('Overnight'), { dataTransfer });
    const inside = within(nav()).getByRole('button', { name: 'Actions for Overnight' });
    // jsdom has no DragEvent, and its stand-in drops `relatedTarget`.
    await fireEvent(
      item('Overnight'),
      new MouseEvent('dragleave', { bubbles: true, relatedTarget: inside }),
    );
    expect(item('Overnight')).toHaveClass('after:-top-px');
    await fireEvent.dragLeave(item('Overnight'), { dataTransfer, relatedTarget: null });
    expect(item('Overnight')).not.toHaveClass('after:-top-px');
    await fireEvent.dragEnd(item('Work files'), { dataTransfer });
    expect(item('Work files')).not.toHaveClass('opacity-40');

    await drag('Work files', 'Main');
    expect(order()).toEqual(['Main', 'Work files', 'Overnight']);
    await drag('Work files', 'Overnight');
    expect(order()).toEqual(['Main', 'Overnight', 'Work files']);
  });

  it('collapses to an icon rail from the title bar, and keeps that choice', async () => {
    await renderApp();
    const user = userEvent.setup();
    await user.click(screen.getByRole('button', { name: 'Collapse the navigation' }));
    expect(sidebarCollapsed()).toBe(true);
    // In the rail the labels are names, with the count where there is one.
    expect(within(nav()).getByRole('button', { name: /^Needs attention, \d+$/ })).toBeTruthy();
    expect(within(nav()).queryByRole('button', { name: 'New queue' })).toBeNull();
    await user.click(screen.getByRole('button', { name: 'Expand the navigation' }));
    expect(sidebarCollapsed()).toBe(false);
  });

  it('is one Tab stop, the view shown, with the arrows moving through it', async () => {
    await renderApp();
    const user = userEvent.setup();
    const stops = within(nav())
      .getAllByRole('button')
      .filter((b) => b.tabIndex === 0);
    expect(stops.map((b) => b.textContent.trim())).toEqual([
      expect.stringMatching(/^All downloads/),
    ]);
    const all = within(nav()).getByRole('button', { name: /^All downloads/ });
    all.focus();
    await user.keyboard('{ArrowDown}');
    expect(document.activeElement).toBe(
      within(nav()).getByRole('button', { name: /^Downloading/ }),
    );
    await user.keyboard('{End}');
    expect(document.activeElement).toBe(
      within(nav()).getByRole('button', { name: 'Keyboard shortcuts' }),
    );
    await user.keyboard('{Home}');
    expect(document.activeElement).toBe(all);
  });

  it("reaches a queue's own buttons with the right arrow, and back with the left", async () => {
    await renderApp();
    const user = userEvent.setup();
    const overnight = within(nav()).getByRole('button', { name: /^Overnight/ });
    overnight.focus();
    await user.keyboard('{ArrowRight}');
    const first = document.activeElement;
    expect(first?.getAttribute('aria-label')).toMatch(/Overnight/);
    expect(first).not.toBe(overnight);
    expect(first).toHaveAttribute('tabindex', '-1');
    await user.keyboard('{ArrowLeft}');
    expect(document.activeElement).toBe(overnight);
  });

  it('shows three queues, folds the rest away, and always the one on screen', async () => {
    const { app } = await renderApp();
    const user = userEvent.setup();
    const [first] = app.queues.list;
    app.queues.list = [
      ...app.queues.list,
      ...[10, 11].map((n) => ({
        ...first!,
        id: queueId(n),
        settings: { ...first!.settings, name: `Extra ${String(n)}` },
        is_default: false,
      })),
    ];
    await settle();
    expect(within(nav()).queryByRole('button', { name: /^Extra 11/ })).toBeNull();
    const more = within(nav()).getByRole('button', { name: '2 more queues' });
    await user.click(more);
    expect(more).toHaveAttribute('aria-expanded', 'true');
    expect(within(nav()).getByRole('button', { name: /^Extra 11/ })).toBeTruthy();
    await user.click(more);
    app.show({ kind: 'queue', id: queueId(11) });
    await settle();
    expect(within(nav()).getByRole('button', { name: /^Extra 11/ })).toHaveAttribute(
      'aria-current',
      'page',
    );
    // Folded, only Extra 10 is out of sight.
    expect(within(nav()).getByRole('button', { name: '1 more queue' })).toBeTruthy();
    app.queues.list = app.queues.list.filter((q) => q.id !== queueId(10));
    await settle();
    // The one queue past three is on screen: nothing is folded away, so nothing offers more.
    expect(within(nav()).queryByRole('button', { name: /more queue/ })).toBeNull();
  });

  it('becomes the list of settings sections, with the way back on top', async () => {
    const { app } = await renderApp();
    const user = userEvent.setup();
    app.openSettings();
    await settle();
    const sections = screen.getByRole('navigation', { name: 'Settings sections' });
    expect(within(sections).getByRole('button', { name: /^General/ })).toHaveAttribute(
      'aria-current',
      'page',
    );
    await user.click(within(sections).getByRole('button', { name: /^Speed/ }));
    expect(app.settingsSection).toBe('speed');
    await user.click(within(sections).getByRole('button', { name: 'Back to downloads' }));
    expect(app.screen).toBe('downloads');
  });
});
