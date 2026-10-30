/**
 * The app's notices in the browser (D65): which tab gets a notice, that only the frame a page script
 * placed receives it, what is told back to the app and when, and the notices page itself with its
 * buttons, close buttons and Clear all.
 */
import {
  NOTICE_ANSWER_WAIT_MS,
  noticeId,
  type BrowserNotice,
  type NoticeId,
} from '@wirecatch/protocol';
import { render, screen } from '@testing-library/svelte';
import userEvent from '@testing-library/user-event';
import { flushSync } from 'svelte';
import { afterEach, describe, expect, it, vi } from 'vitest';
import {
  CONNECT_WAIT_MS,
  NoticeCenter,
  type NoticeCenterDeps,
  type NoticesPort,
} from '../src/lib/notices/center';
import { focusedWebTab, type FocusApi } from '../src/lib/notices/focus';
import { NoticesFrame } from '../src/lib/notices/frame';
import {
  NOTICES_PORT,
  SIZE_CHANNEL,
  type FromNoticesPage,
  type ToNoticesPage,
} from '../src/lib/notices/messages';
import { NoticesPage } from '../src/lib/notices/page.svelte';
import Notices from '../src/ui/notices/Notices.svelte';
import { flush } from './fakes';

afterEach(() => {
  vi.useRealTimers();
});

/** The next message `port` gets: waits for the message itself, for no set time. */
function nextMessage(port: MessagePort): Promise<unknown> {
  return new Promise((resolve) => {
    port.onmessage = (event) => {
      resolve(event.data);
    };
  });
}

function notice(id: number, overrides: Partial<BrowserNotice> = {}): BrowserNotice {
  return {
    id: noticeId(id),
    kind: 'finished',
    title: 'Download finished',
    body: `file-${String(id)}.zip`,
    buttons: ['Show in folder'],
    stays_ms: 8000,
    ...overrides,
  };
}

/** A notices page's port, with what the background sent it and a way to send as the page. */
function fakePort(name = NOTICES_PORT) {
  const listeners: ((message: unknown) => void)[] = [];
  const gone: (() => void)[] = [];
  const received: ToNoticesPage[] = [];
  const port: NoticesPort & { disconnected: boolean } = {
    name,
    disconnected: false,
    postMessage: (message) => received.push(message),
    disconnect: () => {
      port.disconnected = true;
    },
    onMessage: { addListener: (listener) => listeners.push(listener) },
    onDisconnect: { addListener: (listener) => gone.push(listener) },
  };
  return {
    port,
    received,
    send: (message: FromNoticesPage | Record<string, unknown>) => {
      for (const listener of listeners) listener(message);
    },
    close: () => {
      for (const listener of gone) listener();
    },
  };
}

/** The notices among `received`. */
function noticesIn(received: ToNoticesPage[]): BrowserNotice[] {
  return received.flatMap((m) => (m.type === 'notice' ? [m.notice] : []));
}

function center(overrides: Partial<NoticeCenterDeps> = {}, clock = { now: 0 }) {
  const shown: NoticeId[] = [];
  const pressed: [NoticeId, number | null][] = [];
  const unmounted: number[] = [];
  const told = { shown, pressed, unmounted };
  const deps: NoticeCenterDeps = {
    focusedTab: () => Promise.resolve(7),
    mount: () => Promise.resolve('token-7'),
    unmount: (tabId) => {
      unmounted.push(tabId);
      return Promise.resolve();
    },
    shown: (id) => {
      shown.push(id);
      return Promise.resolve();
    },
    pressed: (id, button) => {
      pressed.push([id, button]);
      return Promise.resolve();
    },
    now: () => clock.now,
    ...overrides,
  };
  return { notices: new NoticeCenter(deps), told, deps };
}

/** A notices page's port that said hello with `token`. */
function provedPage(notices: NoticeCenter, token = 'token-7') {
  const page = fakePort();
  notices.connect(page.port);
  page.send({ type: 'hello', token });
  return page;
}

