/** The hub's pure parts, tested on their own: timing defaults, menu planning, duplicate matching and its sentences. */
import { afterEach, describe, expect, it, vi } from 'vitest';
import type { DownloadState, HandoverResult } from '../src/lib/capture/handover';
import { DuplicateWatch } from '../src/lib/hub/duplicates';
import { AppCalls } from '../src/lib/hub/app';
import { hubTiming } from '../src/lib/hub/hub';
import { planMenuClick } from '../src/lib/hub/menu';
import { MAX_NOTICES, NoticeBoard } from '../src/lib/hub/notice-board';
import { keptInBrowser, mediaAnswer } from '../src/lib/hub/notices';
import type { Notice } from '../src/lib/messages';
import { KnownSettings } from '../src/lib/hub/settings';
import { NativeClient, UNREADABLE_ANSWER } from '../src/lib/native/client';
import { MemoryCache, FakeApp } from './fakes';
import { REQUEST_TIMEOUT_MS, START_WAIT_MS, WITHDRAW_TIMEOUT_MS } from '../src/lib/native/timing';

type Kept = Extract<HandoverResult, { kind: 'kept' }>;

function kept(overrides: Partial<Kept>): Kept {
  return {
    kind: 'kept',
    handoverId: 'h1',
    reason: 'app_unreachable',
    detail: null,
    browserCopy: 'resumed',
    uncertain: false,
    ...overrides,
  };
}

describe('hubTiming', () => {
  it('fills in every wait that is not given, also one given as undefined', () => {
    const defaults = hubTiming();
    expect(defaults).toEqual({
      connectWaitMs: START_WAIT_MS,
      requestTimeoutMs: REQUEST_TIMEOUT_MS,
      withdrawTimeoutMs: WITHDRAW_TIMEOUT_MS,
    });
    expect(hubTiming({ connectWaitMs: undefined, requestTimeoutMs: 50 })).toEqual({
      ...defaults,
      requestTimeoutMs: 50,
    });
  });
});

describe('the waits', () => {
  afterEach(() => {
    vi.useRealTimers();
  });

  it('let a capture start a closed app that answers hello and the capture as late as each may', async () => {
    vi.useFakeTimers();
    const app = new FakeApp();
    app.running = false;
    app.startDelayMs = START_WAIT_MS - 1;
    app.addLateMs = REQUEST_TIMEOUT_MS - 1;
    const client = new NativeClient({
      transport: app,
      hello: () => Promise.resolve({ client: 'extension', client_version: '0.1.0', browser: null }),
    });
    const settings = new KnownSettings({ cache: new MemoryCache() }, Date.now, () => undefined);
    const calls = new AppCalls(client, hubTiming(), settings, null);

    const added = calls.addDownload({
      handover_id: 'h1',
      url: 'https://files.example.com/big.zip',
      referrer: null,
      page_url: null,
      suggested_name: null,
      mime: null,
      size_hint: null,
      cookies: null,
      user_agent: null,
      extra_headers: [],
      source: 'browser_download',
      queue: null,
    });
    added.catch(() => undefined);
    await vi.advanceTimersByTimeAsync(START_WAIT_MS + REQUEST_TIMEOUT_MS);

    expect(app.starts).toBe(1);
    await expect(added).resolves.toMatchObject({ action: 'started' });
    client.stop();
  });
});

describe('planMenuClick', () => {
  const web = 'https://example.com/a.zip';

  it('fails without a link or source, before anything else', () => {
    expect(planMenuClick({}, { incognito: true, cookieStoreId: null })).toEqual({ kind: 'failed' });
    expect(planMenuClick({ linkUrl: '' }, undefined)).toEqual({ kind: 'failed' });
  });

  it('keeps private windows out, before looking at the link', () => {
    expect(
      planMenuClick(
        { linkUrl: 'blob:https://example.com/1' },
        { incognito: true, cookieStoreId: null },
      ),
    ).toEqual({
      kind: 'private_window',
    });
  });

  it('lets the browser download links that are not on the web', () => {
    const plan = planMenuClick({ linkUrl: 'ftp://example.com/setup.exe' }, undefined);
    expect(plan).toMatchObject({ kind: 'browser', url: 'ftp://example.com/setup.exe' });
    expect(plan.kind === 'browser' && plan.message).toContain('only take web links');
  });

  it('lets the browser download and check programs', () => {
    const plan = planMenuClick({ linkUrl: 'https://example.com/setup.exe' }, undefined);
    expect(plan).toMatchObject({ kind: 'browser' });
    expect(plan.kind === 'browser' && plan.message).toContain('setup.exe is a program');
  });

  it('offers a web link to Wirecatch, by its file name, the link before the source', () => {
    expect(planMenuClick({ linkUrl: web, srcUrl: 'https://example.com/b.mp4' }, undefined)).toEqual(
      { kind: 'wirecatch', url: web, name: 'a.zip' },
    );
    expect(
      planMenuClick({ srcUrl: 'https://example.com/' }, { incognito: false, cookieStoreId: null }),
    ).toEqual({
      kind: 'wirecatch',
      url: 'https://example.com/',
      name: 'this item',
    });
  });
});

