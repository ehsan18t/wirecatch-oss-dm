/**
 * The app's notes in the browser (D65): which tab gets a note, that only the frame a page script
 * placed receives it, what is told back to the app and when, and the notes page itself with its
 * buttons, close buttons and Clear all.
 */
import { NOTE_ANSWER_WAIT_MS, noteId, type BrowserNote, type NoteId } from '@wirecatch/protocol';
import { render, screen } from '@testing-library/svelte';
import userEvent from '@testing-library/user-event';
import { flushSync } from 'svelte';
import { afterEach, describe, expect, it, vi } from 'vitest';
import {
  CONNECT_WAIT_MS,
  NoteCenter,
  type NoteCenterDeps,
  type NotesPort,
} from '../src/lib/notes/center';
import { focusedWebTab, type FocusApi } from '../src/lib/notes/focus';
import { NotesFrame } from '../src/lib/notes/frame';
import {
  NOTES_PORT,
  SIZE_CHANNEL,
  type FromNotesPage,
  type ToNotesPage,
} from '../src/lib/notes/messages';
import { NotesPage } from '../src/lib/notes/page.svelte';
import Notes from '../src/ui/notes/Notes.svelte';
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

function note(id: number, overrides: Partial<BrowserNote> = {}): BrowserNote {
  return {
    id: noteId(id),
    kind: 'finished',
    title: 'Download finished',
    body: `file-${String(id)}.zip`,
    buttons: ['Show in folder'],
    stays_ms: 8000,
    ...overrides,
  };
}

/** A notes page's port, with what the background sent it and a way to send as the page. */
function fakePort(name = NOTES_PORT) {
  const listeners: ((message: unknown) => void)[] = [];
  const gone: (() => void)[] = [];
  const received: ToNotesPage[] = [];
  const port: NotesPort & { disconnected: boolean } = {
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
    send: (message: FromNotesPage | Record<string, unknown>) => {
      for (const listener of listeners) listener(message);
    },
    close: () => {
      for (const listener of gone) listener();
    },
  };
}