describe('the notice center', () => {
  it('gives notices to the frame of the tab the person looks at, and tells the app each once it is on screen', async () => {
    const mount = vi.fn(() => Promise.resolve('token-7'));
    const { notices, told } = center({ mount });
    await Promise.all([notices.show(notice(1)), notices.show(notice(2))]);
    expect(mount).toHaveBeenCalledOnce();
    expect(told.shown).toEqual([]);

    const page = provedPage(notices);
    expect(noticesIn(page.received).map((n) => n.id)).toEqual([noticeId(1), noticeId(2)]);
    page.send({ type: 'shown', id: noticeId(1) });
    page.send({ type: 'shown', id: noticeId(99) });
    await notices.show(notice(3));
    expect(noticesIn(page.received).at(-1)?.id).toBe(noticeId(3));
    await flush();
    expect(told.shown).toEqual([noticeId(1)]);
  });

  it('asks for no frame while the browser is not in front, and tells nothing when the page cannot take one', async () => {
    const mount = vi.fn(() => Promise.resolve('token-7'));
    const behind = center({ focusedTab: () => Promise.resolve(null), mount });
    await behind.notices.show(notice(1));
    expect(mount).not.toHaveBeenCalled();

    const noScript = center({ mount: () => Promise.reject(new Error('no receiving end')) });
    await noScript.notices.show(notice(1));
    expect(noScript.told.shown).toEqual([]);
  });

  it('gives nothing to a page without the frame’s token, such as a website framing the notices page', async () => {
    const { notices } = center();
    await notices.show(notice(1));
    const stranger = provedPage(notices, 'guessed');
    expect(stranger.received).toEqual([]);
    expect(stranger.port.disconnected).toBe(true);
  });

  it('passes on a pressed button only for a notice it sent, and only from the proved page', async () => {
    const { notices, told } = center();
    await notices.show(notice(1));
    const early = fakePort();
    notices.connect(early.port);
    early.send({ type: 'pressed', id: noticeId(1), button: 0 });
    const page = provedPage(notices);
    page.send({ type: 'pressed', id: noticeId(1), button: 1 });
    page.send({ type: 'pressed', id: noticeId(99), button: 0 });
    page.send({ type: 'pressed', id: noticeId(1), button: -1 });
    page.send({ type: 'pressed', id: noticeId(1), button: null });
    await flush();
    expect(told.pressed).toEqual([
      [noticeId(1), 1],
      [noticeId(1), null],
    ]);
  });

  it('takes the frame away once empty, but not when a newer notice crossed the word', async () => {
    const { notices, told } = center();
    await notices.show(notice(1));
    const page = provedPage(notices);
    await notices.show(notice(2));
    page.send({ type: 'empty', last: noticeId(1) });
    await flush();
    expect(told.unmounted).toEqual([]);
    page.send({ type: 'empty', last: noticeId(2) });
    await flush();
    expect(told.unmounted).toEqual([7]);
    expect(page.port.disconnected).toBe(true);
  });

  it('places a fresh frame once the old one lost its way, as after a restart', async () => {
    const mount = vi.fn(() => Promise.resolve('token-7'));
    const { notices } = center({ mount });
    await notices.show(notice(1));
    provedPage(notices).close();
    await notices.show(notice(2));
    expect(mount).toHaveBeenCalledTimes(2);
  });

  it('takes away a frame that never connects, so the tab gets a working one next time', async () => {
    vi.useFakeTimers();
    const mount = vi.fn(() => Promise.resolve('token-7'));
    const { notices, told } = center({ mount });
    await notices.show(notice(1));
    await vi.advanceTimersByTimeAsync(CONNECT_WAIT_MS);
    expect(told.unmounted).toEqual([7]);
    await notices.show(notice(2));
    expect(mount).toHaveBeenCalledTimes(2);
  });

  it('gives a frame that connects late none of the notices the app told another way meanwhile', async () => {
    const clock = { now: 0 };
    const { notices } = center({}, clock);
    await notices.show(notice(1));
    clock.now = NOTICE_ANSWER_WAIT_MS;
    await notices.show(notice(2));
    clock.now = NOTICE_ANSWER_WAIT_MS + 1;
    const page = provedPage(notices);
    expect(noticesIn(page.received).map((n) => n.id)).toEqual([noticeId(2)]);
  });

  it('moves the notices still open to the tab the person turns to, and a closed one never comes back', async () => {
    const tab = { now: 7 };
    const clock = { now: 0 };
    const { notices, told } = center(
      {
        focusedTab: () => Promise.resolve(tab.now),
        mount: (tabId) => Promise.resolve(`token-${String(tabId)}`),
      },
      clock,
    );
    await notices.show(notice(1, { kind: 'failed', stays_ms: null }));
    await notices.show(notice(2));
    const first = provedPage(notices, 'token-7');
    first.send({ type: 'gone', id: noticeId(2) });

    clock.now = 3000;
    tab.now = 9;
    await notices.focusChanged();
    expect(told.unmounted).toEqual([7]);
    const second = provedPage(notices, 'token-9');
    expect(noticesIn(second.received)).toEqual([notice(1, { kind: 'failed', stays_ms: null })]);

    second.send({ type: 'gone', id: noticeId(1) });
    second.send({ type: 'empty', last: noticeId(1) });
    tab.now = 7;
    await notices.focusChanged();
    expect(provedPage(notices, 'token-7').received).toEqual([]);
  });

  it('moves a notice with the time it has left, and not one whose time is up', async () => {
    const tab = { now: 7 };
    const clock = { now: 0 };
    const { notices } = center(
      {
        focusedTab: () => Promise.resolve(tab.now),
        mount: (tabId) => Promise.resolve(`token-${String(tabId)}`),
      },
      clock,
    );
    await notices.show(notice(1, { stays_ms: 8000 }));
    await notices.show(notice(2, { stays_ms: 2000 }));
    provedPage(notices, 'token-7');
    clock.now = 3000;
    tab.now = 9;
    await notices.focusChanged();
    const moved = noticesIn(provedPage(notices, 'token-9').received);
    expect(moved).toEqual([notice(1, { stays_ms: 5000 })]);
  });

  it('takes a notice the app took back off the page, and never moves it again', async () => {
    const tab = { now: 7 };
    const { notices } = center({
      focusedTab: () => Promise.resolve(tab.now),
      mount: (tabId) => Promise.resolve(`token-${String(tabId)}`),
    });
    await notices.show(notice(1, { kind: 'failed', stays_ms: null }));
    const page = provedPage(notices, 'token-7');
    notices.withdraw(noticeId(1));
    expect(page.received.at(-1)).toEqual({ type: 'withdraw', id: noticeId(1) });
    tab.now = 9;
    await notices.focusChanged();
    expect(provedPage(notices, 'token-9').received).toEqual([]);
  });

  it('keeps the frame while the browser is not in front', async () => {
    const tab = { now: 7 as number | null };
    const { notices, told } = center({ focusedTab: () => Promise.resolve(tab.now) });
    await notices.show(notice(1));
    provedPage(notices);
    tab.now = null;
    await notices.focusChanged();
    expect(told.unmounted).toEqual([]);
  });

  it('ignores ports of other names', () => {
    const { notices } = center();
    const other = fakePort('wirecatch-popup');
    notices.connect(other.port);
    other.send({ type: 'hello', token: 'token-7' });
    expect(other.port.disconnected).toBe(false);
  });
});