describe('DuplicateWatch', () => {
  function watch(): { watch: DuplicateWatch; warned: string[] } {
    const warned: string[] = [];
    return { watch: new DuplicateWatch(({ name }) => warned.push(name)), warned };
  }

  it('warns once when the late confirmation comes after the browser kept it', () => {
    const w = watch();
    expect(w.watch.keptUncertain('h1', { name: 'a.zip', downloadId: null })).toBe(false);
    w.watch.confirmedLate('h1', 'a.zip', true);
    w.watch.confirmedLate('h1', 'a.zip', true);
    expect(w.warned).toEqual(['a.zip']);
  });

  it('warns once when the late confirmation came first', () => {
    const w = watch();
    w.watch.confirmedLate('h1', 'a.zip', true);
    expect(w.watch.keptUncertain('h1', { name: 'a.zip', downloadId: null })).toBe(true);
    expect(w.watch.keptUncertain('h1', { name: 'a.zip', downloadId: null })).toBe(false);
    expect(w.warned).toEqual(['a.zip']);
  });

  it('passes on whether the late answer was a sure confirmation', () => {
    const certainty: boolean[] = [];
    const duplicates = new DuplicateWatch((_copy, certain) => certainty.push(certain));
    duplicates.confirmedLate('h1', 'a.zip', false);
    duplicates.keptUncertain('h1', { name: 'a.zip', downloadId: null });
    duplicates.keptUncertain('h2', { name: 'b.zip', downloadId: null });
    duplicates.confirmedLate('h2', 'b.zip', true);
    expect(certainty).toEqual([false, true]);
  });

  it('does not match different handovers', () => {
    const w = watch();
    w.watch.confirmedLate('h1', 'a.zip', true);
    expect(w.watch.keptUncertain('h2', { name: 'b.zip', downloadId: null })).toBe(false);
    expect(w.warned).toEqual([]);
  });
});

describe('keptInBrowser', () => {
  it('explains the reasons about the app, without a doubled full stop', () => {
    expect(keptInBrowser(kept({ reason: 'app_unreachable' }), 'a.zip')).toBe(
      'a.zip stayed in the browser: Wirecatch could not be reached.',
    );
    expect(keptInBrowser(kept({ reason: 'no_answer' }), 'a.zip')).toBe(
      'a.zip stayed in the browser: Wirecatch did not answer in time.',
    );
    expect(keptInBrowser(kept({ reason: 'app_refused', detail: 'Disk full.' }), 'a.zip')).toBe(
      'a.zip stayed in the browser: Disk full.',
    );
    expect(keptInBrowser(kept({ reason: 'app_refused' }), 'a.zip')).toBe(
      'a.zip stayed in the browser: Wirecatch did not accept it.',
    );
  });

  it('says nothing for the settings or a download that could not be paused', () => {
    expect(keptInBrowser(kept({ reason: 'capture_off' }), 'a.zip')).toBeNull();
    expect(keptInBrowser(kept({ reason: 'pause_failed' }), 'a.zip')).toBeNull();
  });
});

describe('mediaAnswer', () => {
  it('says whether the app took it and what to do next', () => {
    expect(mediaAnswer({ taken: true, action: 'started', via: 'answer' })).toEqual({
      ok: true,
      message: 'Sent to Wirecatch.',
    });
    expect(mediaAnswer({ taken: true, action: 'awaiting_confirmation', via: 'answer' })).toEqual({
      ok: true,
      message: 'Sent to Wirecatch. Confirm it there to start.',
    });
    expect(
      mediaAnswer({ taken: false, reason: 'refused', detail: 'Not a video', uncertain: false }),
    ).toEqual({ ok: false, message: 'Wirecatch did not take it. Not a video.' });
    expect(
      mediaAnswer({ taken: false, reason: 'no_answer', detail: null, uncertain: true }).message,
    ).toContain('check its list');
    expect(
      mediaAnswer({ taken: false, reason: 'no_answer', detail: null, uncertain: false }).message,
    ).toContain('nothing was added');
  });

  it('says when the app answered in a way this version cannot read, not that it was silent', () => {
    const certain = mediaAnswer({
      taken: false,
      reason: 'unreadable',
      detail: null,
      uncertain: false,
    }).message;
    expect(certain).toBe(`Nothing was added. ${UNREADABLE_ANSWER}`);
    const uncertain = mediaAnswer({
      taken: false,
      reason: 'unreadable',
      detail: null,
      uncertain: true,
    }).message;
    expect(uncertain).toBe(
      `${UNREADABLE_ANSWER} Open Wirecatch and check its list before adding it again.`,
    );
  });
});

