/** The hub's pure parts, tested on their own: timing defaults, menu planning, duplicate matching and its sentences. */
import { describe, expect, it } from 'vitest';
import type { HandoverResult } from '../src/lib/capture/handover';
import { DuplicateWatch } from '../src/lib/hub/duplicates';
import { hubTiming } from '../src/lib/hub/hub';
import { planMenuClick } from '../src/lib/hub/menu';
import { keptInBrowser, mediaAnswer } from '../src/lib/hub/notices';

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
      connectWaitMs: 6_000,
      requestTimeoutMs: 10_000,
      withdrawTimeoutMs: 3_000,
    });
    expect(hubTiming({ connectWaitMs: undefined, requestTimeoutMs: 50 })).toEqual({
      ...defaults,
      requestTimeoutMs: 50,
    });
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
    return { watch: new DuplicateWatch((name) => warned.push(name)), warned };
  }

  it('warns once when the late confirmation comes after the browser kept it', () => {
    const w = watch();
    expect(w.watch.keptUncertain('h1', 'a.zip')).toBe(false);
    w.watch.confirmedLate('h1', 'a.zip');
    w.watch.confirmedLate('h1', 'a.zip');
    expect(w.warned).toEqual(['a.zip']);
  });

  it('warns once when the late confirmation came first', () => {
    const w = watch();
    w.watch.confirmedLate('h1', 'a.zip');
    expect(w.watch.keptUncertain('h1', 'a.zip')).toBe(true);
    expect(w.watch.keptUncertain('h1', 'a.zip')).toBe(false);
    expect(w.warned).toEqual(['a.zip']);
  });

  it('does not match different handovers', () => {
    const w = watch();
    w.watch.confirmedLate('h1', 'a.zip');
    expect(w.watch.keptUncertain('h2', 'b.zip')).toBe(false);
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
    expect(certain).toContain('does not understand');
    expect(certain).toContain('nothing was added');
    expect(certain).not.toContain('in time');
    const uncertain = mediaAnswer({
      taken: false,
      reason: 'unreadable',
      detail: null,
      uncertain: true,
    }).message;
    expect(uncertain).toContain('does not understand');
    expect(uncertain).toContain('check its list');
  });
});
