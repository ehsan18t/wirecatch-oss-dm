/**
 * The media button content script's logic, in jsdom: when it shows, where, what it
 * sends, and that it stays out of the page's way. Layout is faked through
 * `getBoundingClientRect`, since jsdom does none.
 */
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import {
  ACTIVE_MS,
  MESSAGE_MS,
  MediaButton,
  SHADOW_CSS,
  findMedia,
  placeButton,
  type Box,
  type ButtonDeps,
} from '../src/lib/media/button';
import type { DownloadAnswer, ElementInfo } from '../src/lib/media/messages';
import { flush } from './fakes';

const VIEWPORT = { width: 1024, height: 768 };
const SIZE = { width: 112, height: 32 };

function box(left: number, top: number, width: number, height: number): Box {
  return { left, top, width, height, right: left + width, bottom: top + height };
}

describe('placeButton', () => {
  it('puts the button in the top right corner of a video, inside it', () => {
    expect(
      placeButton(box(100, 50, 640, 360), { audio: false, controls: true }, VIEWPORT, SIZE),
    ).toEqual({
      top: 58,
      left: 620,
    });
  });

  it('keeps clear of native controls when the video is scrolled mostly out of view', () => {
    // Only the bottom 70 pixels are visible: not enough room above the controls.
    expect(
      placeButton(box(100, -290, 640, 360), { audio: false, controls: true }, VIEWPORT, SIZE),
    ).toBeNull();
    // Without native controls the same sliver has room.
    expect(
      placeButton(box(100, -290, 640, 360), { audio: false, controls: false }, VIEWPORT, SIZE),
    ).toEqual({
      top: 8,
      left: 620,
    });
  });

  it('puts the button just above an audio player, or below it at the top of the page', () => {
    expect(
      placeButton(box(40, 200, 300, 54), { audio: true, controls: true }, VIEWPORT, SIZE),
    ).toEqual({
      top: 164,
      left: 228,
    });
    expect(
      placeButton(box(40, 10, 300, 54), { audio: true, controls: true }, VIEWPORT, SIZE),
    ).toEqual({
      top: 68,
      left: 228,
    });
  });

  it('shows nothing for small, hidden or off-screen elements, and stays inside the viewport', () => {
    expect(
      placeButton(box(0, 0, 120, 68), { audio: false, controls: false }, VIEWPORT, SIZE),
    ).toBeNull();
    expect(
      placeButton(box(0, 0, 0, 0), { audio: true, controls: true }, VIEWPORT, SIZE),
    ).toBeNull();
    expect(
      placeButton(box(0, 900, 640, 360), { audio: false, controls: false }, VIEWPORT, SIZE),
    ).toBeNull();
    expect(
      placeButton(box(-700, 0, 640, 360), { audio: false, controls: false }, VIEWPORT, SIZE),
    ).toBeNull();
    expect(
      placeButton(box(600, 100, 800, 400), { audio: false, controls: false }, VIEWPORT, SIZE),
    ).toEqual({
      top: 108,
      left: 904,
    });
  });
});

interface Fake {
  deps: ButtonDeps;
  checks: ElementInfo[];
  downloads: Array<ElementInfo & { title: string | null }>;
  show: boolean;
  answer: DownloadAnswer | Error;
}

function fakeDeps(): Fake {
  const fake: Fake = {
    checks: [],
    downloads: [],
    show: true,
    answer: { ok: true, message: 'Sent to Wirecatch.' },
    deps: {
      check: (info) => {
        fake.checks.push(info);
        return Promise.resolve(fake.show);
      },
      download: (request) => {
        fake.downloads.push(request);
        return fake.answer instanceof Error
          ? Promise.reject(fake.answer)
          : Promise.resolve(fake.answer);
      },
      // jsdom marks every event as untrusted; the real check is tested on its own below.
      isTrusted: () => true,
    },
  };
  return fake;
}

function media(
  tag: 'video' | 'audio',
  rect: Box,
  attrs: Record<string, string> = {},
): HTMLMediaElement {
  const el = document.createElement(tag);
  for (const [name, value] of Object.entries(attrs)) el.setAttribute(name, value);
  el.getBoundingClientRect = () => ({ ...rect, x: rect.left, y: rect.top, toJSON: () => rect });
  document.body.append(el);
  return el;
}

