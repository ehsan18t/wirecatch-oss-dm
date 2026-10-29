// Queues pause for real, start outside their hours when asked, pick their days, and are made in one step.

import { queueId } from '@wirecatch/protocol';
import { screen, within } from '@testing-library/svelte';
import { describe, expect, it, vi } from 'vitest';
import { button, mockBackend, settle, setupWithUser } from './testing/setup';

describe('queues pause for real and scheduled queues can start now', () => {
  it('pauses the queue itself, then resumes it', async () => {
    const { app, backend, click } = await setupWithUser();
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
    const { app, backend, click } = await setupWithUser();
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

describe('the days picker says what it does', () => {
  it('shows every day pressed, and saves the days left pressed', async () => {
    const { app, click } = await setupWithUser();
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
    const { app, click } = await setupWithUser();
    app.openDialog({ kind: 'queue', id: queueId(2) });
    await settle();
    expect(screen.getByLabelText('Stop time')).toBeInTheDocument();
    await click(screen.getByRole('radio', { name: 'When the queue is empty' }));
    expect(screen.queryByLabelText('Stop time')).toBeNull();
  });
});

describe('creating a queue is one atomic step', () => {
  it('a refused save makes no queue, and saving again makes exactly one', async () => {
    const { app, backend, user, click } = await setupWithUser();
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