describe('the tab a notice goes to', () => {
  const tab = (url: string) => ({ id: 5, url });
  function api(
    window: Partial<Awaited<ReturnType<FocusApi['lastFocusedWindow']>>>,
    url = 'https://example.com/',
    privateWindows = true,
  ) {
    return {
      lastFocusedWindow: () =>
        Promise.resolve({ id: 1, focused: true, state: 'normal', incognito: false, ...window }),
      activeTab: () => Promise.resolve(tab(url)),
      privateWindowsOn: () => Promise.resolve(privateWindows),
    } satisfies FocusApi;
  }

  it('is the active tab of the browser in front, when it shows a website', async () => {
    expect(await focusedWebTab(api({}))).toBe(5);
  });

  it('is in a private window too, unless private windows are off (D82)', async () => {
    expect(await focusedWebTab(api({ incognito: true }))).toBe(5);
    expect(await focusedWebTab(api({ incognito: true }, 'https://example.com/', false))).toBeNull();
    expect(await focusedWebTab(api({}, 'https://example.com/', false))).toBe(5);
  });

  it('is none behind another program, minimized, or on a page that is not a website', async () => {
    expect(await focusedWebTab(api({ focused: false }))).toBeNull();
    expect(await focusedWebTab(api({ state: 'minimized' }))).toBeNull();
    expect(await focusedWebTab(api({}, 'chrome://newtab/'))).toBeNull();
    expect(await focusedWebTab(api({}, 'file:///C:/report.pdf'))).toBeNull();
  });
});

