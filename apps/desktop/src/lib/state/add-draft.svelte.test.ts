// The Add dialog's form in state: what it reads as links, what it sends, what it keeps when the app refuses a link, and links that arrive while it is open (DUI-8).

import { queueId } from '@wirecatch/protocol';
import { describe, expect, it, vi } from 'vitest';
import { appState, mockBackend } from '$lib/testing/setup';
import type { ShellSignal } from '../backend';
import { linksHint, NO_LINK, readLinks } from './add-draft.svelte';

async function make() {
  const backend = mockBackend({ empty: true });
  let signal: ((s: ShellSignal) => void) | undefined;
  vi.spyOn(backend, 'onShellSignal').mockImplementation((listener) => {
    signal = listener;
    return Promise.resolve(vi.fn());
  });
  const app = appState(backend);
  await app.init();
  return { app, backend, signal: (s: ShellSignal) => signal?.(s) };
}

/** A promise settled from outside, to hold a send in flight. */
function deferred() {
  let resolve!: () => void;
  const promise = new Promise<void>((res) => {
    resolve = res;
  });
  return { promise, resolve };
}

function draftOf(app: ReturnType<typeof appState>) {
  const draft = app.addDraft;
  if (!draft) throw new Error('the Add dialog is not open');
  return draft;
}

describe('reading the links box', () => {
  it('takes every link, once, and counts the lines that hold none', () => {
    expect(readLinks('')).toEqual({ urls: [], ignored: 0 });
    expect(
      readLinks('https://a.org/x.zip\n\n  see https://a.org/y.zip  \nnotes\nhttps://a.org/x.zip'),
    ).toEqual({ urls: ['https://a.org/x.zip', 'https://a.org/y.zip'], ignored: 1 });
    expect(readLinks('https://\nwww.example.com/a.zip')).toEqual({ urls: [], ignored: 2 });
  });

  it('says what it leaves out, and how to send the rest', () => {
    expect(linksHint({ urls: [], ignored: 0 })).toBe(
      'One link per line, or paste several at once.',
    );
    expect(linksHint({ urls: ['https://a.org/x'], ignored: 1 })).toBe(
      'One line has no link in it and is left out.',
    );
    expect(linksHint({ urls: ['https://a.org/x'], ignored: 3 })).toBe(
      '3 lines have no link in them and are left out.',
    );
  });
});

