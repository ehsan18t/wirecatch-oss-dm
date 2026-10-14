import { fireEvent, render, screen, within } from '@testing-library/svelte';
import userEvent from '@testing-library/user-event';
import { flushSync } from 'svelte';
import { afterEach, describe, expect, it, vi } from 'vitest';
import SidebarHost from '../testing/SidebarHost.svelte';
import ToastDialogHost from '../testing/ToastDialogHost.svelte';
import { SidebarState } from './sidebar-state.svelte';
import TitleBar from './TitleBar.svelte';
import ToastRegion from './ToastRegion.svelte';
import { MAX_TOASTS, TOAST_DURATION, Toaster } from './toaster.svelte';
import type { ButtonRect, MaximizeState, WindowControls } from './window-controls';

describe('Toaster', () => {
  afterEach(() => {
    vi.useRealTimers();
  });

  it('announces a change politely without a toast, each time anew', () => {
    const toaster = new Toaster();
    toaster.announce('Paused 2 downloads');
    const first = toaster.announcement;
    expect(first).toMatchObject({ text: 'Paused 2 downloads', assertive: false });
    expect(toaster.list).toEqual([]);
    toaster.announce('Paused 2 downloads');
    expect(toaster.announcement?.id).not.toBe(first?.id);
  });

  it('dismisses each toast after its time, and announces failures assertively', () => {
    vi.useFakeTimers();
    const toaster = new Toaster();
    toaster.success('Saved');
    toaster.danger('The disk is full', { title: 'ubuntu.iso' });
    expect(toaster.announcement).toMatchObject({
      text: 'ubuntu.iso: The disk is full',
      assertive: true,
    });
    vi.advanceTimersByTime(TOAST_DURATION.success);
    expect(toaster.list.map((toast) => toast.message)).toEqual(['The disk is full']);
    vi.advanceTimersByTime(TOAST_DURATION.danger);
    expect(toaster.list).toEqual([]);
  });

  it('holds every countdown while paused, with the time each had left', () => {
    vi.useFakeTimers();
    const toaster = new Toaster();
    toaster.info('Copied');
    vi.advanceTimersByTime(TOAST_DURATION.info - 1);
    toaster.setPaused(true);
    vi.advanceTimersByTime(TOAST_DURATION.action);
    expect(toaster.list).toHaveLength(1);
    toaster.setPaused(false);
    vi.advanceTimersByTime(1);
    expect(toaster.list).toEqual([]);
  });

  it('keeps at most a few, pushing out the oldest that does not hold focus', () => {
    const toaster = new Toaster();
    const first = toaster.info('one');
    toaster.hold(first);
    for (let i = 0; i < MAX_TOASTS; i++) toaster.info(`more ${String(i)}`);
    expect(toaster.list).toHaveLength(MAX_TOASTS);
    expect(toaster.list[0]?.id).toBe(first);
    toaster.dispose();
  });

  it('gives a toast with an action longer', () => {
    const toaster = new Toaster();
    toaster.info('Download added', { action: { label: 'Show', run: vi.fn() } });
    expect(toaster.list[0]?.duration).toBe(TOAST_DURATION.action);
    toaster.dispose();
  });
});

describe('ToastRegion', () => {
  it('shows toasts in a named region and speaks through live regions that are always there', async () => {
    const toaster = new Toaster();
    render(ToastRegion, { toaster });
    expect(screen.getByRole('alert')).toBeTruthy();
    expect(screen.getByRole('status')).toBeTruthy();
    toaster.warning('The link expired', { title: 'video.mp4' });
    flushSync();
    expect(screen.getByRole('region', { name: 'Notifications' }).textContent).toContain(
      'The link expired',
    );
    expect(screen.getByRole('status').textContent).toContain('video.mp4: The link expired');
    expect(screen.getByRole('alert').textContent).toBe('');
    await userEvent.click(screen.getByRole('button', { name: 'Dismiss' }));
    expect(toaster.list).toEqual([]);
    toaster.dispose();
  });

  it('runs a toast’s action and dismisses it', async () => {
    const toaster = new Toaster();
    const run = vi.fn();
    render(ToastRegion, { toaster });
    toaster.info('Download added', { action: { label: 'Show', run } });
    flushSync();
    await userEvent.click(screen.getByRole('button', { name: 'Show' }));
    expect(run).toHaveBeenCalledOnce();
    expect(toaster.list).toEqual([]);
    toaster.dispose();
  });
});