function center(overrides: Partial<NoteCenterDeps> = {}, clock = { now: 0 }) {
  const shown: NoteId[] = [];
  const pressed: [NoteId, number][] = [];
  const unmounted: number[] = [];
  const told = { shown, pressed, unmounted };
  const deps: NoteCenterDeps = {
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
  return { notes: new NoteCenter(deps), told, deps };
}

/** A notes page's port that said hello with `token`. */
function provedPage(notes: NoteCenter, token = 'token-7') {
  const page = fakePort();
  notes.connect(page.port);
  page.send({ type: 'hello', token });
  return page;
}

describe('the note center', () => {
  it('gives notes to the frame of the tab the person looks at, and tells the app each once it is on screen', async () => {
    const mount = vi.fn(() => Promise.resolve('token-7'));
    const { notes, told } = center({ mount });
    await Promise.all([notes.show(note(1)), notes.show(note(2))]);
    expect(mount).toHaveBeenCalledOnce();
    expect(told.shown).toEqual([]);

    const page = provedPage(notes);
    expect(page.received.map((m) => m.note.id)).toEqual([noteId(1), noteId(2)]);
    page.send({ type: 'shown', id: noteId(1) });
    page.send({ type: 'shown', id: noteId(99) });
    await notes.show(note(3));
    expect(page.received.at(-1)?.note.id).toBe(noteId(3));
    await flush();
    expect(told.shown).toEqual([noteId(1)]);
  });

  it('asks for no frame while the browser is not in front, and tells nothing when the page cannot take one', async () => {
    const mount = vi.fn(() => Promise.resolve('token-7'));
    const behind = center({ focusedTab: () => Promise.resolve(null), mount });
    await behind.notes.show(note(1));
    expect(mount).not.toHaveBeenCalled();

    const noScript = center({ mount: () => Promise.reject(new Error('no receiving end')) });
    await noScript.notes.show(note(1));
    expect(noScript.told.shown).toEqual([]);
  });

  it('gives nothing to a page without the frame’s token, such as a website framing the notes page', async () => {
    const { notes } = center();
    await notes.show(note(1));
    const stranger = provedPage(notes, 'guessed');
    expect(stranger.received).toEqual([]);
    expect(stranger.port.disconnected).toBe(true);
  });

  it('passes on a pressed button only for a note it sent, and only from the proved page', async () => {
    const { notes, told } = center();
    await notes.show(note(1));
    const early = fakePort();
    notes.connect(early.port);
    early.send({ type: 'pressed', id: noteId(1), button: 0 });
    const page = provedPage(notes);
    page.send({ type: 'pressed', id: noteId(1), button: 1 });
    page.send({ type: 'pressed', id: noteId(99), button: 0 });
    page.send({ type: 'pressed', id: noteId(1), button: -1 });
    await flush();
    expect(told.pressed).toEqual([[noteId(1), 1]]);
  });

  it('takes the frame away once empty, but not when a newer note crossed the word', async () => {
    const { notes, told } = center();
    await notes.show(note(1));
    const page = provedPage(notes);
    await notes.show(note(2));
    page.send({ type: 'empty', last: noteId(1) });
    await flush();
    expect(told.unmounted).toEqual([]);
    page.send({ type: 'empty', last: noteId(2) });
    await flush();
    expect(told.unmounted).toEqual([7]);
    expect(page.port.disconnected).toBe(true);
  });

  it('places a fresh frame once the old one lost its way, as after a restart', async () => {
    const mount = vi.fn(() => Promise.resolve('token-7'));
    const { notes } = center({ mount });
    await notes.show(note(1));
    provedPage(notes).close();
    await notes.show(note(2));
    expect(mount).toHaveBeenCalledTimes(2);
  });

  it('takes away a frame that never connects, so the tab gets a working one next time', async () => {
    vi.useFakeTimers();
    const mount = vi.fn(() => Promise.resolve('token-7'));
    const { notes, told } = center({ mount });
    await notes.show(note(1));
    await vi.advanceTimersByTimeAsync(CONNECT_WAIT_MS);
    expect(told.unmounted).toEqual([7]);
    await notes.show(note(2));
    expect(mount).toHaveBeenCalledTimes(2);
  });

  it('gives a frame that connects late none of the notes the app told another way meanwhile', async () => {
    const clock = { now: 0 };
    const { notes } = center({}, clock);
    await notes.show(note(1));
    clock.now = NOTE_ANSWER_WAIT_MS;
    await notes.show(note(2));
    clock.now = NOTE_ANSWER_WAIT_MS + 1;
    const page = provedPage(notes);
    expect(page.received.map((m) => m.note.id)).toEqual([noteId(2)]);
  });

  it('ignores ports of other names', () => {
    const { notes } = center();
    const other = fakePort('wirecatch-popup');
    notes.connect(other.port);
    other.send({ type: 'hello', token: 'token-7' });
    expect(other.port.disconnected).toBe(false);
  });
});

describe('the tab a note goes to', () => {
  const tab = (url: string) => ({ id: 5, url });
  function api(
    window: Partial<Awaited<ReturnType<FocusApi['lastFocusedWindow']>>>,
    url = 'https://example.com/',
  ) {
    return {
      lastFocusedWindow: () =>
        Promise.resolve({ id: 1, focused: true, state: 'normal', incognito: false, ...window }),
      activeTab: () => Promise.resolve(tab(url)),
    } satisfies FocusApi;
  }

  it('is the active tab of the browser in front, when it shows a website', async () => {
    expect(await focusedWebTab(api({}))).toBe(5);
  });

  it('is none behind another program, minimized, in a private window, or on a page that is not a website', async () => {
    expect(await focusedWebTab(api({ focused: false }))).toBeNull();
    expect(await focusedWebTab(api({ state: 'minimized' }))).toBeNull();
    expect(await focusedWebTab(api({ incognito: true }))).toBeNull();
    expect(await focusedWebTab(api({}, 'chrome://newtab/'))).toBeNull();
    expect(await focusedWebTab(api({}, 'file:///C:/report.pdf'))).toBeNull();
  });
});

describe('the notes frame', () => {
  function frame() {
    let next = 0;
    return new NotesFrame({
      container: () => document.documentElement,
      notesUrl: (token) => `chrome-extension://id/notes.html#${token}`,
      extensionOrigin: 'chrome-extension://id',
      newToken: () => `token-${String(++next)}`,
    });
  }

  it('keeps the page’s elements the same whether a note shows or not, the frame changing only in a closed shadow root', async () => {
    const notes = frame();
    const host = document.documentElement.lastElementChild;
    expect(host?.shadowRoot).toBeNull();
    const seen: MutationRecord[] = [];
    const observer = new MutationObserver((records) => seen.push(...records));
    observer.observe(document, { subtree: true, childList: true, attributes: true });
    expect(notes.mount()).toBe('token-1');
    expect(notes.mount()).toBe('token-2');
    notes.unmount();
    await flush();
    observer.disconnect();
    expect(seen).toEqual([]);
    expect(document.querySelector('iframe')).toBeNull();
    notes.destroy();
    expect(host?.isConnected).toBe(false);
  });

  it('takes no notes once the website took its host away, rather than show it one came', () => {
    const notes = frame();
    document.documentElement.lastElementChild?.remove();
    expect(notes.mount()).toBeNull();
    notes.destroy();
  });

  it('hands its notes page alone a channel for sizes, at the extension’s origin', async () => {
    // A test can reach the closed shadow root; a website's script cannot.
    const attach = vi.spyOn(Element.prototype, 'attachShadow');
    const notes = frame();
    notes.mount();
    const shadow = attach.mock.results[0]?.value as ShadowRoot | undefined;
    const iframe = shadow?.querySelector('iframe');
    if (!iframe?.contentWindow) throw new Error('no frame in the shadow root');
    expect(iframe.src).toBe('chrome-extension://id/notes.html#token-1');
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
    sizes.postMessage({ wirecatchNotesWidth: 352.2, wirecatchNotesHeight: 180.5 });
    await resized;
    expect([iframe.style.width, iframe.style.height]).toEqual(['353px', '181px']);
    sizes.close();
    notes.destroy();
  });
});

describe('the notes page', () => {
  function page() {
    const sent: FromNotesPage[] = [];
    const notes = new NotesPage((message) => sent.push(message));
    render(Notes, { page: notes });
    return { notes, sent };
  }

  it('says a note is on screen once its frame has room for it, sends the button pressed, and the last note once none is left', async () => {
    const { notes, sent } = page();
    notes.show(note(1));
    flushSync();
    expect(screen.getByText('Download finished')).toBeTruthy();
    expect(screen.getByText('file-1.zip')).toBeTruthy();
    expect(sent).toEqual([]);
    const channel = new MessageChannel();
    notes.sizesTo(channel.port1);
    notes.resized({ wirecatchNotesWidth: 300, wirecatchNotesHeight: 90 });
    await userEvent.click(screen.getByRole('button', { name: 'Show in folder' }));
    expect(sent).toEqual([
      { type: 'shown', id: noteId(1) },
      { type: 'pressed', id: noteId(1), button: 0 },
      { type: 'empty', last: noteId(1) },
    ]);
    channel.port1.close();
    notes.dispose();
  });

  it('lets its notes go when the background lets the frame go', () => {
    const { notes } = page();
    notes.show(note(1, { stays_ms: null }));
    notes.lost();
    expect(notes.toaster.list).toEqual([]);
    notes.dispose();
  });

  it('closes one note, or all of them at once', async () => {
    const { notes, sent } = page();
    notes.show(note(1, { kind: 'started', buttons: [], stays_ms: 3000 }));
    notes.show(note(2, { kind: 'failed', title: 'Download failed', buttons: [], stays_ms: null }));
    flushSync();
    expect(screen.getAllByRole('button', { name: 'Dismiss' })).toHaveLength(2);
    await userEvent.click(screen.getByRole('button', { name: 'Clear all' }));
    expect(notes.toaster.list).toEqual([]);
    expect(sent.at(-1)).toEqual({ type: 'empty', last: noteId(2) });
    notes.dispose();
  });

  it('keeps a failure until it is closed, and lets a finished note go by itself', () => {
    vi.useFakeTimers();
    const { notes } = page();
    notes.show(note(1, { stays_ms: 8000 }));
    notes.show(note(2, { kind: 'failed', stays_ms: null }));
    vi.advanceTimersByTime(8000);
    expect(notes.toaster.list.map((t) => t.tone)).toEqual(['danger']);
    notes.dispose();
  });

  it('sends its size only over the channel it was handed, the latest at once, and none without notes', async () => {
    const notes = new NotesPage(() => undefined);
    notes.show(note(1));
    notes.resized({ wirecatchNotesWidth: 1, wirecatchNotesHeight: 2 });
    notes.resized({ wirecatchNotesWidth: 3, wirecatchNotesHeight: 4 });
    const channel = new MessageChannel();
    const first = nextMessage(channel.port2);
    notes.sizesTo(channel.port1);
    expect(await first).toEqual({ wirecatchNotesWidth: 3, wirecatchNotesHeight: 4 });
    notes.toaster.clear();
    const none = nextMessage(channel.port2);
    notes.resized({ wirecatchNotesWidth: 20, wirecatchNotesHeight: 20 });
    expect(await none).toEqual({ wirecatchNotesWidth: 0, wirecatchNotesHeight: 0 });
    channel.port1.close();
    notes.dispose();
  });
});
