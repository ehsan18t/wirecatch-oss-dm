// Adding downloads by hand in the Add dialog: by keyboard and pointer, what it says while the links are not right, where they go, what it keeps when the app refuses one, and where the focus goes.

import { queueId } from '@wirecatch/protocol';
import { fireEvent, screen, waitFor, within } from '@testing-library/svelte';
import userEvent from '@testing-library/user-event';
import { describe, expect, it, vi } from 'vitest';
import { rulesOf } from '$lib/queue-rules';
import { NO_LINK } from '$lib/state/add-draft.svelte';
import { renderApp, settle } from '$lib/testing/setup';

async function setup(options: Parameters<typeof renderApp>[0] = {}) {
  const r = await renderApp(options);
  const user = userEvent.setup();
  const press = async (keys: string) => {
    await user.keyboard(keys);
    await settle();
  };
  const click = async (el: HTMLElement) => {
    await user.click(el);
    await settle();
  };
  return { ...r, user, press, click };
}

const dialog = () => screen.getByRole('dialog', { name: /new downloads?$/i });
const inDialog = () => within(dialog());
const linksBox = () => inDialog().getByRole('textbox', { name: 'Links' });
const headerAdd = () =>
  within(screen.getByRole('group', { name: 'List tools' })).getByRole('button', {
    name: 'New download',
  });

const QUEUE_HELP = 'Automatic sends each link to the queue whose rules match it.';