describe('the Add dialog in state', () => {
  it('starts from the links it was opened with, in the queue on screen', async () => {
    const { app } = await make();
    app.show({ kind: 'queue', id: queueId(2) });
    app.openDialog({ kind: 'add', urls: ['https://a.org/x.zip', 'https://a.org/y.zip'] });
    const draft = draftOf(app);
    expect(draft.text).toBe('https://a.org/x.zip\nhttps://a.org/y.zip');
    expect(draft.queue).toBe(queueId(2));
    expect(draft.fromPaste).toBe(true);
    app.closeDialog();
    expect(app.addDraft).toBeNull();
  });

  it('keeps what was typed when links from a second launch arrive (DUI-8)', async () => {
    const { app, backend, signal } = await make();
    app.openDialog({ kind: 'add', urls: [] });
    const draft = draftOf(app);
    draft.text = 'https://typed.example/one.zip';
    draft.fileName = 'mine.zip';
    vi.spyOn(backend, 'takeLaunchLinks').mockResolvedValueOnce([
      'https://typed.example/one.zip',
      'https://launched.example/two.zip',
    ]);
    signal('links');
    await vi.waitFor(() =>
      expect(draft.links.urls).toEqual([
        'https://typed.example/one.zip',
        'https://launched.example/two.zip',
      ]),
    );
    // The same form, still open, with what was typed in it.
    expect(app.addDraft).toBe(draft);
    expect(draft.fileName).toBe('mine.zip');
    expect(draft.text).toBe('https://typed.example/one.zip\nhttps://launched.example/two.zip');
  });

  it('asks for a link rather than sending nothing', async () => {
    const { app, backend } = await make();
    const add = vi.spyOn(backend, 'addDownload');
    app.openDialog({ kind: 'add', urls: [] });
    const draft = draftOf(app);
    draft.text = 'not a link';
    expect(await draft.submit(true)).toBe(false);
    expect(draft.invalid).toBe(NO_LINK);
    expect(add).not.toHaveBeenCalled();
    expect(app.dialog?.kind).toBe('add');
    // Typing takes the complaint away.
    draft.edit('https://a.org/x.zip');
    expect(draft.invalid).toBeNull();
  });

  it('sends each link with the folder, the queue and, for one link, the name; then closes', async () => {
    const { app, backend } = await make();
    const add = vi.spyOn(backend, 'addDownload');
    app.openDialog({ kind: 'add', urls: ['https://a.org/x.zip'] });
    const draft = draftOf(app);
    draft.fileName = '  report.zip ';
    draft.folder = 'D:\\Saved';
    draft.queue = queueId(2);
    expect(await draft.submit(false)).toBe(true);
    expect(add).toHaveBeenCalledWith({
      url: 'https://a.org/x.zip',
      queue: queueId(2),
      folder: 'D:\\Saved',
      file_name: 'report.zip',
      start: false,
    });
    expect(app.dialog).toBeNull();
    const [id] = app.selection.list;
    expect(id === undefined ? null : app.downloads.get(id)?.status).toBe('paused');
  });

  it('gives several links no name of their own', async () => {
    const { app, backend } = await make();
    const add = vi.spyOn(backend, 'addDownload');
    app.openDialog({ kind: 'add', urls: ['https://a.org/x.zip', 'https://a.org/y.zip'] });
    const draft = draftOf(app);
    draft.fileName = 'ignored.zip';
    await draft.submit(true);
    expect(add.mock.calls.map(([r]) => r.file_name)).toEqual([null, null]);
    expect(add.mock.calls.map(([r]) => r.url)).toEqual([
      'https://a.org/x.zip',
      'https://a.org/y.zip',
    ]);
  });

  it('keeps the links the app refused, says why, and stays open', async () => {
    const { app, backend } = await make();
    const real = backend.addDownload.bind(backend);
    const add = vi
      .spyOn(backend, 'addDownload')
      .mockImplementation((r) =>
        r.url.includes('bad') ? Promise.reject(new Error('The server said no.')) : real(r),
      );
    app.openDialog({
      kind: 'add',
      urls: ['https://a.org/x.zip', 'https://bad.org/y.zip', 'https://a.org/z.zip'],
    });
    const draft = draftOf(app);
    expect(await draft.submit(true)).toBe(false);
    expect(draft.text).toBe('https://bad.org/y.zip');
    expect(draft.problem).toEqual({
      title: 'Could not add 1 of 3 links',
      message: 'The server said no. The link stays here to try again.',
    });
    expect(app.dialog?.kind).toBe('add');
    // What was added is in the list, and selected.
    expect(app.selection.list).toHaveLength(1);
    // Sending again starts afresh.
    add.mockImplementation(real);
    expect(await draft.submit(true)).toBe(true);
    expect(app.dialog).toBeNull();
  });

  it('says a single refusal as the app said it', async () => {
    const { app, backend } = await make();
    vi.spyOn(backend, 'addDownload').mockRejectedValue(new Error('The server said no.'));
    app.openDialog({ kind: 'add', urls: ['https://bad.org/y.zip'] });
    const draft = draftOf(app);
    await draft.submit(true);
    expect(draft.problem).toEqual({
      title: 'Could not add the link',
      message: 'The server said no.',
    });
  });

  it('sends once, however often it is asked while sending', async () => {
    const { app, backend } = await make();
    const add = vi.spyOn(backend, 'addDownload');
    app.openDialog({ kind: 'add', urls: ['https://a.org/x.zip'] });
    const draft = draftOf(app);
    await Promise.all([draft.submit(true), draft.submit(true)]);
    expect(add).toHaveBeenCalledTimes(1);
  });

  it('picks a folder, goes back to the default, and says when the picker fails', async () => {
    const { app, backend } = await make();
    app.openDialog({ kind: 'add', urls: [] });
    const draft = draftOf(app);
    vi.spyOn(backend, 'pickFolder').mockResolvedValueOnce('E:\\Media');
    await draft.chooseFolder();
    expect(draft.folder).toBe('E:\\Media');
    draft.useDefaultFolder();
    expect(draft.folder).toBeNull();
    vi.spyOn(backend, 'pickFolder').mockRejectedValueOnce(new Error('No picker here.'));
    await draft.chooseFolder();
    expect(draft.folder).toBeNull();
    expect(draft.problem).toEqual({
      title: 'Could not open the folder picker',
      message: 'No picker here.',
    });
    // Nothing went to the notes behind the dialog.
    expect(app.toasts.items).toEqual([]);
    // A folder picked afterwards takes the failure away.
    vi.spyOn(backend, 'pickFolder').mockResolvedValueOnce('E:\\Media');
    await draft.chooseFolder();
    expect(draft.folder).toBe('E:\\Media');
    expect(draft.problem).toBeNull();
  });
});