describe('NoticeBoard', () => {
  it('keeps the newest notices, at most MAX_NOTICES', () => {
    let now = 0;
    const board = new NoticeBoard({ now: () => (now += 1), changed: () => undefined });
    for (let i = 1; i <= MAX_NOTICES + 2; i += 1) board.warn(`Notice ${String(i)}.`);
    const shown = board.current.map((n) => n.message);
    expect(shown).toHaveLength(MAX_NOTICES);
    expect(shown[0]).toBe(`Notice ${String(MAX_NOTICES + 2)}.`);
  });

  it('drops other notices before one that needs something done, however old', () => {
    let now = 0;
    const board = new NoticeBoard({ now: () => (now += 1), changed: () => undefined });
    board.error('big.zip is paused.', { downloadId: 3, untilResumed: true });
    for (let i = 1; i <= MAX_NOTICES; i += 1) board.warn(`Notice ${String(i)}.`);
    expect(board.current).toHaveLength(MAX_NOTICES);
    expect(board.current.at(-1)?.message).toBe('big.zip is paused.');
    expect(board.current[0]?.message).toBe(`Notice ${String(MAX_NOTICES)}.`);
  });

  it('drops notices about a download that need nothing done before one that does', () => {
    let now = 0;
    const board = new NoticeBoard({ now: () => (now += 1), changed: () => undefined });
    board.error('big.zip is paused.', { downloadId: 3, untilResumed: true });
    for (let i = 1; i <= MAX_NOTICES; i += 1) {
      board.warn(`file${String(i)}.zip stayed in the browser.`, { downloadId: 10 + i });
    }
    expect(board.current).toHaveLength(MAX_NOTICES);
    expect(board.current.at(-1)?.message).toBe('big.zip is paused.');
  });

  it('leaves out a stored notice whose download moved or which was dismissed during the read', async () => {
    const stored: Notice[] = [
      {
        id: 'd7',
        downloadId: 7,
        untilResumed: true,
        urgent: true,
        tone: 'error',
        message: 'a.zip is paused.',
        at: 1,
      },
      {
        id: 'd8',
        downloadId: 8,
        untilResumed: true,
        urgent: true,
        tone: 'error',
        message: 'b.zip is paused.',
        at: 2,
      },
    ];
    let answer: (state: DownloadState) => void = () => undefined;
    const reading = new Promise<DownloadState>((resolve) => (answer = resolve));
    let now = 10;
    const board = new NoticeBoard({
      now: () => (now += 1),
      changed: () => undefined,
      store: { load: () => Promise.resolve(stored), save: () => Promise.resolve() },
      readCopy: () => reading,
    });
    const loading = board.load();
    await Promise.resolve();
    await Promise.resolve();
    board.downloadMoved(7);
    board.dismiss('d8');
    answer({ state: 'in_progress', paused: true });
    await loading;
    expect(board.current).toEqual([]);
  });

  it('reads the copy again when it moves while a late answer is being judged', async () => {
    const reads: ((state: DownloadState) => void)[] = [];
    let now = 0;
    const board = new NoticeBoard({
      now: () => (now += 1),
      changed: () => undefined,
      readCopy: () => new Promise<DownloadState>((resolve) => reads.push(resolve)),
    });
    board.uncertain('h1', 'a.zip', 3);
    board.confirmedLate('h1', 'a.zip', true);
    await Promise.resolve();
    board.downloadMoved(3);
    reads[0]?.({ state: 'in_progress', paused: true });
    await new Promise((resolve) => setTimeout(resolve, 0));
    reads[1]?.({ state: 'in_progress', paused: false });
    await new Promise((resolve) => setTimeout(resolve, 0));
    expect(board.current[0]?.message).toContain('Wirecatch took it after all');
  });

  it('dismisses one notice and leaves the others', () => {
    let now = 0;
    const board = new NoticeBoard({ now: () => (now += 1), changed: () => undefined });
    board.warn('First.');
    board.error('Second.', { downloadId: 3 });
    board.dismiss(board.current[1]?.id ?? '');
    expect(board.current.map((n) => n.message)).toEqual(['Second.']);
  });
});