describe('toasts while a modal layer is open', () => {
  it('are heard inside the layer, without repeating what was said before it opened', async () => {
    render(ToastDialogHost);
    await fireEvent.click(screen.getByRole('button', { name: 'Earlier news' }));
    await userEvent.click(screen.getByRole('button', { name: 'Add a download' }));
    const dialog = screen.getByRole('dialog', { name: 'Add a download' });
    const regions = () =>
      [...dialog.querySelectorAll('[aria-live]')].map((r) => r.textContent.trim());
    expect(regions()).toEqual(['', '']);
    await fireEvent.click(screen.getByRole('button', { name: 'Background failure', hidden: true }));
    expect(within(dialog).getByRole('alert').textContent).toContain('ubuntu.iso failed');
  });
});

describe('Sidebar', () => {
  it('marks the current page, and shows labels only as names while it is a rail', async () => {
    const sidebar = new SidebarState({ collapsed: true });
    render(SidebarHost, { sidebar });
    expect(screen.getByRole('navigation', { name: 'Views' })).toBeTruthy();
    // In the rail the count is not shown in words, so it is part of the name.
    const active = screen.getByRole('button', { name: 'Active, 3' });
    expect(active.getAttribute('aria-current')).toBe('page');
    expect(sidebar.mode).toBe('rail');
    await userEvent.click(screen.getByRole('button', { name: 'Paused' }));
    expect(screen.getByRole('button', { name: 'Paused' }).getAttribute('aria-current')).toBe(
      'page',
    );
    expect(active.hasAttribute('aria-current')).toBe(false);
  });

  it('opens as a modal drawer in a narrow window: focus in, Escape out, focus back', async () => {
    vi.stubGlobal('matchMedia', (query: string) => ({
      matches: false,
      media: query,
      addEventListener: vi.fn(),
      removeEventListener: vi.fn(),
    }));
    const sidebar = new SidebarState({ breakpoint: '63rem' });
    render(SidebarHost, { sidebar });
    expect(sidebar.wide).toBe(false);
    const toggle = screen.getByRole('button', { name: 'Navigation' });
    await userEvent.click(toggle);
    expect(sidebar.drawerOpen).toBe(true);
    const drawer = screen.getByRole('dialog', { name: 'Views' });
    expect(drawer.getAttribute('aria-modal')).toBe('true');
    expect(drawer.contains(document.activeElement)).toBe(true);
    expect(document.activeElement).toBe(screen.getByRole('button', { name: 'Active 3' }));
    // The page behind is out of reach, as behind a native modal dialog.
    expect(toggle.closest('[inert]')).not.toBeNull();
    await userEvent.keyboard('{Escape}');
    expect(sidebar.drawerOpen).toBe(false);
    expect(screen.queryByRole('dialog')).toBeNull();
    expect(toggle.closest('[inert]')).toBeNull();
    expect(document.activeElement).toBe(toggle);
  });

  it('gives an item a second line and buttons of its own while labels show, and neither in the rail', async () => {
    const onedit = vi.fn();
    const sidebar = new SidebarState();
    render(SidebarHost, { sidebar, onedit });
    // The second line is part of the item's name; the buttons beside it keep their own.
    const item = screen.getByRole('button', { name: 'Overnight Starts at 22:00' });
    const edit = screen.getByRole('button', { name: 'Edit Overnight' });
    expect(item.contains(edit)).toBe(false);
    await userEvent.click(edit);
    expect(onedit).toHaveBeenCalledOnce();
    sidebar.toggle();
    flushSync();
    expect(screen.getByRole('button', { name: 'Overnight' })).toBeTruthy();
    expect(screen.queryByRole('button', { name: 'Edit Overnight' })).toBeNull();
  });

  it("is one Tab stop, the current page, with the arrows through the rest and the right arrow to an item's buttons", async () => {
    const sidebar = new SidebarState();
    render(SidebarHost, { sidebar });
    const active = screen.getByRole('button', { name: 'Active 3' });
    const tabbable = screen
      .getByRole('navigation', { name: 'Views' })
      .querySelectorAll('[tabindex="0"]');
    expect([...tabbable]).toEqual([active]);
    active.focus();
    await userEvent.keyboard('{ArrowDown}');
    expect(document.activeElement).toBe(screen.getByRole('button', { name: 'Paused' }));
    await userEvent.keyboard('{ArrowDown}');
    expect(document.activeElement).toBe(screen.getByRole('button', { name: 'Queues' }));
    await userEvent.keyboard('{End}');
    const overnight = screen.getByRole('button', { name: 'Overnight Starts at 22:00' });
    expect(document.activeElement).toBe(overnight);
    await userEvent.keyboard('{ArrowRight}');
    expect(document.activeElement).toBe(screen.getByRole('button', { name: 'Edit Overnight' }));
    await userEvent.keyboard('{ArrowLeft}');
    expect(document.activeElement).toBe(overnight);
    // Choosing another page moves the Tab stop with it.
    await userEvent.click(screen.getByRole('button', { name: 'Paused' }));
    flushSync();
    expect(screen.getByRole('button', { name: 'Paused' }).tabIndex).toBe(0);
    expect(active.tabIndex).toBe(-1);
  });

  it('folds a collapsible section away from its heading', async () => {
    const sidebar = new SidebarState();
    render(SidebarHost, { sidebar });
    const heading = screen.getByRole('button', { name: 'Queues' });
    expect(heading.getAttribute('aria-expanded')).toBe('true');
    await userEvent.click(heading);
    expect(heading.getAttribute('aria-expanded')).toBe('false');
    expect(screen.queryByRole('button', { name: /^Overnight/ })).toBeNull();
    expect(screen.getByRole('group', { name: 'Queues' })).toBeTruthy();
  });

  it('collapses to the rail and back in a wide window', () => {
    const sidebar = new SidebarState();
    expect(sidebar.mode).toBe('expanded');
    sidebar.toggle();
    expect(sidebar.mode).toBe('rail');
    expect(sidebar.showLabels).toBe(false);
  });
});