describe('what changes while the form sends', () => {
  it('offers links from a second launch that arrive while it sends', async () => {
    const { app, backend, signal } = await make();
    const real = backend.addDownload.bind(backend);
    const gate = deferred();
    const add = vi.spyOn(backend, 'addDownload').mockImplementation(async (r) => {
      await gate.promise;
      return real(r);
    });
    app.openDialog({ kind: 'add', urls: ['https://a.org/x.zip'] });
    const draft = draftOf(app);
    const sent = draft.submit(true);
    vi.spyOn(backend, 'takeLaunchLinks').mockResolvedValueOnce([
      'https://launched.example/two.zip',
    ]);
    signal('links');
    await vi.waitFor(() => {
      expect(draft.text).toContain('launched.example');
    });
    gate.resolve();
    expect(await sent).toBe(false);
    expect(add.mock.calls.map(([r]) => r.url)).toEqual(['https://a.org/x.zip']);
    // The form stays open with the link that came later, ready to send; nothing went wrong.
    expect(app.addDraft).toBe(draft);
    expect(draft.text).toBe('https://launched.example/two.zip');
    expect(draft.problem).toBeNull();
  });

  it('keeps what was typed while it sends, after the links the app refused', async () => {
    const { app, backend } = await make();
    const gate = deferred();
    vi.spyOn(backend, 'addDownload').mockImplementation(async () => {
      await gate.promise;
      throw new Error('The server said no.');
    });
    app.openDialog({ kind: 'add', urls: ['https://bad.org/y.zip'] });
    const draft = draftOf(app);
    const sent = draft.submit(true);
    draft.edit(`${draft.text}\nhttps://typed.example/later.zip`);
    gate.resolve();
    await sent;
    expect(draft.links.urls).toEqual(['https://bad.org/y.zip', 'https://typed.example/later.zip']);
    expect(draft.problem?.title).toBe('Could not add the link');
  });

  it('closes only its own dialog when it finishes after that one was closed', async () => {
    const { app, backend } = await make();
    const real = backend.addDownload.bind(backend);
    const gate = deferred();
    vi.spyOn(backend, 'addDownload').mockImplementation(async (r) => {
      await gate.promise;
      return real(r);
    });
    app.openDialog({ kind: 'add', urls: ['https://a.org/x.zip'] });
    const sent = draftOf(app).submit(true);
    // Escape while it sends, then a fresh form.
    app.closeDialog();
    app.openDialog({ kind: 'add', urls: [] });
    const next = draftOf(app);
    next.edit('https://typed.example/new.zip');
    gate.resolve();
    await sent;
    expect(app.addDraft).toBe(next);
    expect(app.dialog?.kind).toBe('add');
    expect(next.text).toBe('https://typed.example/new.zip');
  });
});
