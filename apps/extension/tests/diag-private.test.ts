import { beforeEach, describe, expect, it, vi } from 'vitest';
import { fakeBrowser } from 'wxt/testing/fake-browser';
import { installDiagnostics } from '../src/lib/diag/install';
import { Diagnostics, type DiagEntry } from '../src/lib/diag/log';
import { DIAG_KEY_PREFIX, type ErrorEvents } from '../src/lib/diag/persist';
import { flush } from './fakes';

const KEY = `${DIAG_KEY_PREFIX}background`;
/** A global scope whose errors are not the test's concern. */
const QUIET: ErrorEvents = { addEventListener: () => undefined };

/** An entry as an earlier run of the background saved it. */
function saved(at: number, message: string, isPrivate: boolean): DiagEntry {
  return {
    at,
    level: 'warn',
    scope: 'hub',
    message,
    detail: null,
    repeated: 0,
    private: isPrivate,
  };
}

/** The messages the background's saved entries hold now. */
async function savedMessages(): Promise<string[]> {
  const items = await fakeBrowser.storage.session.get(KEY);
  const entries: unknown = items[KEY];
  return Array.isArray(entries) ? entries.map((entry: DiagEntry) => entry.message) : [];
}

async function openPrivateWindow(): Promise<number> {
  const id = (await fakeBrowser.windows.create({ incognito: true }))?.id;
  if (id === undefined) throw new Error('the fake browser gave the window no id');
  return id;
}

describe('diagnostics and private windows, in the background', () => {
  beforeEach(() => {
    fakeBrowser.reset();
  });

  it('does not save again what an earlier run logged while a private window was open', async () => {
    await fakeBrowser.storage.session.set({
      [KEY]: [saved(1, 'before', false), saved(2, 'private', true)],
    });
    const diag = new Diagnostics({ now: () => 3 });
    installDiagnostics('background', QUIET, diag);
    diag.log('warn', 'hub', 'after');
    await vi.waitFor(async () => {
      expect(await savedMessages()).toContain('after');
    });
    expect(await savedMessages()).toEqual(['before', 'after']);
    expect(diag.entries().map((e) => e.message)).toEqual(['before', 'after']);
  });

  it('treats what it logs before it knows of the windows as private, while one is open', async () => {
    const window = await openPrivateWindow();
    const diag = new Diagnostics();
    installDiagnostics('background', QUIET, diag);
    diag.log('warn', 'hub', 'at start');
    expect(diag.entries()).toEqual([expect.objectContaining({ private: true })]);
    await fakeBrowser.windows.remove(window);
    await vi.waitFor(() => {
      expect(diag.entries()).toEqual([]);
    });
  });

  it('keeps what it logged before it knew of the windows, when none of them is private', async () => {
    const diag = new Diagnostics();
    installDiagnostics('background', QUIET, diag);
    diag.log('warn', 'hub', 'at start');
    await vi.waitFor(async () => {
      expect(await savedMessages()).toEqual(['at start']);
    });
    expect(diag.entries()).toEqual([expect.objectContaining({ private: false })]);
  });

  it('forgets every private entry, saved ones included, when the last private window closes', async () => {
    const window = await openPrivateWindow();
    const diag = new Diagnostics();
    installDiagnostics('background', QUIET, diag);
    diag.log('warn', 'hub', 'private');
    await vi.waitFor(async () => {
      expect(await savedMessages()).toEqual(['private']);
    });
    await fakeBrowser.windows.remove(window);
    await vi.waitFor(() => {
      expect(diag.entries()).toEqual([]);
    });
    diag.log('warn', 'hub', 'after');
    await vi.waitFor(async () => {
      expect(await savedMessages()).toEqual(['after']);
    });
    expect(diag.entries().map((e) => e.message)).toEqual(['after']);
  });
});

describe('diagnostics and private windows, in the popup', () => {
  beforeEach(() => {
    fakeBrowser.reset();
  });

  /** Opens the window the popup will belong to, and makes it the current one. */
  async function focusWindow(incognito: boolean): Promise<void> {
    await fakeBrowser.windows.create({ incognito, focused: true });
  }

  it('marks its entries private when its window is private, though its process is not', async () => {
    // Chromium's default "spanning" mode: the popup runs in the regular profile.
    expect(fakeBrowser.extension.inIncognitoContext).toBe(false);
    await focusWindow(true);
    const getCurrent = vi.spyOn(fakeBrowser.windows, 'getCurrent');
    const diag = new Diagnostics();
    installDiagnostics('popup', QUIET, diag);
    diag.log('warn', 'popup', 'at start');
    await vi.waitFor(() => {
      expect(getCurrent).toHaveBeenCalled();
    });
    await getCurrent.mock.results[0]?.value;
    await flush();
    diag.log('warn', 'popup', 'later');
    expect(diag.entries().map((e) => e.private)).toEqual([true, true]);
  });

  it('keeps its entries when its window is not private', async () => {
    await focusWindow(false);
    const diag = new Diagnostics();
    installDiagnostics('popup', QUIET, diag);
    diag.log('warn', 'popup', 'at start');
    await vi.waitFor(() => {
      expect(diag.entries()).toEqual([expect.objectContaining({ private: false })]);
    });
  });

  it('takes its window as private when the browser cannot say', async () => {
    vi.spyOn(fakeBrowser.windows, 'getCurrent').mockRejectedValue(new Error('no window'));
    const diag = new Diagnostics();
    installDiagnostics('popup', QUIET, diag);
    await vi.waitFor(() => {
      expect(diag.entries().map((e) => e.message)).toContain(
        'Could not tell whether this window is private',
      );
    });
    diag.log('warn', 'popup', 'later');
    expect(diag.entries().every((e) => e.private)).toBe(true);
  });
});
