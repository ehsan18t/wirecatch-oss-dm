// The download complete window (D52): one download gets Open as its main button, several are a
// list with a queue's summary under its own heading, it follows what happens to the downloads it
// lists, and "Use notifications instead" switches the setting and goes.

import { fireEvent, render, screen, within } from '@testing-library/svelte';
import { describe, expect, it, vi } from 'vitest';
import { MockFinished, type MockBackend } from '$lib/backend/mock';
import { mockBackend, settle } from '$lib/testing/setup';
import { FinishedWindowState } from '$lib/window/finished-window.svelte';
import FinishedWindow from './FinishedWindow.svelte';

/** The sample's complete window list, with its palette actions. */
function sample(backend: MockBackend): MockFinished {
  const finished = backend.sampleData.finished;
  if (!(finished instanceof MockFinished)) throw new Error('The sample keeps its own list.');
  return finished;
}

async function shown(backend: MockBackend) {
  const sample = backend.sampleData;
  const host = { close: vi.fn() };
  const win = new FinishedWindowState({
    ...host,
    backend: () => backend,
    finished: () => sample.finished,
  });
  await win.init();
  render(FinishedWindow, { props: { win } });
  await settle();
  return { win, host };
}

describe('the download complete window', () => {
  it('gives one finished download the room, with Open as its main button', async () => {
    const b = mockBackend();
    // The sample file may be a program, which asks first: the person agrees.
    b.answerProgramWarning = () => true;
    sample(b).sampleOne();
    const { win } = await shown(b);
    const only = win.rows[0]!;
    expect(screen.getByRole('heading', { name: 'Download finished' })).toBeVisible();
    expect(screen.getByText(only.file_name)).toBeVisible();
    await fireEvent.click(screen.getByRole('button', { name: 'Open' }));
    await settle();
    expect(b.opened).toEqual([only.id]);
    expect(screen.getByRole('button', { name: 'Show in folder' })).toBeVisible();
  });

  it('lists a queue that is done under its own heading, and tries a failed download again', async () => {
    const b = mockBackend();
    sample(b).sampleQueueRun();
    const { win } = await shown(b);
    const { completed, failed } = win.counts;
    expect(failed).toBe(1);
    expect(
      screen.getByRole('heading', { name: `${String(completed)} downloads finished and 1 failed` }),
    ).toBeVisible();
    const queue = win.groups[0]!.queue!;
    const group = within(screen.getByRole('region', { name: `${queue.name} is done` }));
    expect(group.getAllByRole('listitem')).toHaveLength(completed + failed);

    const broken = win.rows.find((r) => r.status === 'failed')!;
    const retry = vi.spyOn(b, 'retry');
    await fireEvent.click(group.getByRole('button', { name: `Try ${broken.file_name} again` }));
    await settle();
    expect(retry).toHaveBeenCalledWith(broken.id);
    // It follows the download from then on, as it runs again.
    expect(win.rows.find((r) => r.id === broken.id)?.status).not.toBe('failed');
  });

  it('offers Try again only when trying again can fix the failure', async () => {
    const b = mockBackend();
    const expired = (await b.listDownloads()).find((r) => r.error?.fix === 'refresh_link');
    if (!expired) throw new Error('The sample has no expired link.');
    sample(b).add(null, [expired]);
    await shown(b);
    expect(screen.getByRole('heading', { name: 'Download failed' })).toBeVisible();
    expect(screen.queryByRole('button', { name: 'Try again' })).toBeNull();
    expect(screen.getByText('Fix it from the download list in Wirecatch.')).toBeVisible();
  });

  it('lets go of a download removed from the list', async () => {
    const b = mockBackend();
    sample(b).sampleQueueRun();
    const { win } = await shown(b);
    const gone = win.rows[0]!;
    await b.remove([gone.id], false);
    await settle();
    expect(win.rows.some((r) => r.id === gone.id)).toBe(false);
    expect(screen.queryByText(gone.file_name)).toBeNull();
  });

  it('switches to notifications in one click, and goes', async () => {
    const b = mockBackend();
    await b.changeSettings({ finish_notice: 'window' });
    sample(b).sampleOne();
    const { host } = await shown(b);
    await fireEvent.click(screen.getByRole('button', { name: 'Use notifications instead' }));
    await settle();
    expect((await b.getSettings()).finish_notice).toBe('notification');
    expect(host.close).toHaveBeenCalledOnce();
  });

  it('opens by itself in the sample page when a download finishes and the window is chosen', async () => {
    const b = mockBackend({ empty: true });
    await b.changeSettings({ finish_notice: 'window' });
    const id = await b.addDownload({
      url: 'https://example.com/small.zip',
      queue: null,
      folder: null,
      file_name: null,
      start: true,
    });
    for (let i = 0; i < 400 && (await b.getDownload(id)).status !== 'completed'; i++) b.tick(1000);
    expect(b.windows.windows.map((w) => w.shows.kind)).toEqual(['finished']);
    expect((await b.sampleData.finished.finishedList()).groups[0]?.downloads[0]?.id).toBe(id);
  });
});