describe('the notices frame', () => {
  function frame() {
    let next = 0;
    return new NoticesFrame({
      container: () => document.documentElement,
      noticesUrl: (token) => `chrome-extension://id/notices.html#${token}`,
      extensionOrigin: 'chrome-extension://id',
      newToken: () => `token-${String(++next)}`,
    });
  }

  it('keeps the page’s elements the same whether a notice shows or not, the frame changing only in a closed shadow root', async () => {
    const notices = frame();
    const host = document.documentElement.lastElementChild;
    expect(host?.shadowRoot).toBeNull();
    const seen: MutationRecord[] = [];
    const observer = new MutationObserver((records) => seen.push(...records));
    observer.observe(document, { subtree: true, childList: true, attributes: true });
    expect(notices.mount()).toBe('token-1');
    expect(notices.mount()).toBe('token-2');
    notices.unmount();
    await flush();
    observer.disconnect();
    expect(seen).toEqual([]);
    expect(document.querySelector('iframe')).toBeNull();
    notices.destroy();
    expect(host?.isConnected).toBe(false);
  });

  it('takes no notices once the website took its host away, rather than show it one came', () => {
    const notices = frame();
    document.documentElement.lastElementChild?.remove();
    expect(notices.mount()).toBeNull();
    notices.destroy();
  });

  it('hands its notices page alone a channel for sizes, at the extension’s origin', async () => {
    // A test can reach the closed shadow root; a website's script cannot.
    const attach = vi.spyOn(Element.prototype, 'attachShadow');
    const notices = frame();
    notices.mount();
    const shadow = attach.mock.results[0]?.value as ShadowRoot | undefined;
    const iframe = shadow?.querySelector('iframe');
    if (!iframe?.contentWindow) throw new Error('no frame in the shadow root');
    expect(iframe.src).toBe('chrome-extension://id/notices.html#token-1');
    const post = vi.spyOn(iframe.contentWindow, 'postMessage').mockImplementation(() => undefined);
    iframe.dispatchEvent(new Event('load'));
    // The spy takes the two-argument overload's type; the frame calls the one with an origin and ports.
    const call = post.mock.calls[0] as unknown as [unknown, string, MessagePort[]] | undefined;
    const [message, origin, ports] = call ?? [];
    expect([message, origin]).toEqual([
      { type: SIZE_CHANNEL, token: 'token-1' },
      'chrome-extension://id',
    ]);
    const [sizes] = ports ?? [];
    if (!sizes) throw new Error('no channel handed over');
    // Waits for the frame to take the size, for no set time.
    const resized = new Promise<void>((resolve) => {
      const observer = new MutationObserver(() => {
        observer.disconnect();
        resolve();
      });
      observer.observe(iframe, { attributes: true, attributeFilter: ['style'] });
    });
    sizes.postMessage({ wirecatchNoticesWidth: 352.2, wirecatchNoticesHeight: 180.5 });
    await resized;
    expect([iframe.style.width, iframe.style.height]).toEqual(['353px', '181px']);
    sizes.close();
    notices.destroy();
  });
});

