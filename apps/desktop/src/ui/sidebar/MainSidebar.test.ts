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

  it('gives each queue what it is doing as a second line, and its own buttons', async () => {
    const { app } = await renderApp();
    const user = userEvent.setup();
    const overnight = within(nav()).getByRole('button', { name: /^Overnight/ });
    expect(overnight).toHaveTextContent(app.queues.status(app.queues.byId(queueId(2))!));
    await user.click(within(nav()).getByRole('button', { name: 'Pause the Overnight queue' }));
    await settle();
    expect(app.queues.byId(queueId(2))?.paused).toBe(true);
    expect(within(nav()).getByRole('button', { name: /^Overnight/ })).toHaveTextContent('Paused');
    await user.click(
      within(nav()).getByRole('button', { name: 'Edit Overnight: name, schedule and limits' }),
    );
    expect(app.dialog).toEqual({ kind: 'queue', id: queueId(2) });
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
        name: `Extra ${String(n)}`,
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
