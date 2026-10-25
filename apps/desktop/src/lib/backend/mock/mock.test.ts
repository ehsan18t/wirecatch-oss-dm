import {
  downloadId,
  FOLLOW_APP,
  MANUAL_QUEUE,
  ownSetting,
  ownValue,
  questionId,
  queueId,
  type DownloadSummary,
  type UiEvent,
} from '@wirecatch/protocol';
import { describe, expect, it, vi } from 'vitest';
import { mockBackend, NOON } from '$lib/testing/setup';
import { BackendError } from '../types';
import type { MockBackend } from '.';

// Noon on Saturday 10 October 2026: the Overnight queue (01:00 to 07:00) is closed.

function make(opts: ConstructorParameters<typeof MockBackend>[0] = {}) {
  let now = NOON;
  const mock = mockBackend({ now: () => now, ...opts });
  const events: UiEvent[] = [];
  void mock.subscribe(
    (e) => events.push(e),
    () => undefined,
  );
  return {
    mock,
    events,
    advance: (ms: number) => {
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
    const messages = rows.flatMap((r) => (r.error ? [r.error.message] : []));
    expect(messages.filter((m) => m.includes('\u2014'))).toEqual([]);
  });

  it('respects queue limits, the global limit and schedules', async () => {
    const { mock, advance } = make();
    for (let i = 0; i < 20; i++) advance(250);
    const rows = await mock.listDownloads();
    const queues = await mock.listQueues();
    const limited = queues.flatMap((q) => {
      const own = ownValue(q.settings.max_concurrent);
      return own === null ? [] : [{ id: q.id, own }];
    });
    expect(limited.length).toBeGreaterThan(0);
    for (const { id, own } of limited) expect(running(rows, id).length).toBeLessThanOrEqual(own);
    expect(running(rows).length).toBeLessThanOrEqual(
      (await mock.getSettings()).max_concurrent_downloads,
    );
    // Overnight is outside its window at noon.
    expect(running(rows, 2)).toHaveLength(0);
  });

  it('moves bytes and finishes downloads, sending only changed rows', async () => {
    const { mock, events, advance } = make();
    advance(1000);
    const batch = events.findLast((e) => e.type === 'downloads_changed');
    if (batch?.type !== 'downloads_changed') throw new Error('no batch of changes was sent');
    expect(batch.data.length).toBeLessThan((await mock.listDownloads()).length);
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
    const work3 = (await mock.listQueues()).find((q) => q.id === 3);
    const limit = work3 ? ownValue(work3.settings.speed_limit) : null;
    expect(limit).toBeTypeOf('number');
    // Speeds are rounded to whole bytes, so the total may go one byte over.
    expect(total).toBeLessThanOrEqual((limit ?? 0) + 1);
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
      folder: String.raw`D:\Media`,
      file_name: 'clip.mp4',
      start: false,
    });
    let rows = await mock.listDownloads();
    expect(rows.map((r) => r.file_name)).toEqual(['a.zip', 'clip.mp4']);
    expect(rows[0]?.path).toBe(String.raw`C:\Users\you\Downloads\Archives\a.zip`);
    expect(rows[1]?.path).toBe(String.raw`D:\Media\clip.mp4`);
    expect(rows[1]?.status).toBe('paused');
    await mock.reorder(queueId(1), [b, a]);
    rows = await mock.listDownloads();
    expect(rows.map((r) => r.id)).toEqual([b, a]);
    await mock.moveToQueue([a], queueId(3));
    expect((await mock.listDownloads()).find((r) => r.id === a)?.queue).toBe(3);
    await mock.remove([a, b], false);
    expect(await mock.listDownloads()).toHaveLength(0);
    expect(events.some((e) => e.type === 'downloads_removed')).toBe(true);
  });

  it('manages queues and refuses to delete the default one', async () => {
    const { mock } = make();
    const weekend = {
      name: 'Weekend',
      description: '',
      max_concurrent: ownSetting(2),
      speed_limit: FOLLOW_APP,
      connections: FOLLOW_APP,
      takes: MANUAL_QUEUE,
      schedule: null,
      notify: 'each_download',
    } as const;
    const q = await mock.createQueue(weekend);
    await mock.updateQueue(q.id, { ...weekend, max_concurrent: ownSetting(5) });
    expect((await mock.listQueues()).find((x) => x.id === q.id)?.settings.max_concurrent).toEqual(
      ownSetting(5),
    );
    await expect(mock.deleteQueue(queueId(1))).rejects.toThrow(/default queue cannot be deleted/);
    await expect(mock.createQueue({ ...weekend, name: '  ' })).rejects.toThrow(/name/);
    await mock.deleteQueue(q.id);
    expect((await mock.listQueues()).some((x) => x.id === q.id)).toBe(false);
  });

  it('round-trips settings through export and import', async () => {
    const { mock } = make();
    const s = await mock.getSettings();
    await mock.exportSettings(String.raw`C:\p.json`);
    await mock.changeSettings({ theme: 'dark' });
    expect((await mock.getSettings()).theme).toBe('dark');
    await mock.importSettings(String.raw`C:\p.json`);
    expect((await mock.getSettings()).theme).toBe(s.theme);
    await expect(mock.importSettings(String.raw`C:\missing.json`)).rejects.toThrow(
      /not a Wirecatch settings file/,
    );
  });

  it('asks first in its own window by default, or captures without one', async () => {
    const { mock, events } = make({ empty: true });
    const s = await mock.getSettings();
    expect(s.extension.ask_before_download).toBe(true);
    await mock.changeSettings({ extension: { ask_before_download: false } });
    mock.simulateCapture();
    expect(events.some((e) => e.type === 'captured')).toBe(true);
    expect(mock.windows.windows).toHaveLength(0);
    await mock.changeSettings({ extension: { ask_before_download: true } });
    mock.simulateCapture();
    const asked = events.find((e) => e.type === 'confirmation_requested');
    expect(asked?.type).toBe('confirmation_requested');
    const pending = await mock.pendingConfirmations();
    expect(pending).toHaveLength(1);
    const id = pending[0]?.id ?? questionId(0);
    expect(mock.windows.windows.map((w) => w.shows)).toEqual([{ kind: 'question', id }]);
    expect(await mock.getQuestion(id)).toEqual({ kind: 'capture', data: pending[0] });
    const confirmed = await mock.confirmCapture(id, {
      folder: String.raw`D:\Media`,
      file_name: 'renamed.pdf',
      queue: null,
      later: false,
    });
    expect(await mock.pendingConfirmations()).toHaveLength(0);
    expect(await mock.getQuestion(id)).toBeNull();
    expect((await mock.getDownload(confirmed)).file_name).toBe('renamed.pdf');
    const added = (await mock.listDownloads()).at(-1);
    expect(added?.path).toBe(String.raw`D:\Media\renamed.pdf`);
  });

  it('can generate 10,000 downloads for load testing', async () => {
    const { mock } = make({ count: 10_000, active: 50 });
    const rows = await mock.listDownloads();
    expect(rows).toHaveLength(10_000);
    expect(new Set(rows.map((r) => r.id)).size).toBe(10_000);
  });

  it('answers after the configured latency, once its events have arrived', async () => {
    vi.useFakeTimers();
    const { mock, events } = make({ empty: true, latencyMs: 200 });
    let answered: number | null = null;
    void mock
      .addDownload({
        url: 'https://x.org/a.zip',
        queue: null,
        folder: null,
        file_name: null,
        start: true,
      })
      .then((id) => (answered = id));
    expect(events.some((e) => e.type === 'downloads_changed')).toBe(true);
    await vi.advanceTimersByTimeAsync(199);
    expect(answered).toBeNull();
    await vi.advanceTimersByTimeAsync(1);
    expect(answered).toBe(1);
  });

  it('fails after the configured latency too', async () => {
    vi.useFakeTimers();
    const { mock } = make({ empty: true, latencyMs: 50 });
    let error: unknown = null;
    void mock.pause(downloadId(42)).catch((error_: unknown) => (error = error_));
    await vi.advanceTimersByTimeAsync(49);
    expect(error).toBeNull();
    await vi.advanceTimersByTimeAsync(1);
    expect(error).toBeInstanceOf(BackendError);
  });

  it('answers within one microtask by default', async () => {
    const { mock } = make({ empty: true });
    let answered = false;
    let failed = false;
    void mock.listDownloads().then(() => (answered = true));
    void mock.pause(downloadId(42)).catch(() => (failed = true));
    await Promise.resolve();
    expect(answered).toBe(true);
    expect(failed).toBe(true);
  });

  it('drops a slow reading still on its way when disposed', () => {
    vi.useFakeTimers();
    const { mock } = make({ empty: true });
    mock.simulateSlowReading();
    mock.dispose();
    expect(vi.getTimerCount()).toBe(0);
  });

  it('asks nothing more about a slow reading cancelled before it finished', async () => {
    vi.useFakeTimers();
    const { mock } = make({ empty: true });
    mock.simulateSlowReading();
    const shown = mock.windows.windows[0]?.shows;
    const id = shown?.kind === 'question' ? shown.id : questionId(0);
    await mock.dismissMediaChoice(id);
    await vi.advanceTimersByTimeAsync(10_000);
    expect(await mock.pendingMediaChoices()).toEqual([]);
    await expect(mock.getQuestion(id)).resolves.toBeNull();
  });

  it('drops answers still on their way back when disposed', async () => {
    vi.useFakeTimers();
    const { mock } = make({ empty: true, latencyMs: 100 });
    let answered = false;
    void mock.listDownloads().then(() => (answered = true));
    mock.dispose();
    await vi.advanceTimersByTimeAsync(1000);
    expect(answered).toBe(false);
    expect(vi.getTimerCount()).toBe(0);
  });
});
