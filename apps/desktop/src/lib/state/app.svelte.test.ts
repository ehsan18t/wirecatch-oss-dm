import { describe, expect, it, vi } from 'vitest';
import { MockBackend } from '../backend/mock';
import { BackendError } from '../backend/types';
import { buildCommands, fuzzyScore, searchCommands } from '../commands';
import { AppState } from './app.svelte';

const NOON = new Date(2026, 9, 10, 12, 0).getTime();

async function make() {
  const backend = new MockBackend({ autoTick: false, now: () => NOON });
  const app = new AppState(backend);
  await app.init();
  return { app, backend };
}

describe('AppState', () => {
  it('loads everything and applies events', async () => {
    const { app, backend } = await make();
    expect(app.ready).toBe(true);
    expect(app.queues.length).toBe(3);
    expect(app.downloads.size).toBe(app.counts.all);
    backend.simulateCapture();
    expect(app.toasts.at(-1)?.kind).toBe('capture');
    app.dispose();
  });

  it('shows a readable error when the backend is not there', async () => {
    const backend = new MockBackend({ autoTick: false });
    vi.spyOn(backend, 'listDownloads').mockRejectedValue(
      new BackendError('Wirecatch is not connected to its download engine yet.'),
    );
    const app = new AppState(backend);
    await app.init();
    expect(app.loadError).toMatch(/not connected/);
    app.dispose();
  });

  it('selects ranges in display order', async () => {
    const { app } = await make();
    const [a, b, c] = app.visible;
    app.selectOnly(a as number);
    app.selectRange(c as number);
    expect([...app.selection]).toEqual([a, b, c]);
    app.toggle(b as number);
    expect(app.selection.has(b as number)).toBe(false);
    app.moveActive(1, true);
    expect(app.active).toBe(c);
    app.dispose();
  });

  it('places downloads inside their queue and tells the backend the new order', async () => {
    const { app, backend } = await make();
    const spy = vi.spyOn(backend, 'reorder');
    const q = app.downloads.idsInQueue(1);
    await app.placeInQueue(1, [q[3] as number], q[0] as number);
    const expected = [q[3], q[0], q[1], q[2], ...q.slice(4)];
    expect(app.downloads.idsInQueue(1)).toEqual(expected);
    expect(spy).toHaveBeenCalledWith(1, expected);
    // Dropping at the end.
    await app.placeInQueue(1, [q[3] as number], null);
    expect(app.downloads.idsInQueue(1).at(-1)).toBe(q[3]);
    app.dispose();
  });

  it('only nudges inside a queue view', async () => {
    const { app } = await make();
    app.selectOnly(app.visible[1] as number);
    await app.nudge(-1);
    expect(app.toasts.at(-1)?.message).toMatch(/Open a queue/);
    app.dispose();
  });

  it('turns pasted text into the Add dialog, or explains why not', async () => {
    const { app } = await make();
    expect(app.pasteText('hello')).toBe(false);
    expect(app.toasts.at(-1)?.kind).toBe('error');
    expect(app.pasteText('https://a.org/x.zip\nhttps://a.org/y.zip')).toBe(true);
    expect(app.dialog).toEqual({
      kind: 'add',
      urls: ['https://a.org/x.zip', 'https://a.org/y.zip'],
    });
    app.dispose();
  });

  it('saves settings right away and puts them back if saving fails', async () => {
    const { app, backend } = await make();
    await app.updateSettings((d) => void (d.theme = 'dark'));
    expect(app.settings?.theme).toBe('dark');
    expect((await backend.getSettings()).theme).toBe('dark');
    vi.spyOn(backend, 'updateSettings').mockRejectedValueOnce(
      new BackendError('Disk is read only.'),
    );
    await app.updateSettings((d) => void (d.theme = 'light'));
    expect(app.settings?.theme).toBe('dark');
    expect(app.toasts.at(-1)?.message).toBe('Disk is read only.');
    app.dispose();
  });

  it('runs fixes: choose a folder then retry', async () => {
    const { app } = await make();
    const id = app.visible.find(
      (x) => app.downloads.get(x)?.error?.fix === 'choose_folder',
    ) as number;
    await app.fix(id, 'choose_folder');
    expect(app.downloads.get(id)?.status).toBe('queued');
    expect(app.downloads.get(id)?.path).not.toContain('E:\\Backups');
    app.dispose();
  });

  it('confirms and rejects browser captures', async () => {
    const { app, backend } = await make();
    backend.simulateConfirmation();
    backend.simulateConfirmation();
    expect(app.pending).toHaveLength(2);
    await app.rejectCapture(app.pending[0]?.id ?? -1);
    await app.confirmCapture(app.pending[0]?.id ?? -1, { folder: null, file_name: null, queue: 3 });
    expect(app.pending).toHaveLength(0);
    app.dispose();
  });

  it('exports and imports settings', async () => {
    const { app } = await make();
    expect(await app.exportSettings()).toMatch(/wirecatch-settings\.json$/);
    await app.updateSettings((d) => void (d.connections_per_download = 2));
    expect(await app.importSettings()).toMatch(/wirecatch-settings\.json$/);
    expect(app.settings?.connections_per_download).toBe(8);
    app.dispose();
  });
});

describe('commands', () => {
  it('cover the selection only when something is selected', async () => {
    const { app } = await make();
    expect(buildCommands(app).some((c) => c.group === 'Selected downloads')).toBe(false);
    app.selectOnly(app.visible[0] as number);
    expect(buildCommands(app).some((c) => c.id === 'sel.pause')).toBe(true);
    app.dispose();
  });

  it('rank good matches first and are plain language', async () => {
    const { app } = await make();
    const all = buildCommands(app);
    expect(searchCommands(all, 'pause all')[0]?.id).toBe('dl.pauseall');
    expect(searchCommands(all, 'new queue')[0]?.id).toBe('queue.new');
    expect(
      searchCommands(all, 'bandwidth').some((c) => c.id === 'setting.global_speed_limit'),
    ).toBe(true);
    expect(fuzzyScore('xyz', 'Pause all downloads')).toBe(-1);
    for (const c of all) expect(c.title).not.toMatch(/\u2014/);
    app.dispose();
  });
});
