// One download window (D23), against the mock backend: its question, Download now and later,
// Cancel, the media add-on for a picked quality, questions closed elsewhere, and the progress
// that follows.

import { downloadId, questionId } from '@wirecatch/protocol';
import { PRODUCT_NAME } from '@wirecatch/product';
import { describe, expect, it, vi } from 'vitest';
import { mockBackend, windowState } from '$lib/testing/setup';
import { BackendError } from '../backend';
import type { MockBackend } from '../backend/mock';
import { ANSWERED_ELSEWHERE, READING_ENDED, type Target } from './download-window.svelte';

function backend() {
  return mockBackend({ empty: true });
}

async function open(b: MockBackend, target: Target) {
  const { win, host } = windowState(b, target);
  await win.init();
  await vi.waitFor(() => expect(win.addon).not.toBeNull());
  return { win, host };
}

const keep = { fileName: null, folder: null, queue: null };
const keepAnswer = { folder: null, file_name: null, queue: null };
/** The same for a video, saved as it comes. */
const keepMedia = {
  ...keepAnswer,
  output: 'original' as const,
  audio_kbps: null,
  subtitles: [],
  subtitle_files: false,
  chapters: false,
};

describe('a browser file', () => {
  it('Download now turns the window into the download’s progress window', async () => {
    const b = backend();
    b.simulateConfirmation();
    const [asked] = await b.pendingConfirmations();
    const { win, host } = await open(b, { question: asked?.id ?? questionId(0) });
    expect(win.phase).toBe('ask');
    expect(win.question?.kind).toBe('capture');
    expect(win.earlier).toBeNull();

    expect(await win.confirm({ ...keep, fileName: 'renamed.exe' }, false)).toBe(true);
    const id = host.becameDownload.mock.calls[0]?.[0] ?? downloadId(0);
    expect(win.phase).toBe('progress');
    expect(win.row?.id).toBe(id);
    expect(win.row?.file_name).toBe('renamed.exe');
    expect(host.close).not.toHaveBeenCalled();
    // Its answer closed the question; the window is not told it went elsewhere.
    expect(win.phase).toBe('progress');

    // Progress arrives with the events, and only this row is kept.
    b.tick(1000);
    b.tick(1000);
    expect(win.row?.status).toBe('downloading');
    expect(win.row?.done).toBeGreaterThan(0);
  });

  it('Download later adds it paused and closes the window', async () => {
    const b = backend();
    b.simulateConfirmation();
    const [asked] = await b.pendingConfirmations();
    const { win, host } = await open(b, { question: asked?.id ?? questionId(0) });
    await win.confirm(keep, true);
    expect(host.close).toHaveBeenCalledTimes(1);
    const id = host.becameDownload.mock.calls[0]?.[0] ?? downloadId(0);
    expect((await b.getDownload(id)).status).toBe('paused');
  });

  it('closes when downloads start, if the person chose that', async () => {
    const b = backend();
    b.simulateConfirmation();
    const [asked] = await b.pendingConfirmations();
    const { win, host } = await open(b, { question: asked?.id ?? questionId(0) });
    await win.setCloseWhenStarted(true);
    expect((await b.getSettings()).show_progress_windows).toBe(false);
    await win.confirm(keep, false);
    expect(host.close).toHaveBeenCalledTimes(1);
    expect(win.phase).toBe('ask');
  });

  it('Start downloads without asking turns the window off for files', async () => {
    const b = backend();
    b.simulateConfirmation();
    const [asked] = await b.pendingConfirmations();
    const { win } = await open(b, { question: asked?.id ?? questionId(0) });
    await win.setStartWithoutAsking(true);
    expect((await b.getSettings()).extension.ask_before_download).toBe(false);
  });

  it('only Cancel discards; closing the window does not', async () => {
    const b = backend();
    b.simulateConfirmation();
    const [asked] = await b.pendingConfirmations();
    const { win, host } = await open(b, { question: asked?.id ?? questionId(0) });
    const reject = vi.spyOn(b, 'rejectCapture');
    win.close();
    expect(host.close).toHaveBeenCalledTimes(1);
    expect(reject).not.toHaveBeenCalled();
    expect(await b.pendingConfirmations()).toHaveLength(1);
    await win.discard();
    expect(reject).toHaveBeenCalledTimes(1);
    expect(await b.pendingConfirmations()).toHaveLength(0);
  });

  it('says when the file is already in the list', async () => {
    const b = backend();
    b.simulateConfirmation();
    const [first] = await b.pendingConfirmations();
    await b.confirmCapture(first?.id ?? questionId(0), { ...keepAnswer, later: true });
    // The mock sends four sample files in turn; the fifth is the first one again.
    for (let i = 0; i < 4; i++) b.simulateConfirmation();
    const again = (await b.pendingConfirmations()).at(-1);
    expect(again?.url).toBe(first?.url);
    const { win } = await open(b, { question: again?.id ?? questionId(0) });
    expect(win.earlier?.status).toBe('paused');
  });

  it('a question answered before its window loaded is never asked again', async () => {
    const b = backend();
    b.simulateConfirmation();
    const [asked] = await b.pendingConfirmations();
    await b.rejectCapture(asked?.id ?? questionId(0));
    const { win } = await open(b, { question: asked?.id ?? questionId(0) });
    expect(win.phase).toBe('gone');
    expect(win.goneReason).toBe(ANSWERED_ELSEWHERE);
  });

  it('says so when it is answered elsewhere or taken back', async () => {
    const b = backend();
    b.simulateConfirmation();
    const [asked] = await b.pendingConfirmations();
    const { win } = await open(b, { question: asked?.id ?? questionId(0) });
    b.simulateAnsweredElsewhere();
    expect(win.phase).toBe('gone');
    expect(win.goneReason).toBe(ANSWERED_ELSEWHERE);
  });

  it('keeps the question when the answer fails, and says why', async () => {
    const b = backend();
    b.simulateConfirmation();
    const [asked] = await b.pendingConfirmations();
    const { win } = await open(b, { question: asked?.id ?? questionId(0) });
    vi.spyOn(b, 'confirmCapture').mockRejectedValueOnce(new Error('The folder is read-only.'));
    expect(await win.confirm(keep, false)).toBe(false);
    expect(win.error).toBe('The folder is read-only.');
    expect(win.phase).toBe('ask');
    // A later close from elsewhere is news again.
    b.simulateAnsweredElsewhere();
    expect(win.phase).toBe('gone');
  });
});

