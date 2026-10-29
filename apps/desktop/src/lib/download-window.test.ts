// A download window closed without an answer keeps its download waiting.

import { screen, within } from '@testing-library/svelte';
import { describe, expect, it, vi } from 'vitest';
import { buildCommands } from './commands';
import { button, downloadWindow, settle, setupWithUser } from './testing/setup';

describe('closing a download window keeps the download waiting', () => {
  it('only Cancel discards; Escape closes the window and Waiting for you reopens it', async () => {
    const { app, backend, user, click } = await setupWithUser();
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