describe('TitleBar', () => {
  function controls(maximized = false) {
    const calls = {
      minimize: vi.fn(() => Promise.resolve()),
      toggleMaximize: vi.fn(() => Promise.resolve()),
      close: vi.fn(() => Promise.resolve()),
    };
    const window: WindowControls = {
      ...calls,
      isMaximized: vi.fn(() => Promise.resolve(maximized)),
      onResized: vi.fn(() => Promise.resolve(vi.fn())),
    };
    return { window, calls };
  }

  it('drags the window and calls the window for each caption button', async () => {
    const { window, calls } = controls();
    render(TitleBar, { title: 'Wirecatch', controls: window });
    expect(screen.getByRole('banner').hasAttribute('data-tauri-drag-region')).toBe(true);
    await userEvent.click(screen.getByRole('button', { name: 'Minimize' }));
    await userEvent.click(screen.getByRole('button', { name: 'Maximize' }));
    await userEvent.click(screen.getByRole('button', { name: 'Close' }));
    expect(calls.minimize).toHaveBeenCalledOnce();
    expect(calls.toggleMaximize).toHaveBeenCalledOnce();
    expect(calls.close).toHaveBeenCalledOnce();
  });

  it('offers Restore while maximized, and reports failed window calls', async () => {
    const { window } = controls(true);
    const onerror = vi.fn();
    window.close = vi.fn(() => Promise.reject(new Error('busy')));
    render(TitleBar, { title: 'Wirecatch', controls: window, onerror });
    expect(await screen.findByRole('button', { name: 'Restore' })).toBeTruthy();
    await userEvent.click(screen.getByRole('button', { name: 'Close' }));
    await vi.waitFor(() => {
      expect(onerror).toHaveBeenCalledWith('close', expect.any(Error));
    });
  });

  it('tells the window where its maximize button is, and draws the look the window reports', async () => {
    const { window: frame } = controls();
    const setMaximizeButton = vi.fn<(rect: ButtonRect) => void>();
    let report: ((state: MaximizeState) => void) | undefined;
    frame.setMaximizeButton = setMaximizeButton;
    frame.onMaximizeState = (listener) => {
      report = listener;
      return Promise.resolve(vi.fn());
    };
    render(TitleBar, { title: 'Wirecatch', controls: frame });
    const maximize = screen.getByRole('button', { name: 'Maximize' });
    expect(setMaximizeButton).toHaveBeenCalledOnce();
    expect(Object.keys(setMaximizeButton.mock.calls[0]?.[0] ?? {})).toEqual([
      'x',
      'y',
      'width',
      'height',
    ]);
    await vi.waitFor(() => {
      expect(report).toBeDefined();
    });
    report?.('hover');
    flushSync();
    expect(maximize.classList.contains('bg-wash')).toBe(true);
    report?.('pressed');
    flushSync();
    expect(maximize.classList.contains('bg-wash-strong')).toBe(true);
  });

  it('has no caption buttons without window controls, and quiets while the window is behind', async () => {
    render(TitleBar, { title: 'Wirecatch' });
    expect(screen.queryByRole('button', { name: 'Close' })).toBeNull();
    await fireEvent.blur(window);
    expect(screen.getByRole('banner').hasAttribute('data-inactive')).toBe(true);
  });
});