describe('a video', () => {
  it('shows the reading, then the quality list, in the same window', async () => {
    const b = backend();
    vi.useFakeTimers();
    b.simulateSlowReading();
    const shown = b.windows.windows[0]?.shows;
    const id = shown?.kind === 'question' ? shown.id : questionId(0);
    const { win } = windowState(b, { question: id });
    await win.init();
    expect(win.question?.kind).toBe('reading');
    await vi.advanceTimersByTimeAsync(3000);
    expect(win.question?.kind).toBe('choice');
  });

  it('drops the note about an earlier download when it cannot be read again', async () => {
    const b = backend();
    const earlierId = await b.addDownload({
      url: 'https://example.com/a.zip',
      queue: null,
      folder: null,
      file_name: null,
      start: false,
    });
    const earlier = await b.getDownload(earlierId);
    vi.spyOn(b, 'earlierDownload')
      .mockResolvedValueOnce(earlier)
      .mockRejectedValueOnce(new Error('The list could not be read.'));
    vi.useFakeTimers();
    b.simulateSlowReading();
    const shown = b.windows.windows[0]?.shows;
    const id = shown?.kind === 'question' ? shown.id : questionId(0);
    const { win } = windowState(b, { question: id });
    await win.init();
    expect(win.earlier?.id).toBe(earlierId);
    await vi.advanceTimersByTimeAsync(3000);
    expect(win.question?.kind).toBe('choice');
    // A note that could not be checked again is not shown as if it still held.
    expect(win.earlier).toBeNull();
  });

  it('a reading that ends without a list says why it went', async () => {
    const b = backend();
    vi.useFakeTimers();
    b.simulateSlowReading();
    const shown = b.windows.windows[0]?.shows;
    const id = shown?.kind === 'question' ? shown.id : questionId(0);
    const { win } = windowState(b, { question: id });
    await win.init();
    win.handle({ type: 'confirmation_closed', data: { id } });
    expect(win.phase).toBe('gone');
    expect(win.goneReason).toBe(READING_ENDED);
  });

  it('installs the add-on for a quality that needs it, then downloads by itself', async () => {
    const b = backend();
    b.simulateMediaChoice();
    const [choice] = await b.pendingMediaChoices();
    const { win, host } = await open(b, { question: choice?.id ?? questionId(0) });
    const best = choice?.options.find((o) => o.needs_ffmpeg);
    expect(await win.choose({ ...keepMedia, option_id: best?.id ?? '', later: false })).toBe(true);
    expect(win.awaitingAddon).not.toBeNull();
    expect(win.addon?.state.type).toBe('installing');
    expect(host.becameDownload).not.toHaveBeenCalled();
    b.tick(4500);
    await vi.waitFor(() => expect(win.phase).toBe('progress'));
    expect(win.awaitingAddon).toBeNull();
    expect(win.row?.media?.quality).toBe('2160p');
  });

  it('waits for the add-on when the core asks for it before the add-on is listed', async () => {
    const b = backend();
    b.simulateMediaChoice();
    const [choice] = await b.pendingMediaChoices();
    vi.spyOn(b, 'listAddons').mockRejectedValue(new Error('Not listed yet.'));
    vi.spyOn(b, 'chooseMedia').mockRejectedValue(
      new BackendError('This needs the media add-on to join video and audio.', 'addon_required'),
    );
    const { win } = windowState(b, { question: choice?.id ?? questionId(0) });
    await win.init();
    expect(win.addon).toBeNull();
    const best = choice?.options.find((o) => o.needs_ffmpeg);
    expect(await win.choose({ ...keepMedia, option_id: best?.id ?? '', later: false })).toBe(true);
    expect(win.awaitingAddon).not.toBeNull();
    expect(win.error).toBeNull();
  });

  it('gives the quality list back when the add-on is not wanted or its install stops', async () => {
    const b = backend();
    b.simulateMediaChoice();
    const [choice] = await b.pendingMediaChoices();
    const { win } = await open(b, { question: choice?.id ?? questionId(0) });
    await win.choose({ ...keepMedia, option_id: '1080', later: false });
    await win.stopAddonInstall();
    expect(win.awaitingAddon).toBeNull();
    await win.choose({ ...keepMedia, option_id: '1080', later: false });
    await win.declineAddon();
    expect(win.awaitingAddon).toBeNull();
    expect(win.phase).toBe('ask');
    // Nothing was answered.
    expect(await b.pendingMediaChoices()).toHaveLength(1);
  });

  it('a page waiting for the add-on goes on to its quality list once installed', async () => {
    const b = backend();
    b.simulateAddonRequired();
    const [page] = await b.pendingAddonRequests();
    const { win } = await open(b, { question: page?.id ?? questionId(0) });
    expect(win.question?.kind).toBe('addon');
    await win.installAddon();
    b.tick(4500);
    await vi.waitFor(() => expect(win.question?.kind).toBe('choice'));
  });

  it('Download later adds the picked quality paused', async () => {
    const b = backend();
    b.simulateMediaChoice();
    const [choice] = await b.pendingMediaChoices();
    const { win, host } = await open(b, { question: choice?.id ?? questionId(0) });
    await win.choose({ ...keepMedia, option_id: '720', later: true });
    expect(host.close).toHaveBeenCalledTimes(1);
    const id = host.becameDownload.mock.calls[0]?.[0] ?? downloadId(0);
    expect((await b.getDownload(id)).status).toBe('paused');
  });
});

