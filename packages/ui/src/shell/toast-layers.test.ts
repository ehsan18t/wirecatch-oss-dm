// A toast that moves to the layer below when a dialog closes is the same toast: it does not slide in again.

import { fireEvent, render, screen, within } from '@testing-library/svelte';
import userEvent from '@testing-library/user-event';
import { describe, expect, it, vi } from 'vitest';
import * as motion from '../motion/motion';
import ToastDialogHost from '../testing/ToastDialogHost.svelte';
import { TOAST_DURATION } from './toaster.svelte';

vi.mock('../motion/motion', async (original) => {
  const actual = await original<typeof motion>();
  return { ...actual, shift: vi.fn(actual.shift) };
});

const toastsShifted = () =>
  vi
    .mocked(motion.shift)
    .mock.calls.filter(([node]) => node instanceof HTMLElement && 'toast' in node.dataset);

describe('toasts across modal layers', () => {
  it('keeps a toast still when the dialog it showed in closes', async () => {
    render(ToastDialogHost);
    await userEvent.click(screen.getByRole('button', { name: 'Add a download' }));
    const dialog = screen.getByRole('dialog', { name: 'Add a download' });
    await fireEvent.click(screen.getByRole('button', { name: 'Background failure', hidden: true }));
    expect(within(dialog).getByRole('region', { name: 'Notifications' }).textContent).toContain(
      'ubuntu.iso failed',
    );
    vi.mocked(motion.shift).mockClear();
    await userEvent.keyboard('{Escape}');
    expect(screen.queryByRole('dialog', { name: 'Add a download' })).toBeNull();
    expect(screen.getByRole('region', { name: 'Notifications' }).textContent).toContain(
      'ubuntu.iso failed',
    );
    expect(toastsShifted()).toEqual([]);
  });
});

describe('a toast region that closes while the person points at it', () => {
  it('lets the countdown run on in the layer below', async () => {
    vi.useFakeTimers();
    try {
      render(ToastDialogHost);
      await fireEvent.click(screen.getByRole('button', { name: 'Add a download' }));
      await fireEvent.click(
        screen.getByRole('button', { name: 'Background failure', hidden: true }),
      );
      const dialog = screen.getByRole('dialog', { name: 'Add a download' });
      await fireEvent.pointerEnter(within(dialog).getByRole('region', { name: 'Notifications' }));
      await fireEvent.keyDown(dialog, { key: 'Escape' });
      expect(screen.queryByRole('dialog', { name: 'Add a download' })).toBeNull();
      await vi.advanceTimersByTimeAsync(TOAST_DURATION.danger);
      expect(screen.queryByText('ubuntu.iso failed', { selector: '[data-toast] p' })).toBeNull();
    } finally {
      vi.useRealTimers();
    }
  });
});
