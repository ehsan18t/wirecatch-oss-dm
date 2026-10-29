import {
  downloadId,
  GLOBAL,
  ownSetting,
  questionId,
  queueId,
  type QueueSettings,
  newQueueSettings,
} from '@wirecatch/protocol';
import { PRODUCT_NAME } from '@wirecatch/product';
import { describe, expect, it, vi } from 'vitest';
import { PATIENCE_MS } from '@wirecatch/ui/testing/jsdom';
import { appState, mockBackend } from '$lib/testing/setup';
import { BackendError } from '../backend';
import { buildCommands, fuzzyScore, searchCommands } from '../commands';
import { revealDownloads } from './reveal';
import { QUEUE_ORDER } from './view';

async function make() {
  const backend = mockBackend();
  const app = appState(backend);
  await app.init();
  return { app, backend };
}

describe('AppState', () => {
  it('loads as usual when the shell will not say its name, and shows the product name', async () => {
    const backend = mockBackend();
    vi.spyOn(backend, 'appName').mockRejectedValue(new Error('core:app:allow-name not allowed'));
    const app = appState(backend);
    await app.init();
    expect(app.ready).toBe(true);
    expect(app.loadError).toBeNull();
    expect(app.name).toBe(PRODUCT_NAME);
  });

  it('names itself as the running app does, so a debug build says so', async () => {
    const backend = mockBackend();
    vi.spyOn(backend, 'appName').mockResolvedValue('Wirecatch Dev');
    const app = appState(backend);
    await app.init();
    expect(app.name).toBe('Wirecatch Dev');
  });

  it('says what an idle queue may run in its phrase, so nothing reads the phrase back', async () => {
    const backend = mockBackend({ empty: true });
    const app = appState(backend);
    await app.init();
    const queue = app.queues.defaultQueue;
    if (!queue) throw new Error('The sample has no default queue.');
    const atOnce = (max_concurrent: QueueSettings['max_concurrent']) =>
      app.queues.describe({ ...queue, settings: { ...queue.settings, max_concurrent } });
    expect(atOnce(GLOBAL)).toBe('Idle');
    expect(atOnce(ownSetting(2))).toBe('Idle, up to 2 at once');
  });

  it('makes a new queue with its settings in one step, and a refused one makes nothing', async () => {
    const { app, backend } = await make();
    const settings = { ...newQueueSettings('Night'), max_concurrent: ownSetting(2) };
    const named = () => app.queues.list.filter((q) => q.settings.name === 'Night');
    vi.spyOn(backend, 'createQueue').mockRejectedValueOnce(new BackendError('Not now.'));

    expect(await app.queues.save(null, settings)).toBeUndefined();
    expect(named()).toHaveLength(0);

    const id = await app.queues.save(null, { ...settings, max_concurrent: ownSetting(3) });
    expect(named().map((q) => [q.id, q.settings.max_concurrent])).toEqual([[id, ownSetting(3)]]);
  });

  it('loads everything and applies events', async () => {
    const { app, backend } = await make();
    expect(app.ready).toBe(true);
    expect(app.queues.list).toHaveLength(3);
    expect(app.downloads.size).toBe(app.counts.all);
    await app.setAskBeforeDownload(false);
    const before = app.downloads.size;
    backend.simulateCapture();
    expect(app.downloads.size).toBe(before + 1);
  });

  it('shows a readable error when the backend is not there', async () => {
    const backend = mockBackend();
    vi.spyOn(backend, 'listDownloads').mockRejectedValue(
      new BackendError('Wirecatch is not connected to its download engine yet.'),
    );
    const app = appState(backend);
    await app.init();
    expect(app.loadError).toMatch(/not connected/);
  });

  it('stops listening to events when the shell signals cannot be heard', async () => {
    const backend = mockBackend();
    const stopEvents = vi.fn();
    vi.spyOn(backend, 'subscribe').mockResolvedValue(stopEvents);
    vi.spyOn(backend, 'onShellSignal').mockRejectedValue(new BackendError('No shell.'));
    const app = appState(backend);
    await app.init();
    expect(app.loadError).toBe('No shell.');
    expect(stopEvents).toHaveBeenCalledOnce();
  });

  it('shows the downloads a clicked notification was about, selected, in their queue', async () => {
    const { app } = await make();
    const queue = app.queues.list.find((q) => app.downloads.idsInQueue(q.id).length > 1);
    if (!queue) throw new Error('The sample has no queue with two downloads.');
    const ids = app.downloads.idsInQueue(queue.id).slice(0, 2);
    const unknown = downloadId(999_999);
    revealDownloads(app, { queue: queue.id, ids: [unknown, ...ids] });
    expect(app.view).toEqual({ kind: 'queue', id: queue.id });
    expect(app.selection.list).toEqual(ids);
    expect(app.selection.active).toBe(ids[0]);

    revealDownloads(app, { queue: null, ids: [ids[1]!] });
    expect(app.view).toEqual({ kind: 'all' });
    expect(app.selection.list).toEqual([ids[1]]);
    // Downloads that are all gone change nothing.
    revealDownloads(app, { queue: null, ids: [unknown] });
    expect(app.selection.list).toEqual([ids[1]]);
  });

  it('opens a download’s details for a progress window’s Change queue, and the row’s fix does too', async () => {
    const { app } = await make();
    const [first, second] = app.downloads.idsInQueue(app.queues.list[0]!.id);
    revealDownloads(app, { queue: null, ids: [first!], details: true });
    expect(app.details).toBe(first);
    app.closeDetails();
    await app.actions.fix(second!, 'change_queue');
    expect(app.details).toBe(second);
  });

  it('selects the downloads of a notification clicked while the page was loading, once the list is in', async () => {
    const backend = mockBackend();
    const [first] = await backend.listDownloads();
    if (!first) throw new Error('The sample has no downloads.');
    vi.spyOn(backend, 'takeReveal').mockResolvedValue({ queue: null, ids: [first.id] });
    const app = appState(backend);
    await app.init();
    await vi.waitFor(() => expect(app.selection.list).toEqual([first.id]), PATIENCE_MS);
  });

  it('opens the browser settings the extension asked for, even while the page was loading (D61)', async () => {
    const backend = mockBackend();
    vi.spyOn(backend, 'takeReveal').mockResolvedValue({
      queue: null,
      ids: [],
      browser_settings: true,
    });
    const app = appState(backend);
    await app.init();
    await vi.waitFor(() => expect(app.screen).toBe('settings'), PATIENCE_MS);
    expect(app.settingsSection).toBe('browser');
  });

  it('selects ranges in display order', async () => {
    const { app } = await make();
    const [a, b, c] = app.visible;
    app.selection.only(a!);
    app.selection.range(c!);
    expect(app.selection.list).toEqual([a, b, c]);
    app.selection.toggle(b!);
    expect(app.selection.has(b!)).toBe(false);
    app.selection.move(1, true);
    expect(app.selection.active).toBe(c);
  });

  it('places downloads inside their queue and tells the backend the new order', async () => {
    const { app, backend } = await make();
    const spy = vi.spyOn(backend, 'reorder');
    const q = app.downloads.idsInQueue(queueId(1));
    await app.actions.placeInQueue(queueId(1), [q[3]!], q[0]!);
    const expected = [q[3], q[0], q[1], q[2], ...q.slice(4)];
    expect(app.downloads.idsInQueue(queueId(1))).toEqual(expected);
    expect(spy).toHaveBeenCalledWith(1, expected);
    // Dropping at the end.
    await app.actions.placeInQueue(queueId(1), [q[3]!], null);
    expect(app.downloads.idsInQueue(queueId(1)).at(-1)).toBe(q[3]);
  });

  it('nudges only in queue order, and only within one queue', async () => {
    const { app } = await make();
    const q = app.downloads.idsInQueue(queueId(1));
    app.selection.only(q[1]!);
    await app.actions.nudge(-1);
    expect(app.downloads.idsInQueue(queueId(1))[0]).toBe(q[1]);
    app.sort = { column: 'name', descending: false };
    await app.actions.nudge(1);
    expect(app.toasts.items.at(-1)?.message).toMatch(/Sort the list by #/);
    app.sort = QUEUE_ORDER;
    const other = app.downloads.idsInQueue(queueId(2))[0]!;
    app.selection.toggle(other);
    await app.actions.nudge(1);
    expect(app.toasts.items.at(-1)?.message).toMatch(/one queue/);
  });

  it('nudges a download only among the downloads of its own group', async () => {
    const { app } = await make();
    const line = app.downloads.idsInQueue(queueId(1));
    const firstWaiting = line.find((id) => app.downloads.get(id)?.status === 'queued')!;
    const ahead = line.slice(0, line.indexOf(firstWaiting));
    expect(ahead.length).toBeGreaterThan(0);
    app.selection.only(firstWaiting);
    await app.actions.nudge(-1);
    expect(app.downloads.idsInQueue(queueId(1))).toEqual(line);
  });

  it('closes the details of a download that is removed', async () => {
    const { app, backend } = await make();
    const id = app.visible[0]!;
    app.openDetails(id);
    expect(app.detailsOpen).toBe(true);
    await backend.remove([id], false);
    await vi.waitFor(() => expect(app.downloads.get(id)).toBeUndefined(), PATIENCE_MS);
    expect(app.details).toBeNull();
  });

  it('turns pasted text into the Add dialog, or explains why not', async () => {
    const { app } = await make();
    expect(app.pasteText('hello')).toBe(false);
    expect(app.toasts.items.at(-1)?.kind).toBe('error');
    expect(app.pasteText('https://a.org/x.zip\nhttps://a.org/y.zip')).toBe(true);
    expect(app.dialog).toEqual({
      kind: 'add',
      urls: ['https://a.org/x.zip', 'https://a.org/y.zip'],
    });
  });

  it('saves settings right away and puts them back if saving fails', async () => {
    const { app, backend } = await make();
    await app.updateSettings((d) => void (d.theme = 'dark'));
    expect(app.settings?.theme).toBe('dark');
    expect((await backend.getSettings()).theme).toBe('dark');
    vi.spyOn(backend, 'changeSettings').mockRejectedValueOnce(
      new BackendError('Disk is read only.'),
    );
    await app.updateSettings((d) => void (d.theme = 'light'));
    expect(app.settings?.theme).toBe('dark');
    expect(app.toasts.items.at(-1)?.message).toBe('Disk is read only.');
  });

  it('runs fixes: choose a folder then retry', async () => {
    const { app } = await make();
    const id = app.visible.find((x) => app.downloads.get(x)?.error?.fix === 'choose_folder')!;
    await app.actions.fix(id, 'choose_folder');
    expect(app.downloads.get(id)?.status).toBe('queued');
    expect(app.downloads.get(id)?.path).not.toContain(String.raw`E:\Backups`);
  });

  it('cancels a download, then starts it over from zero', async () => {
    const { app } = await make();
    const id = app.visible.find((x) => app.downloads.get(x)?.status === 'paused')!;
    expect(app.downloads.get(id)?.done).toBeGreaterThan(0);
    await app.actions.cancel([id]);
    expect(app.downloads.get(id)?.status).toBe('cancelled');
    expect(app.downloads.get(id)?.done).toBe(0);
    expect(app.counts.status.cancelled).toBe(2);
    // Space on a cancelled download starts it over.
    await app.actions.togglePause([id]);
    expect(app.downloads.get(id)?.status).toBe('queued');
  });

  it('counts browser captures until their windows answer them', async () => {
    const { app, backend } = await make();
    backend.simulateConfirmation();
    backend.simulateConfirmation();
    expect(app.questions.pending).toHaveLength(2);
    expect(app.questions.waitingCount).toBe(2);
    await backend.rejectCapture(app.questions.pending[0]?.id ?? questionId(0));
    await backend.confirmCapture(app.questions.pending[0]?.id ?? questionId(0), {
      folder: null,
      file_name: null,
      queue: queueId(3),
      later: false,
    });
    expect(app.questions.pending).toHaveLength(0);
    expect(app.questions.waitingCount).toBe(0);
  });

  it('exports and imports settings', async () => {
    const { app } = await make();
    expect(await app.transfer.exportSettings()).toMatch(/wirecatch-settings\.json$/);
    await app.updateSettings((d) => void (d.connections_per_download = 2));
    // Nothing is imported without a preview first.
    expect(await app.transfer.importSettings()).toBeNull();
    expect(app.settings?.connections_per_download).toBe(2);
    const preview = await app.transfer.previewImport();
    expect(preview?.folders[0]?.setting).toBe('Default folder');
    // The preview changes nothing.
    expect(app.settings?.connections_per_download).toBe(2);
    expect(await app.transfer.importSettings()).toMatch(/wirecatch-settings\.json$/);
    expect(app.transfer.importPreview).toBeNull();
    expect(app.settings?.connections_per_download).toBe(8);
    // What changed, as the core reports it, for the import dialog to show.
    expect(app.transfer.lastImport?.queues_updated).toBeGreaterThan(0);
    expect(app.transfer.lastImport?.folders[0]?.setting).toBe('Default folder');
    expect(app.transfer.lastImport?.addons_to_offer).toEqual([]);
  });

  it('offers the add-ons an imported profile lists, and installs nothing by itself', async () => {
    const { app, backend } = await make();
    await app.addons.install('media');
    backend.tick(4500);
    expect(await app.transfer.exportSettings()).toBeTruthy();
    await app.addons.remove('media');
    expect((await app.transfer.previewImport())?.addons_to_offer).toEqual(['media']);
    await app.transfer.importSettings();
    expect(app.transfer.lastImport?.addons_to_offer).toEqual(['media']);
    const [addon] = await backend.listAddons();
    expect(addon?.state.type).toBe('not_installed');
  });
});

describe('commands', () => {
  it('cover the selection only when something is selected', async () => {
    const { app } = await make();
    expect(buildCommands(app).some((c) => c.group === 'Selected downloads')).toBe(false);
    app.selection.only(app.visible.find((id) => app.downloads.get(id)?.status === 'downloading')!);
    expect(buildCommands(app).some((c) => c.id === 'sel.pause')).toBe(true);
  });

  it('sort the list from the keyboard, as the column headings do', async () => {
    const { app } = await make();
    const sort = buildCommands(app).find((c) => c.id === 'list.sort.size.descending');
    expect(sort?.title).toBe('Sort the list by size, largest first');
    await sort?.run();
    expect(app.sort).toEqual({ column: 'size', descending: true });
    expect(buildCommands(app).some((c) => c.id === 'list.sort.size.descending')).toBe(false);
    expect(buildCommands(app).some((c) => c.id === 'list.sort.position')).toBe(true);
  });

  it('rank good matches first and are plain language', async () => {
    const { app } = await make();
    const all = buildCommands(app);
    expect(searchCommands(all, 'pause all')[0]?.id).toBe('dl.pauseall');
    expect(searchCommands(all, 'new queue')[0]?.id).toBe('queue.new');
    expect(
      searchCommands(all, 'bandwidth').some((c) => c.id === 'setting.global_speed_limit'),
    ).toBe(true);
    expect(fuzzyScore('xyz', 'Pause all downloads')).toBe(-1);
    for (const c of all) expect(c.title).not.toMatch(/\u2014/);
  });
});

describe('a browser download that started without asking (D63)', () => {
  it('joins the list, and the shell, not the page, tells the person', async () => {
    const { app, backend } = await make();
    await app.setAskBeforeDownload(false);
    const before = app.downloads.size;
    backend.simulateCapture();
    expect(app.downloads.size).toBe(before + 1);
    expect(app.toasts.items).toEqual([]);
  });
});