describe('the notices page', () => {
  function page() {
    const sent: FromNoticesPage[] = [];
    const notices = new NoticesPage((message) => sent.push(message));
    render(Notices, { page: notices });
    return { notices, sent };
  }

  it('says a notice is on screen once its frame has room for it, sends the button pressed, and the last notice once none is left', async () => {
    const { notices, sent } = page();
    notices.show(notice(1));
    flushSync();
    expect(screen.getByText('Download finished')).toBeTruthy();
    expect(screen.getByText('file-1.zip')).toBeTruthy();
    expect(sent).toEqual([]);
    const channel = new MessageChannel();
    notices.sizesTo(channel.port1);
    notices.resized({ wirecatchNoticesWidth: 300, wirecatchNoticesHeight: 90 });
    await userEvent.click(screen.getByRole('button', { name: 'Show in folder' }));
    expect(sent).toEqual([
      { type: 'shown', id: noticeId(1) },
      { type: 'pressed', id: noticeId(1), button: 0 },
      { type: 'gone', id: noticeId(1) },
      { type: 'empty', last: noticeId(1) },
    ]);
    channel.port1.close();
    notices.dispose();
  });

  it('lets its notices go when the background lets the frame go, still open for the next tab', () => {
    const { notices, sent } = page();
    notices.show(notice(1, { stays_ms: null }));
    notices.lost();
    flushSync();
    expect(notices.toaster.list).toEqual([]);
    expect(sent.filter((m) => m.type === 'gone')).toEqual([]);
    notices.dispose();
  });

  it('opens the download in the app from the notice’s text', async () => {
    const { notices, sent } = page();
    notices.show(notice(1));
    flushSync();
    await userEvent.click(screen.getByRole('button', { name: /Download finished/ }));
    expect(sent).toContainEqual({ type: 'pressed', id: noticeId(1), button: null });
    notices.dispose();
  });

  it('lets a notice the app took back go', () => {
    const { notices, sent } = page();
    notices.show(notice(1, { stays_ms: null }));
    notices.show(notice(2, { stays_ms: null }));
    flushSync();
    notices.withdraw(noticeId(1));
    flushSync();
    expect(notices.toaster.list.map((t) => t.message)).toEqual(['file-2.zip']);
    expect(sent.filter((m) => m.type === 'gone')).toEqual([{ type: 'gone', id: noticeId(1) }]);
    notices.toaster.clear();
    flushSync();
    notices.dispose();
  });

  it('closes one notice, or all of them at once', async () => {
    const { notices, sent } = page();
    notices.show(notice(1, { kind: 'started', buttons: [], stays_ms: 3000 }));
    notices.show(
      notice(2, { kind: 'failed', title: 'Download failed', buttons: [], stays_ms: null }),
    );
    flushSync();
    expect(screen.getAllByRole('button', { name: 'Dismiss' })).toHaveLength(2);
    await userEvent.click(screen.getByRole('button', { name: 'Clear all' }));
    expect(notices.toaster.list).toEqual([]);
    expect(sent.filter((m) => m.type === 'gone')).toEqual([
      { type: 'gone', id: noticeId(1) },
      { type: 'gone', id: noticeId(2) },
    ]);
    expect(sent.at(-1)).toEqual({ type: 'empty', last: noticeId(2) });
    notices.dispose();
  });

  it('keeps a failure until it is closed, and lets a finished notice go by itself', () => {
    vi.useFakeTimers();
    const { notices } = page();
    notices.show(notice(1, { stays_ms: 8000 }));
    notices.show(notice(2, { kind: 'failed', stays_ms: null }));
    vi.advanceTimersByTime(8000);
    expect(notices.toaster.list.map((t) => t.tone)).toEqual(['danger']);
    notices.dispose();
  });

  it('sends its size only over the channel it was handed, the latest at once, and none without notices', async () => {
    const notices = new NoticesPage(() => undefined);
    notices.show(notice(1));
    notices.resized({ wirecatchNoticesWidth: 1, wirecatchNoticesHeight: 2 });
    notices.resized({ wirecatchNoticesWidth: 3, wirecatchNoticesHeight: 4 });
    const channel = new MessageChannel();
    const first = nextMessage(channel.port2);
    notices.sizesTo(channel.port1);
    expect(await first).toEqual({ wirecatchNoticesWidth: 3, wirecatchNoticesHeight: 4 });
    notices.toaster.clear();
    const none = nextMessage(channel.port2);
    notices.resized({ wirecatchNoticesWidth: 20, wirecatchNoticesHeight: 20 });
    expect(await none).toEqual({ wirecatchNoticesWidth: 0, wirecatchNoticesHeight: 0 });
    channel.port1.close();
    notices.dispose();
  });
});