describe('a progress window', () => {
  async function running(b: MockBackend) {
    const id = await b.addDownload({
      url: 'https://example.com/big.iso',
      queue: null,
      folder: null,
      file_name: null,
      start: true,
    });
    return open(b, { download: id });
  }

  it('keeps only its own row from each batch', async () => {
    const b = backend();
    const { win } = await running(b);
    const other = { ...win.row!, id: downloadId(999), done: 12345 };
    const mine = { ...win.row!, done: 42 };
    win.handle({ type: 'downloads_changed', data: [other] });
    expect(win.row?.done).not.toBe(12345);
    win.handle({ type: 'downloads_changed', data: [other, mine] });
    expect(win.row?.done).toBe(42);
  });

  it('pauses, resumes, and Cancel removes it with its partial file', async () => {
    const b = backend();
    const { win, host } = await running(b);
    b.tick(500);
    await win.progress.pause();
    expect(win.row?.status).toBe('paused');
    await win.progress.resume();
    expect(win.row?.status).toBe('queued');
    const remove = vi.spyOn(b, 'remove');
    await win.progress.cancelDownload();
    expect(remove).toHaveBeenCalledWith([win.downloadId], true);
    expect(host.close).toHaveBeenCalledTimes(1);
  });

  it('says so when its download leaves the list', async () => {
    const b = backend();
    const { win } = await running(b);
    await b.remove([win.downloadId ?? downloadId(0)], false);
    expect(win.phase).toBe('gone');
  });
});

describe('closing a window', () => {
  it('lets go of its first listener when loaded again', async () => {
    const b = backend();
    const first = vi.fn();
    const second = vi.fn();
    vi.spyOn(b, 'subscribe').mockResolvedValueOnce(first).mockResolvedValueOnce(second);
    const { win } = windowState(b, { question: questionId(1) });
    await win.init();
    await win.init();
    expect(first).toHaveBeenCalledOnce();
    expect(second).not.toHaveBeenCalled();
  });

  it('lets go of its listener when closed while it still loads', async () => {
    const b = backend();
    const stop = vi.fn();
    vi.spyOn(b, 'subscribe').mockResolvedValue(stop);
    const { win } = windowState(b, { question: questionId(1) });
    const loading = win.init();
    win.dispose();
    await loading;
    expect(stop).toHaveBeenCalledOnce();
  });
});

describe('the window title', () => {
  it('shows the question as usual when the shell will not say its name', async () => {
    const b = backend();
    b.simulateSlowReading();
    vi.spyOn(b, 'appName').mockRejectedValue(new Error('core:app:allow-name not allowed'));
    const shown = b.windows.windows[0]?.shows;
    const id = shown?.kind === 'question' ? shown.id : questionId(0);
    const { win } = windowState(b, { question: id });
    await win.init();
    expect(win.phase).not.toBe('gone');
    expect(win.question?.kind).toBe('reading');
    expect(win.name).toBe(PRODUCT_NAME);
  });

  it('names the running app, as the main window does', async () => {
    const b = backend();
    b.simulateSlowReading();
    vi.spyOn(b, 'appName').mockResolvedValue('Wirecatch Dev');
    const shown = b.windows.windows[0]?.shows;
    const id = shown?.kind === 'question' ? shown.id : questionId(0);
    const { win } = windowState(b, { question: id });
    await win.init();
    expect(win.name).toBe('Wirecatch Dev');
  });
});