describe('the Add dialog', () => {
  it('opens on the links box, and Escape gives the focus back to what opened it', async () => {
    const { click, press } = await setup();
    await click(headerAdd());
    expect(dialog()).toHaveAccessibleName('New download');
    expect(dialog()).toHaveAttribute('aria-modal', 'true');
    expect(linksBox()).toHaveFocus();
    expect(linksBox()).toHaveAccessibleDescription('One link per line, or paste several at once.');
    await press('{Escape}');
    expect(screen.queryByRole('dialog', { name: /new downloads?$/i })).toBeNull();
    expect(headerAdd()).toHaveFocus();
  });

  it('downloads every link with Ctrl+Enter in the links box, and shows the first selected', async () => {
    const { app, user, press } = await setup();
    const before = app.downloads.size;
    app.openDialog({ kind: 'add', urls: [] });
    await settle();
    await user.type(linksBox(), 'https://a.example/one.zip{Enter}https://a.example/two.zip');
    await settle();
    expect(dialog()).toHaveAccessibleName('2 new downloads');
    expect(inDialog().getByRole('button', { name: 'Download 2' })).toBeInTheDocument();
    await press('{Control>}{Enter}{/Control}');
    expect(screen.queryByRole('dialog', { name: /new downloads?$/i })).toBeNull();
    expect(app.downloads.size).toBe(before + 2);
    expect(app.selection.rows.map((r) => r.url)).toEqual(['https://a.example/one.zip']);
  });

  it('starts on Download when opened with links, so Enter downloads them', async () => {
    const { app, press } = await setup();
    const before = app.downloads.size;
    app.pasteText('https://a.example/report.pdf');
    await settle();
    expect(inDialog().getByRole('button', { name: 'Download' })).toHaveFocus();
    await press('{Enter}');
    expect(app.downloads.size).toBe(before + 1);
  });

  it('asks for a link in the field, and the focus goes to it', async () => {
    const { app, click, press, user } = await setup();
    const add = vi.spyOn(app.actions, 'add');
    app.openDialog({ kind: 'add', urls: [] });
    await settle();
    await click(inDialog().getByRole('button', { name: 'Download' }));
    expect(add).not.toHaveBeenCalled();
    expect(linksBox()).toHaveFocus();
    expect(linksBox()).toHaveAttribute('aria-invalid', 'true');
    expect(linksBox()).toHaveAccessibleDescription(NO_LINK);
    await user.type(linksBox(), 'h');
    await settle();
    expect(linksBox()).not.toHaveAttribute('aria-invalid');
    // With the focus already in the box, moving it there says nothing: the dialog says it.
    await user.clear(linksBox());
    await press('{Control>}{Enter}{/Control}');
    expect(inDialog().getByRole('status')).toHaveTextContent(NO_LINK);
  });

  it('says which lines it leaves out', async () => {
    const { app, user } = await setup();
    app.openDialog({ kind: 'add', urls: [] });
    await settle();
    await user.type(linksBox(), 'https://a.example/one.zip{Enter}my notes');
    await settle();
    expect(linksBox()).toHaveAccessibleDescription('One line has no link in it and is left out.');
  });

  it('names one link as the server would, or as the person types', async () => {
    const { app, backend, user, press } = await setup();
    const add = vi.spyOn(backend, 'addDownload');
    app.openDialog({ kind: 'add', urls: ['https://a.example/dir/My%20File.pdf'] });
    await settle();
    const name = inDialog().getByRole('textbox', { name: 'File name' });
    expect(name).toHaveAttribute('placeholder', 'My File.pdf');
    await user.type(name, 'mine.pdf');
    await press('{Enter}');
    expect(add).toHaveBeenCalledWith(expect.objectContaining({ file_name: 'mine.pdf' }));
  });

  it('has no file name for several links', async () => {
    const { app } = await setup();
    app.openDialog({ kind: 'add', urls: ['https://a.example/1.zip', 'https://a.example/2.zip'] });
    await settle();
    expect(inDialog().queryByRole('textbox', { name: 'File name' })).toBeNull();
  });

  it('saves where the person picks, or back where Settings says', async () => {
    const { app, backend, click } = await setup();
    vi.spyOn(backend, 'pickFolder').mockResolvedValueOnce(String.raw`E:\Media`);
    app.openDialog({ kind: 'add', urls: ['https://a.example/1.zip'] });
    await settle();
    const saveTo = inDialog().getByRole('group', { name: 'Save to' });
    expect(saveTo).toHaveTextContent('By file type (from Settings)');
    expect(within(saveTo).queryByRole('button', { name: 'Use default' })).toBeNull();
    await click(within(saveTo).getByRole('button', { name: 'Change' }));
    expect(saveTo).toHaveTextContent(String.raw`E:\Media`);
    expect(within(saveTo).getByRole('button', { name: 'Change' })).toHaveAccessibleDescription(
      String.raw`E:\Media`,
    );
    await click(within(saveTo).getByRole('button', { name: 'Use default' }));
    expect(saveTo).toHaveTextContent('By file type');
  });

  it('keeps the focus in the form when Use default goes with the folder', async () => {
    const { app, backend, click, press } = await setup();
    vi.spyOn(backend, 'pickFolder').mockResolvedValueOnce(String.raw`E:\Media`);
    app.openDialog({ kind: 'add', urls: ['https://a.example/1.zip'] });
    await settle();
    const saveTo = inDialog().getByRole('group', { name: 'Save to' });
    await click(within(saveTo).getByRole('button', { name: 'Change' }));
    within(saveTo).getByRole('button', { name: 'Use default' }).focus();
    await press('{Enter}');
    expect(within(saveTo).queryByRole('button', { name: 'Use default' })).toBeNull();
    expect(within(saveTo).getByRole('button', { name: 'Change' })).toHaveFocus();
  });

  it('adds to the queue picked, paused when asked', async () => {
    const { app, backend, click } = await setup();
    const add = vi.spyOn(backend, 'addDownload');
    app.openDialog({ kind: 'add', urls: ['https://a.example/1.zip'] });
    await settle();
    const queue = inDialog().getByRole('combobox', { name: 'Queue' });
    // Automatic is chosen; the default queue is marked as the default in the list only.
    expect(queue).toHaveTextContent('Automatic');
    await click(queue);
    expect(screen.getByRole('option', { name: /^Main/ })).toHaveTextContent('The default queue');
    await click(screen.getByRole('option', { name: /^Overnight/ }));
    expect(queue).toHaveAccessibleDescription(
      `${QUEUE_HELP} Queue: Overnight. You picked it. For this add only.`,
    );
    await click(inDialog().getByRole('button', { name: 'Add paused' }));
    expect(add).toHaveBeenCalledWith(expect.objectContaining({ queue: queueId(2), start: false }));
  });

  it('sends each link where the rules do, and says where and why', async () => {
    const { app, user, press } = await setup();
    const work = app.queues.list.find((q) => q.settings.name === 'Work files');
    if (!work) throw new Error('no sample queue');
    const settings = JSON.parse(JSON.stringify(work.settings)) as typeof work.settings;
    await app.queues.save(work.id, {
      ...settings,
      add_downloads: { kind: 'auto', ...rulesOf(undefined), sites: ['share.example.com'] },
    });
    app.openDialog({ kind: 'add', urls: ['https://share.example.com/a.zip'] });
    await settle();
    const queue = inDialog().getByRole('combobox', { name: 'Queue' });
    await waitFor(() => {
      expect(queue).toHaveAccessibleDescription(
        `${QUEUE_HELP} Queue: Work files. Its rules match share.example.com.`,
      );
    });
    await user.type(linksBox(), '{End}{Enter}https://elsewhere.example/b.zip');
    await waitFor(() => {
      expect(queue).toHaveAccessibleDescription(
        `${QUEUE_HELP} Queue: 1 link to Work files, 1 to Main.`,
      );
    });
    await press('{Control>}{Enter}{/Control}');
    const [added] = app.selection.rows;
    expect(added?.url).toBe('https://share.example.com/a.zip');
    expect(added?.queue).toBe(work.id);
    expect(added?.queue_reason).toEqual({
      kind: 'matched',
      rule: { kind: 'site', value: 'share.example.com' },
    });
  });

  it('starts on the queue shown in the main window', async () => {
    const { app } = await setup();
    app.show({ kind: 'queue', id: queueId(2) });
    app.openDialog({ kind: 'add', urls: [] });
    await settle();
    const queue = inDialog().getByRole('combobox', { name: 'Queue' });
    expect(queue).toHaveTextContent('Overnight');
    expect(queue).toHaveAccessibleDescription(
      `${QUEUE_HELP} Queue: Overnight. It is the queue you had open.`,
    );
  });

  it('keeps a link the app refused in the box and says why there, in the dialog', async () => {
    const { app, backend, press } = await setup();
    vi.spyOn(backend, 'addDownload').mockRejectedValueOnce(new Error('The server said no.'));
    app.pasteText('https://bad.example/x.zip');
    await settle();
    await press('{Enter}');
    // The dialog's own alert, beside its empty live regions for the window's news.
    const [alert, ...more] = inDialog()
      .getAllByRole('alert')
      .filter((el) => el.textContent.trim() !== '');
    expect(more).toEqual([]);
    expect(alert).toHaveTextContent('Could not add the link');
    expect(alert).toHaveTextContent('The server said no.');
    expect(linksBox()).toHaveValue('https://bad.example/x.zip');
    expect(linksBox()).toHaveFocus();
  });

  it('leaves a send in flight alone when asked to send again', async () => {
    const { app, backend, click, press } = await setup();
    let release!: () => void;
    const gate = new Promise<void>((resolve) => {
      release = resolve;
    });
    const real = backend.addDownload.bind(backend);
    vi.spyOn(backend, 'addDownload').mockImplementation(async (r) => {
      await gate;
      return real(r);
    });
    app.openDialog({ kind: 'add', urls: ['https://a.example/1.zip'] });
    await settle();
    const paused = inDialog().getByRole('button', { name: 'Add paused' });
    await click(paused);
    expect(paused).toHaveAttribute('aria-busy', 'true');
    // Enter in the file name sends the form again; Ctrl+Enter in the links box does too.
    const name = inDialog().getByRole('textbox', { name: 'File name' });
    name.focus();
    await press('{Enter}');
    expect(paused).toHaveAttribute('aria-busy', 'true');
    expect(name).toHaveFocus();
    linksBox().focus();
    await press('{Control>}{Enter}{/Control}');
    expect(paused).toHaveAttribute('aria-busy', 'true');
    release();
    await settle();
    expect(screen.queryByRole('dialog', { name: /new downloads?$/i })).toBeNull();
  });

  it('keeps what was typed when links from a second launch arrive (DUI-8)', async () => {
    const { app, user } = await setup();
    app.openDialog({ kind: 'add', urls: [] });
    await settle();
    const box = linksBox();
    await user.type(box, 'https://typed.example/one.zip');
    // What a second launch's links do to an open form (the state's tests follow them from the shell).
    app.addDraft?.append(['https://launched.example/two.zip']);
    await settle();
    expect(linksBox()).toBe(box);
    expect(box).toHaveValue('https://typed.example/one.zip\nhttps://launched.example/two.zip');
  });

  it('keeps Tab inside, around its own controls', async () => {
    const { app } = await setup();
    app.openDialog({ kind: 'add', urls: [] });
    await settle();
    const buttons = inDialog().getAllByRole('button');
    const last = buttons.at(-1)!;
    last.focus();
    await fireEvent.keyDown(last, { key: 'Tab' });
    expect(linksBox()).toHaveFocus();
  });
});