const nextFrame = () => new Promise<void>((resolve) => requestAnimationFrame(() => resolve()));

let clock = 1_000_000;
let button: MediaButton | null = null;

function start(fake: Fake, enabled = true): MediaButton {
  button = new MediaButton(fake.deps, document, () => clock);
  button.setEnabled(enabled);
  return button;
}

async function settle(): Promise<void> {
  await flush();
  await nextFrame();
  await flush();
}

beforeEach(() => {
  clock = 1_000_000;
  Object.defineProperty(document, 'fullscreenElement', { value: null, configurable: true });
});

afterEach(() => {
  button?.destroy();
  button = null;
  document.body.innerHTML = '';
  document.title = '';
});

describe('MediaButton', () => {
  it('shows over a playing video once the background knows something to download, in a closed shadow root', async () => {
    const fake = fakeDeps();
    const video = media('video', box(100, 50, 640, 360), {
      src: 'https://cdn.example.com/clip.mp4',
      controls: '',
    });
    const b = start(fake);
    video.dispatchEvent(new Event('play'));
    await settle();
    expect(fake.checks).toEqual([{ src: 'https://cdn.example.com/clip.mp4', audio: false }]);
    expect(b.state).toBe('shown');
    expect(b.host.parentNode).toBe(document.documentElement);
    expect(b.host.shadowRoot).toBeNull();
    expect(b.host.style.getPropertyValue('top')).toBe('58px');
    expect(b.host.style.getPropertyValue('left')).toBe('620px');
  });

  it('stays hidden where nothing downloadable was detected, and asks again a little later', async () => {
    const fake = fakeDeps();
    fake.show = false;
    const video = media('video', box(0, 0, 640, 360), { src: 'blob:https://example.com/1' });
    const b = start(fake);
    video.dispatchEvent(new Event('play'));
    await settle();
    expect(b.state).toBe('none');
    expect(b.host.isConnected).toBe(false);

    fake.show = true;
    video.dispatchEvent(new Event('play'));
    await settle();
    expect(fake.checks).toHaveLength(1);
    clock += 3_500;
    video.dispatchEvent(new Event('play'));
    await settle();
    expect(fake.checks).toHaveLength(2);
    expect(b.state).toBe('shown');
  });

  it('follows the setting live: off hides it at once, on brings it back', async () => {
    const fake = fakeDeps();
    const video = media('video', box(0, 0, 640, 360), { src: 'https://e.com/a.mp4' });
    const b = start(fake, false);
    video.dispatchEvent(new Event('play'));
    await settle();
    expect(b.state).toBe('none');
    expect(fake.checks).toEqual([]);
    b.setEnabled(true);
    await settle();
    expect(b.state).toBe('shown');
    b.setEnabled(false);
    expect(b.state).toBe('none');
    expect(b.host.isConnected).toBe(false);
  });

  it('ignores decorative background videos, playing or hovered', async () => {
    const fake = fakeDeps();
    const video = media('video', box(0, 0, 640, 360), {
      src: 'https://e.com/bg.mp4',
      muted: '',
      loop: '',
    });
    const b = start(fake);
    video.dispatchEvent(new Event('play'));
    await settle();
    expect(b.state).toBe('none');
    video.dispatchEvent(
      new PointerEvent('pointermove', { bubbles: true, clientX: 300, clientY: 200 }),
    );
    await settle();
    expect(b.state).toBe('none');
    video.dispatchEvent(new FocusEvent('focusin', { bubbles: true, composed: true }));
    await settle();
    expect(b.state).toBe('none');
  });

  it("shows on hover, even under a player's transparent overlay", async () => {
    const fake = fakeDeps();
    media('video', box(0, 0, 640, 360), { src: 'https://e.com/a.mp4' });
    const overlay = document.createElement('div');
    document.body.append(overlay);
    const b = start(fake);
    overlay.dispatchEvent(
      new PointerEvent('pointermove', { bubbles: true, clientX: 300, clientY: 200 }),
    );
    await settle();
    expect(b.state).toBe('shown');

    overlay.dispatchEvent(
      new PointerEvent('pointermove', { bubbles: true, clientX: 900, clientY: 700 }),
    );
    await settle();
    expect(b.state).toBe('none');
  });

  it('fades out while a video plays untouched, but stays reachable with Tab and shows on focus', async () => {
    const fake = fakeDeps();
    const video = media('video', box(0, 0, 640, 360), { src: 'https://e.com/a.mp4' });
    const b = start(fake);
    video.dispatchEvent(new Event('play'));
    await settle();
    expect(b.state).toBe('shown');

    clock += ACTIVE_MS + 10;
    b.setEnabled(true);
    expect(b.state).toBe('idle');
    expect(b.host.isConnected).toBe(true);
    expect(b.button.tabIndex).toBe(0);

    b.button.focus();
    expect(document.activeElement).toBe(b.host);
    expect(b.state).toBe('shown');

    video.dispatchEvent(new Event('pause'));
    b.button.blur();
    expect(b.state).toBe('none');
  });

  it('lets only the visible button take the pointer, never the box around it', () => {
    const host = /:host\{([^}]*)\}/.exec(SHADOW_CSS)?.[1] ?? '';
    expect(host).toContain('pointer-events:none!important');
    expect(host).not.toContain('pointer-events:auto');
    expect(/\.w\{[^}]*pointer-events:none/.test(SHADOW_CSS)).toBe(true);
    expect(/\.b\{[^}]*pointer-events:auto/.test(SHADOW_CSS)).toBe(true);
    expect(/\.w\.idle \.b:not\(:focus-visible\)\{[^}]*pointer-events:none/.test(SHADOW_CSS)).toBe(
      true,
    );
  });

  it('comes back when the pointer moves over the button, and stays while it is on it', async () => {
    const fake = fakeDeps();
    const video = media('video', box(0, 0, 640, 360), { src: 'https://e.com/a.mp4' });
    const b = start(fake);
    video.dispatchEvent(new Event('play'));
    await settle();
    clock += ACTIVE_MS + 10;
    b.setEnabled(true);
    expect(b.state).toBe('idle');
    b.host.dispatchEvent(
      new PointerEvent('pointermove', { bubbles: true, clientX: 580, clientY: 20 }),
    );
    await settle();
    expect(b.state).toBe('shown');

    // An audio player's button sits outside the element: being on it keeps it shown.
    video.dispatchEvent(new Event('pause'));
    const audio = media('audio', box(40, 300, 300, 54), {
      src: 'https://e.com/b.mp3',
      controls: '',
    });
    audio.dispatchEvent(
      new PointerEvent('pointermove', { bubbles: true, clientX: 100, clientY: 320 }),
    );
    await settle();
    expect(b.state).toBe('shown');
    b.host.dispatchEvent(
      new PointerEvent('pointermove', { bubbles: true, clientX: 300, clientY: 280 }),
    );
    await settle();
    expect(b.state).toBe('shown');
  });

  it("finds media inside a page's open shadow roots, but not closed ones", () => {
    const outer = document.createElement('div');
    const inner = document.createElement('div');
    const closed = document.createElement('div');
    document.body.append(outer, closed);
    const outerRoot = outer.attachShadow({ mode: 'open' });
    outerRoot.append(inner);
    const deep = document.createElement('video');
    inner.attachShadow({ mode: 'open' }).append(deep);
    closed.attachShadow({ mode: 'closed' }).append(document.createElement('video'));
    const light = media('video', box(0, 0, 640, 360));
    expect(findMedia(document)).toEqual([light, deep]);
  });

  it('shows over a video playing inside an open shadow root, whose events never reach the page', async () => {
    const fake = fakeDeps();
    const player = document.createElement('fancy-player');
    document.body.append(player);
    const video = document.createElement('video');
    video.src = 'https://e.com/a.mp4';
    video.getBoundingClientRect = () => ({
      ...box(0, 0, 640, 360),
      x: 0,
      y: 0,
      toJSON: () => null,
    });
    player.attachShadow({ mode: 'open' }).append(video);
    const b = start(fake);
    // Media events are not composed: only a listener on the element itself hears this.
    video.dispatchEvent(new Event('play'));
    await settle();
    expect(b.state).toBe('shown');
  });

  it('notices a video in a shadow root added later, when it gets focus', async () => {
    const fake = fakeDeps();
    const b = start(fake);
    const player = document.createElement('fancy-player');
    document.body.append(player);
    const video = document.createElement('video');
    video.src = 'https://e.com/a.mp4';
    video.controls = true;
    video.getBoundingClientRect = () => ({
      ...box(0, 0, 640, 360),
      x: 0,
      y: 0,
      toJSON: () => null,
    });
    player.attachShadow({ mode: 'open' }).append(video);
    video.dispatchEvent(new FocusEvent('focusin', { bubbles: true, composed: true }));
    await settle();
    expect(b.state).toBe('shown');
    expect(fake.checks.at(-1)).toEqual({ src: 'https://e.com/a.mp4', audio: false });
  });

  it('sends only web addresses and a bounded title to the background', async () => {
    const fake = fakeDeps();
    const b = start(fake);
    for (const src of [
      'blob:https://e.com/1',
      'data:video/mp4;base64,AAAA',
      `https://e.com/${'a'.repeat(9000)}.mp4`,
    ]) {
      const video = media('video', box(0, 0, 640, 360), { src });
      video.dispatchEvent(new Event('play'));
      await settle();
      video.remove();
    }
    expect(fake.checks.map((c) => c.src)).toEqual([null, null, null]);
    document.title = 'T'.repeat(5000);
    const video = media('video', box(0, 0, 640, 360), { src: 'https://e.com/a.mp4' });
    video.dispatchEvent(new Event('play'));
    await settle();
    b.button.click();
    await settle();
    expect(fake.downloads[0]?.title?.length).toBe(1024);
  });

  it('does not rewrite its position while nothing moves', async () => {
    const fake = fakeDeps();
    const video = media('video', box(0, 0, 640, 360), { src: 'https://e.com/a.mp4' });
    const b = start(fake);
    video.dispatchEvent(new Event('play'));
    await settle();
    const writes: MutationRecord[] = [];
    const observer = new MutationObserver((records) => writes.push(...records));
    observer.observe(b.host, { attributes: true, attributeFilter: ['style'] });
    for (let i = 0; i < 3; i += 1) {
      document.dispatchEvent(new Event('scroll'));
      await settle();
    }
    expect(writes).toEqual([]);
    video.getBoundingClientRect = () => ({
      ...box(0, 100, 640, 360),
      x: 0,
      y: 100,
      toJSON: () => null,
    });
    document.dispatchEvent(new Event('scroll'));
    await settle();
    observer.disconnect();
    expect(writes).toHaveLength(1);
    expect(b.host.style.getPropertyValue('top')).toBe('108px');
  });

  describe('keyboard order', () => {
    const tab = (target: Element, shiftKey = false): KeyboardEvent => {
      const event = new KeyboardEvent('keydown', {
        key: 'Tab',
        shiftKey,
        bubbles: true,
        composed: true,
        cancelable: true,
      });
      target.dispatchEvent(event);
      return event;
    };

    it('moves from the focused video to the button with Tab, and back with Shift+Tab or Escape', async () => {
      const b = start(fakeDeps());
      const video = media('video', box(0, 0, 640, 360), {
        src: 'https://e.com/a.mp4',
        controls: '',
        tabindex: '0',
      });
      video.focus();
      await settle();
      expect(b.state).toBe('shown');

      expect(tab(video).defaultPrevented).toBe(true);
      expect(document.activeElement).toBe(b.host);
      const shiftTab = new KeyboardEvent('keydown', {
        key: 'Tab',
        shiftKey: true,
        bubbles: true,
        cancelable: true,
      });
      b.button.dispatchEvent(shiftTab);
      expect(shiftTab.defaultPrevented).toBe(true);
      expect(document.activeElement).toBe(video);

      tab(video);
      expect(document.activeElement).toBe(b.host);
      b.button.dispatchEvent(
        new KeyboardEvent('keydown', { key: 'Escape', bubbles: true, cancelable: true }),
      );
      expect(document.activeElement).toBe(video);

      // Tab on the button goes back to the video without cancelling, so the browser moves on from there.
      tab(video);
      const onward = new KeyboardEvent('keydown', { key: 'Tab', bubbles: true, cancelable: true });
      b.button.dispatchEvent(onward);
      expect(onward.defaultPrevented).toBe(false);
      expect(document.activeElement).toBe(video);
    });

    it("also from a focused player container about the video's size, but not from a large page area", async () => {
      const b = start(fakeDeps());
      const player = document.createElement('div');
      player.tabIndex = 0;
      player.getBoundingClientRect = () => ({
        ...box(0, 0, 640, 400),
        x: 0,
        y: 0,
        toJSON: () => null,
      });
      const page = document.createElement('main');
      page.tabIndex = -1;
      page.getBoundingClientRect = () => ({
        ...box(0, 0, 1024, 3000),
        x: 0,
        y: 0,
        toJSON: () => null,
      });
      document.body.append(page);
      page.append(player);
      const video = media('video', box(0, 0, 640, 360), { src: 'https://e.com/a.mp4' });
      player.append(video);
      player.focus();
      await settle();
      expect(b.state).toBe('shown');
      tab(player);
      expect(document.activeElement).toBe(b.host);
      b.button.dispatchEvent(
        new KeyboardEvent('keydown', { key: 'Escape', bubbles: true, cancelable: true }),
      );
      expect(document.activeElement).toBe(player);

      page.focus();
      expect(tab(page).defaultPrevented).toBe(false);
      expect(document.activeElement).toBe(page);
    });
  });

  it('keeps its message readable after the video stops being in use, until it expires', async () => {
    const fake = fakeDeps();
    fake.answer = {
      ok: false,
      message: 'Wirecatch could not be reached. Open Wirecatch, then try again.',
    };
    const saved = { ...MESSAGE_MS };
    MESSAGE_MS.error = 150;
    try {
      const video = media('video', box(0, 0, 640, 360), { src: 'https://e.com/a.mp4' });
      const b = start(fake);
      video.dispatchEvent(new Event('play'));
      await settle();
      b.button.click();
      await settle();
      // The video stops and the pointer is elsewhere: the button goes, its message stays.
      video.dispatchEvent(new Event('pause'));
      expect(b.state).toBe('message');
      expect(b.host.isConnected).toBe(true);
      expect(b.status.textContent).toContain('Open Wirecatch');
      await new Promise((resolve) => setTimeout(resolve, 200));
      expect(b.status.textContent).toBe('');
      expect(b.state).toBe('none');
      expect(b.host.isConnected).toBe(false);
    } finally {
      Object.assign(MESSAGE_MS, saved);
    }
  });

  it("shows the keyboard shortcut's answer in the corner when no button is showing", async () => {
    const saved = { ...MESSAGE_MS };
    MESSAGE_MS.ok = 150;
    try {
      const b = start(fakeDeps());
      b.announce('Sent to Wirecatch.', false);
      // The live region is in the page before its text arrives, so it is announced.
      expect(b.host.isConnected).toBe(true);
      expect(b.status.textContent).toBe('');
      await settle();
      expect(b.state).toBe('message');
      expect(b.status.textContent).toBe('Sent to Wirecatch.');
      await new Promise((resolve) => setTimeout(resolve, 200));
      expect(b.host.isConnected).toBe(false);
    } finally {
      Object.assign(MESSAGE_MS, saved);
    }
  });

  it('is a real, labelled button that keyboard users can reach', () => {
    const b = start(fakeDeps());
    expect(b.button.tagName).toBe('BUTTON');
    expect(b.button.type).toBe('button');
    expect(b.button.getAttribute('aria-label')).toBe('Download with Wirecatch');
    expect(b.button.textContent).toBe('Download');
    expect(b.status.getAttribute('role')).toBe('status');
  });

  it('only offers audio that shows its own player, placed above it', async () => {
    const fake = fakeDeps();
    const hidden = media('audio', box(40, 300, 300, 54), { src: 'https://e.com/a.mp3' });
    const shownAudio = media('audio', box(40, 200, 300, 54), {
      src: 'https://e.com/b.mp3',
      controls: '',
    });
    const b = start(fake);
    hidden.dispatchEvent(new Event('play'));
    await settle();
    expect(b.state).toBe('none');
    shownAudio.dispatchEvent(new FocusEvent('focusin', { bubbles: true }));
    await settle();
    expect(b.state).toBe('shown');
    expect(fake.checks).toEqual([{ src: 'https://e.com/b.mp3', audio: true }]);
    expect(b.host.style.getPropertyValue('top')).toBe('164px');
  });

  it('sends the element and the page title on click, and shows the answer', async () => {
    const fake = fakeDeps();
    document.title = 'A page';
    const video = media('video', box(0, 0, 640, 360), { src: 'https://e.com/a.mp4' });
    const b = start(fake);
    video.dispatchEvent(new Event('play'));
    await settle();
    b.button.click();
    expect(b.button.textContent).toBe('Sending…');
    expect(b.button.getAttribute('aria-busy')).toBe('true');
    await settle();
    expect(fake.downloads).toEqual([{ src: 'https://e.com/a.mp4', audio: false, title: 'A page' }]);
    expect(b.status.textContent).toBe('Sent to Wirecatch.');
    expect(b.button.textContent).toBe('Download');

    fake.answer = {
      ok: false,
      message: 'Wirecatch could not be reached. Open Wirecatch, then try again.',
    };
    b.button.click();
    await settle();
    expect(b.status.textContent).toBe(
      'Wirecatch could not be reached. Open Wirecatch, then try again.',
    );

    fake.answer = new Error('Extension context invalidated.');
    b.button.click();
    await settle();
    expect(b.status.textContent).toBe(
      'Wirecatch could not be reached. Reload this page, then try again.',
    );
  });

  it('ignores clicks a page script made up', async () => {
    const fake = fakeDeps();
    delete fake.deps.isTrusted;
    const video = media('video', box(0, 0, 640, 360), { src: 'https://e.com/a.mp4' });
    const b = start(fake);
    video.dispatchEvent(new Event('play'));
    await settle();
    b.button.click();
    b.button.dispatchEvent(new MouseEvent('click', { bubbles: true }));
    await settle();
    expect(fake.downloads).toEqual([]);
  });

  it("keeps its clicks and keys away from the page's player", async () => {
    const fake = fakeDeps();
    const video = media('video', box(0, 0, 640, 360), { src: 'https://e.com/a.mp4' });
    const seen = vi.fn();
    document.addEventListener('click', seen);
    document.addEventListener('keydown', seen);
    const b = start(fake);
    video.dispatchEvent(new Event('play'));
    await settle();
    b.button.click();
    b.button.dispatchEvent(
      new KeyboardEvent('keydown', { key: ' ', bubbles: true, composed: true }),
    );
    await settle();
    expect(seen).not.toHaveBeenCalled();
    document.removeEventListener('click', seen);
    document.removeEventListener('keydown', seen);
  });

  it('moves into a full screen player, and hides when the video itself is full screen', async () => {
    const fake = fakeDeps();
    const player = document.createElement('div');
    document.body.append(player);
    const video = media('video', box(0, 0, 640, 360), { src: 'https://e.com/a.mp4' });
    player.append(video);
    const b = start(fake);
    video.dispatchEvent(new Event('play'));
    await settle();

    Object.defineProperty(document, 'fullscreenElement', { value: player, configurable: true });
    document.dispatchEvent(new Event('fullscreenchange'));
    expect(b.host.parentNode).toBe(player);

    Object.defineProperty(document, 'fullscreenElement', { value: video, configurable: true });
    document.dispatchEvent(new Event('fullscreenchange'));
    expect(b.state).toBe('none');

    Object.defineProperty(document, 'fullscreenElement', { value: null, configurable: true });
    document.dispatchEvent(new Event('fullscreenchange'));
    expect(b.host.parentNode).toBe(document.documentElement);
  });

  it('removes itself and stops listening when destroyed', async () => {
    const fake = fakeDeps();
    const video = media('video', box(0, 0, 640, 360), { src: 'https://e.com/a.mp4' });
    const b = start(fake);
    video.dispatchEvent(new Event('play'));
    await settle();
    b.destroy();
    expect(b.host.isConnected).toBe(false);
    video.dispatchEvent(new Event('play'));
    await settle();
    expect(b.host.isConnected).toBe(false);
  });
});
