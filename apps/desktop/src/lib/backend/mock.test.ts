import type { DownloadSummary, UiEvent } from '@wirecatch/protocol';
import { describe, expect, it } from 'vitest';
import { MockBackend } from './mock';

// Noon on Saturday 10 October 2026: the Overnight queue (01:00 to 07:00) is closed.
const NOON = new Date(2026, 9, 10, 12, 0).getTime();

function make(opts: ConstructorParameters<typeof MockBackend>[0] = {}) {
  let now = NOON;
  const mock = new MockBackend({ autoTick: false, now: () => now, ...opts });
  const events: UiEvent[] = [];
  void mock.subscribe((e) => events.push(e));
  return {
    mock,
    events,
    advance(ms: number) {
      now += ms;
      mock.tick(ms);
    },
  };
}

const running = (rows: DownloadSummary[], queue?: number) =>
  rows.filter(
    (r) =>
      (r.status === 'downloading' || r.status === 'connecting') &&
      (queue === undefined || r.queue === queue),
  );

describe('MockBackend', () => {
  it('has realistic sample data with every status and every fix', async () => {
    const { mock } = make();
    const rows = await mock.listDownloads();
    const statuses = new Set(rows.map((r) => r.status));
    for (const s of [
      'queued',
      'downloading',
      'paused',
      'completed',
      'failed',
      'cancelled',
    ] as const) {
      expect(statuses.has(s)).toBe(true);
    }
    const fixes = new Set(rows.map((r) => r.error?.fix).filter(Boolean));
    expect(fixes).toEqual(
      new Set(['retry', 'refresh_link', 'choose_folder', 'free_disk_space', 'keep_original']),
    );
    for (const r of rows) {
      if (r.error) expect(r.error.message).not.toMatch(/\u2014/);
    }
  });

  it('respects queue limits, the global limit and schedules', async () => {
    const { mock, advance } = make();
    for (let i = 0; i < 20; i++) advance(250);
    const rows = await mock.listDownloads();
    const queues = await mock.listQueues();
    for (const q of queues)
      expect(running(rows, q.id).length).toBeLessThanOrEqual(q.max_concurrent);
    expect(running(rows).length).toBeLessThanOrEqual(
      (await mock.getSettings()).max_concurrent_downloads,
    );
    // Overnight is outside its window at noon.
    expect(running(rows, 2)).toHaveLength(0);
  });

  it('moves bytes and finishes downloads, sending only changed rows', async () => {
    const { mock, events, advance } = make();
    advance(1000);
    const batch = events.filter((e) => e.type === 'downloads_changed').at(-1);
    expect(batch?.type).toBe('downloads_changed');
    if (batch?.type === 'downloads_changed') {
      expect(batch.data.length).toBeLessThan((await mock.listDownloads()).length);
    }
    for (let i = 0; i < 400; i++) advance(1000);
    const blender = (await mock.listDownloads()).find((r) => r.file_name.startsWith('blender'));
    expect(blender?.status).toBe('completed');
    expect(blender?.done).toBe(blender?.total);
  });

  it('keeps a queue under its speed limit', async () => {
    const { mock, advance } = make();
    for (let i = 0; i < 12; i++) advance(250);
    const work = running(await mock.listDownloads(), 3);
    const total = work.reduce((s, r) => s + r.speed, 0);
    expect(total).toBeLessThanOrEqual(1024 * 1024 + 1);
  });

  it('pauses, resumes, retries and refreshes links', async () => {
    const { mock } = make();
    const rows = await mock.listDownloads();
    const first = rows[0];
    if (!first) throw new Error('no rows');
    await mock.pause(first.id);
    expect((await mock.listDownloads())[0]?.status).toBe('paused');
    await mock.resume(first.id);
    expect((await mock.listDownloads())[0]?.status).toBe('queued');
    const expired = rows.find((r) => r.error?.fix === 'refresh_link');
    if (!expired) throw new Error('no expired link');
    await expect(mock.refreshLink(expired.id, 'not a link')).rejects.toThrow(
      /does not look like a link/,
    );
    await mock.refreshLink(expired.id, 'https://cdn.example-conf.com/v/new.mp4');
    const after = (await mock.listDownloads()).find((r) => r.id === expired.id);
    expect(after?.status).toBe('queued');
    expect(after?.done).toBe(expired.done);
    expect(after?.error).toBeNull();
  });

  it('adds, moves, reorders and removes', async () => {
    const { mock, events } = make({ empty: true });
    const a = await mock.addDownload({
      url: 'https://x.org/a.zip',
      queue: null,
      folder: null,
      file_name: null,
      start: true,
    });
    const b = await mock.addDownload({
      url: 'https://x.org/b.mp4',
      queue: null,
      folder: 'D:\\Media',
      file_name: 'clip.mp4',
      start: false,
    });
    let rows = await mock.listDownloads();
    expect(rows.map((r) => r.file_name)).toEqual(['a.zip', 'clip.mp4']);
    expect(rows[0]?.path).toBe('C:\\Users\\you\\Downloads\\Archives\\a.zip');
    expect(rows[1]?.path).toBe('D:\\Media\\clip.mp4');
    expect(rows[1]?.status).toBe('paused');
    await mock.reorder(1, [b, a]);
    rows = await mock.listDownloads();
    expect(rows.map((r) => r.id)).toEqual([b, a]);
    await mock.moveToQueue([a], 3);
    expect((await mock.listDownloads()).find((r) => r.id === a)?.queue).toBe(3);
    await mock.remove([a, b], false);
    expect(await mock.listDownloads()).toHaveLength(0);
    expect(events.some((e) => e.type === 'downloads_removed')).toBe(true);
  });

  it('manages queues and refuses to delete the default one', async () => {
    const { mock } = make();
    const q = await mock.createQueue('Weekend');
    await mock.updateQueue({ ...q, max_concurrent: 5 });
    expect((await mock.listQueues()).find((x) => x.id === q.id)?.max_concurrent).toBe(5);
    await expect(mock.deleteQueue(1)).rejects.toThrow(/default queue cannot be deleted/);
    await expect(mock.createQueue('  ')).rejects.toThrow(/name/);
    await mock.deleteQueue(q.id);
    expect((await mock.listQueues()).some((x) => x.id === q.id)).toBe(false);
  });

  it('round-trips settings through export and import', async () => {
    const { mock } = make();
    const s = await mock.getSettings();
    await mock.exportSettings('C:\\p.json');
    await mock.updateSettings({ ...s, theme: 'dark' });
    expect((await mock.getSettings()).theme).toBe('dark');
    await mock.importSettings('C:\\p.json');
    expect((await mock.getSettings()).theme).toBe(s.theme);
    await expect(mock.importSettings('C:\\missing.json')).rejects.toThrow(
      /not a Wirecatch settings file/,
    );
  });

  it('asks first in its own window by default, or captures with a toast', async () => {
    const { mock, events } = make({ empty: true });
    const s = await mock.getSettings();
    expect(s.extension.ask_before_download).toBe(true);
    await mock.updateSettings({ ...s, extension: { ...s.extension, ask_before_download: false } });
    mock.simulateCapture();
    expect(events.some((e) => e.type === 'captured')).toBe(true);
    expect(mock.windows.windows).toHaveLength(0);
    await mock.updateSettings(s);
    mock.simulateCapture();
    const asked = events.find((e) => e.type === 'confirmation_requested');
    expect(asked?.type).toBe('confirmation_requested');
    const pending = await mock.pendingConfirmations();
    expect(pending).toHaveLength(1);
    const id = pending[0]?.id ?? -1;
    expect(mock.windows.windows.map((w) => w.shows)).toEqual([{ kind: 'question', id }]);
    expect(await mock.getQuestion(id)).toEqual({ kind: 'capture', data: pending[0] });
    const confirmed = await mock.confirmCapture(id, {
      folder: 'D:\\Media',
      file_name: 'renamed.pdf',
      queue: null,
      later: false,
    });
    expect(await mock.pendingConfirmations()).toHaveLength(0);
    expect(await mock.getQuestion(id)).toBeNull();
    expect((await mock.getDownload(confirmed)).file_name).toBe('renamed.pdf');
    const added = (await mock.listDownloads()).at(-1);
    expect(added?.path).toBe('D:\\Media\\renamed.pdf');
  });

  it('can generate 10,000 downloads for load testing', async () => {
    const { mock } = make({ count: 10_000, active: 50 });
    const rows = await mock.listDownloads();
    expect(rows).toHaveLength(10_000);
    expect(new Set(rows.map((r) => r.id)).size).toBe(10_000);
  });
});
