/** The media button's helpers, tested on their own; layout is faked, since jsdom does none. */
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { DownloadableChecks } from '../src/lib/media/checks';
import { LiveMessage, MESSAGE_MS } from '../src/lib/media/live-message';
import type { ElementInfo } from '../src/lib/media/messages';
import { mediaAt, playerMedia } from '../src/lib/media/page-media';
import type { Box } from '../src/lib/media/placement';
import { backoffDelay } from '../src/lib/native/client';
import { flush } from './fakes';

function box(left: number, top: number, width: number, height: number): Box {
  return { left, top, width, height, right: left + width, bottom: top + height };
}

function laidOut<T extends Element>(el: T, rect: Box): T {
  el.getBoundingClientRect = () => ({ ...rect, x: rect.left, y: rect.top, toJSON: () => rect });
  return el;
}

function video(rect: Box, attrs: Record<string, string> = {}): HTMLVideoElement {
  const el = document.createElement('video');
  for (const [name, value] of Object.entries(attrs)) el.setAttribute(name, value);
  return laidOut(el, rect);
}

beforeEach(() => {
  vi.useFakeTimers();
});

afterEach(() => {
  document.body.replaceChildren();
  vi.useRealTimers();
});

describe('DownloadableChecks', () => {
  it('asks once while waiting, keeps a yes, and asks a no again after a while', async () => {
    let now = 0;
    const asked: ElementInfo[] = [];
    const answers = [false, true];
    let answered = 0;
    const checks = new DownloadableChecks(
      (info) => {
        asked.push(info);
        return Promise.resolve(answers.shift() ?? false);
      },
      () => now,
      () => (answered += 1),
    );
    const el = video(box(0, 0, 320, 180), { src: 'https://cdn.example.com/a.mp4' });

    expect(checks.allowed(el)).toBe(false);
    expect(checks.allowed(el)).toBe(false);
    await flush();
    expect(asked).toEqual([{ src: 'https://cdn.example.com/a.mp4', audio: false }]);
    expect(answered).toBe(1);

    // A no is kept for a while, then asked again.
    expect(checks.allowed(el)).toBe(false);
    expect(asked).toHaveLength(1);
    now = 3000;
    expect(checks.allowed(el)).toBe(false);
    await flush();
    expect(asked).toHaveLength(2);

    // A yes is kept for good.
    now = 60_000;
    expect(checks.allowed(el)).toBe(true);
    expect(asked).toHaveLength(2);
  });

  it('asks again when the element plays another source, and reads a failure as no', async () => {
    const asked: (string | null)[] = [];
    const checks = new DownloadableChecks(
      (info) => {
        asked.push(info.src);
        return info.src?.endsWith('b.mp4') === true
          ? Promise.reject(new Error('gone'))
          : Promise.resolve(true);
      },
      () => 0,
      () => undefined,
    );
    const el = video(box(0, 0, 320, 180), { src: 'https://cdn.example.com/a.mp4' });
    checks.allowed(el);
    await flush();
    expect(checks.allowed(el)).toBe(true);
    el.setAttribute('src', 'https://cdn.example.com/b.mp4');
    expect(checks.allowed(el)).toBe(true);
    await flush();
    expect(asked).toEqual(['https://cdn.example.com/a.mp4', 'https://cdn.example.com/b.mp4']);
    expect(checks.allowed(el)).toBe(false);
  });
});

describe('LiveMessage', () => {
  it('shows a message and clears it after its time, longer for errors', () => {
    const el = document.createElement('span');
    let expired = 0;
    const message = new LiveMessage(el, () => (expired += 1));

    message.say('Sent to Wirecatch.', false);
    expect(message.showing).toBe(true);
    expect(el.classList.contains('err')).toBe(false);
    vi.advanceTimersByTime(MESSAGE_MS.ok);
    expect(el.textContent).toBe('');
    expect(expired).toBe(1);

    message.say('Wirecatch could not be reached.', true);
    expect(el.classList.contains('err')).toBe(true);
    vi.advanceTimersByTime(MESSAGE_MS.ok);
    expect(message.showing).toBe(true);
    vi.advanceTimersByTime(MESSAGE_MS.error - MESSAGE_MS.ok);
    expect(message.showing).toBe(false);
  });

  it('clears at once with an empty message, and stop keeps a message from clearing', () => {
    const el = document.createElement('span');
    let expired = 0;
    const message = new LiveMessage(el, () => (expired += 1));
    message.say('Sent to Wirecatch.', false);
    message.say('', false);
    expect(message.showing).toBe(false);
    message.say('Sent to Wirecatch.', false);
    message.stop();
    vi.advanceTimersByTime(MESSAGE_MS.error);
    expect(message.showing).toBe(true);
    expect(expired).toBe(0);
  });
});

describe('playerMedia', () => {
  it('finds the media a player container about its size stands for', () => {
    const player = laidOut(document.createElement('div'), box(0, 0, 330, 190));
    const el = video(box(0, 0, 320, 180));
    player.append(el);
    document.body.append(player);
    expect(playerMedia(player, [el])).toBe(el);
  });

  it('ignores a large container, media outside it, and decoration', () => {
    const page = laidOut(document.createElement('main'), box(0, 0, 1200, 900));
    const inPage = video(box(0, 0, 320, 180));
    page.append(inPage);
    const player = laidOut(document.createElement('div'), box(0, 0, 330, 190));
    const background = video(box(0, 0, 320, 180), { muted: '', loop: '' });
    player.append(background);
    const elsewhere = video(box(0, 0, 320, 180));
    document.body.append(page, player, elsewhere);
    expect(playerMedia(page, [inPage])).toBeNull();
    expect(playerMedia(player, [elsewhere, background])).toBeNull();
  });
});

describe('mediaAt', () => {
  it('finds the last usable media under the point and forgets media gone from the page', () => {
    const back = video(box(0, 0, 400, 300));
    const front = video(box(100, 100, 200, 150));
    const decoration = video(box(0, 0, 400, 300), { muted: '', loop: '' });
    const gone = video(box(0, 0, 400, 300));
    document.body.append(back, front, decoration);
    const tracked = new Set<HTMLMediaElement>([back, front, decoration, gone]);

    expect(mediaAt(tracked, 150, 150)).toBe(front);
    expect(mediaAt(tracked, 10, 10)).toBe(back);
    expect(mediaAt(tracked, 500, 500)).toBeNull();
    expect(tracked.has(gone)).toBe(false);
  });
});

describe('backoffDelay', () => {
  it('walks the table and then repeats its last delay', () => {
    const table = [1000, 2000, 4000] as const;
    expect([0, 1, 2, 3, 10].map((attempt) => backoffDelay(table, attempt))).toEqual([
      1000, 2000, 4000, 4000, 4000,
    ]);
    expect([0, 5].map((attempt) => backoffDelay([60_000], attempt))).toEqual([60_000, 60_000]);
  });
});
