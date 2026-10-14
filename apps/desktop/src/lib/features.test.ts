// Acceptance item 1: every feature is reachable from visible UI, along the path that feels natural rather than within a click count (D36), and from the command palette. Each case below performs the visible path for real and checks the outcome, then checks that the palette offers the same action.

import { downloadId, PROFILE_FORMAT_VERSION } from '@wirecatch/protocol';
import { fireEvent, screen, within } from '@testing-library/svelte';
import userEvent from '@testing-library/user-event';
import { describe, expect, it } from 'vitest';
import { buildCommands } from './commands';
import { downloadWindow, renderApp, rowByName, settle } from './testing/setup';

async function setup() {
  const r = await renderApp();
  const user = userEvent.setup();
  const click = async (el: HTMLElement) => {
    await user.click(el);
    await settle();
  };
  const palette = (id: string) => buildCommands(r.app).some((c) => c.id === id);
  return { ...r, user, click, palette };
}

const button = (name: RegExp | string) => screen.getByRole('button', { name });
const dialog = () => screen.getByRole('dialog');

describe('feature paths (visible in the interface, and in the palette)', () => {
  it('add a download: Add download button, 1 click', async () => {
    const { click, palette } = await setup();
    await click(button('Add download'));
    expect(within(dialog()).getByText('Add a download')).toBeInTheDocument();
    expect(palette('dl.add')).toBe(true);
  });

  it('paste a link anywhere: Ctrl+V, 0 clicks, then Enter downloads it', async () => {
    const { app, user } = await setup();
    const before = app.downloads.size;
    const clipboardData = { getData: () => 'see https://example.com/files/report.pdf' };
    await fireEvent.paste(document.body, { clipboardData });
    await settle();
    expect(
      within(dialog()).getByDisplayValue('https://example.com/files/report.pdf'),
    ).toBeInTheDocument();
    await user.keyboard('{Enter}');
    await settle();
    expect(app.downloads.size).toBe(before + 1);
  });

  it('pause and resume one download: row, then Pause or Resume, 2 clicks', async () => {
    const { app, click, palette } = await setup();
    await click(rowByName('ubuntu-24.04.3'));
    await click(button('Pause'));
    expect(app.selection.rows[0]?.status).toBe('paused');
    expect(palette('sel.pause')).toBe(true);
    await click(button('Resume'));
    expect(app.selection.rows[0]?.status).toBe('queued');
    expect(palette('sel.resume')).toBe(true);
  });

  it('error fixes: the fix button in the row, 1 click (also in the detail panel)', async () => {
    const { app, click, palette } = await setup();
    const inRow = (name: string, fix: string) =>
      within(rowByName(name)).getByRole('button', { name: fix });

    await click(inRow('Conference keynote', 'New link'));
    expect(within(dialog()).getByText('Give it a fresh link')).toBeInTheDocument();
    expect(palette('sel.refresh')).toBe(true);
    app.closeDialog();
    await settle();

    const nightly = rowByName('nightly-build');
    await click(within(nightly).getByRole('button', { name: 'Try again' }));
    expect(app.downloads.get(downloadId(Number(nightly.dataset.id)))?.status).toBe('queued');

    const climate = rowByName('climate-dataset');
    await click(within(climate).getByRole('button', { name: 'Change folder' }));
    const climateRow = app.downloads.get(downloadId(Number(climate.dataset.id)));
    expect(climateRow?.path.startsWith('E:')).toBe(false);
    expect(climateRow?.status).toBe('queued');
    expect(palette('sel.choosefolder')).toBe(true);

    // The detail panel shows the same message and fix.
    await click(rowByName('win11-dev'));
    const alert = screen.getByRole('alert');
    expect(alert).toHaveTextContent('Drive C: is full');
    expect(
      within(alert).getByRole('button', { name: 'Open the folder to free space' }),
    ).toBeInTheDocument();
    expect(palette('sel.retry')).toBe(true);
  });

  it('remove, cancel, show in folder, copy link: row, then the button, 2 clicks', async () => {
    const { click, palette } = await setup();
    await click(rowByName('LibreOffice'));
    expect(button('Show in folder')).toBeInTheDocument();
    expect(button('Copy link')).toBeInTheDocument();
    expect(button('Cancel and discard the partial file')).toBeInTheDocument();
    await click(button('Remove'));
    expect(within(dialog()).getByText(/Remove "LibreOffice/)).toBeInTheDocument();
    for (const id of ['sel.remove', 'sel.cancel', 'sel.folder']) expect(palette(id)).toBe(true);
  });

  it('move to another queue: row, then the Queue menu, 2 clicks (or drag onto the queue)', async () => {
    const { app, click, user, palette } = await setup();
    await click(rowByName('LibreOffice'));
    await user.selectOptions(screen.getByLabelText('Queue'), 'Work files');
    await settle();
    expect(app.selection.rows[0]?.queue).toBe(3);
    expect(palette('sel.move.2')).toBe(true);
  });

  it('pause all and resume all: 1 click each', async () => {
    const { app, click, palette } = await setup();
    await click(button('Pause all'));
    expect(app.counts.status.active + app.counts.status.waiting).toBe(0);
    await click(button('Resume all'));
    expect(app.counts.status.paused).toBe(0);
    expect(palette('dl.pauseall') && palette('dl.resumeall')).toBe(true);
  });

  it('queues: show 1 click, create 1 click, edit name, schedule and limits 1 click, delete 2 clicks', async () => {
    const { app, click, palette } = await setup();
    await click(screen.getByRole('button', { name: /^Overnight/ }));
    expect(screen.getByRole('heading', { level: 1, name: 'Overnight' })).toBeInTheDocument();
    expect(button('Pause queue')).toBeInTheDocument();
    await click(button('Edit Overnight: name, schedule and limits'));
    const d = dialog();
    expect(within(d).getByLabelText('Only run at certain times')).toBeChecked();
    expect(within(d).getByLabelText("Limit this queue's speed")).toBeChecked();
    expect(within(d).getByLabelText('Downloads at the same time')).toHaveValue(2);
    await click(within(d).getByRole('button', { name: 'Delete queue' }));
    await click(within(d).getByRole('button', { name: /Delete it/ }));
    expect(app.queues.list.some((q) => q.name === 'Overnight')).toBe(false);
    await click(button('New queue'));
    expect(within(dialog()).getByText('New queue')).toBeInTheDocument();
    for (const id of [
      'queue.new',
      'queue.edit.1',
      'queue.show.3',
      'queue.delete.3',
      'queue.pause.1',
    ]) {
      expect(palette(id)).toBe(true);
    }
  });

  it('status and file type filters: 1 click', async () => {
    const { app, click, palette } = await setup();
    await click(screen.getByRole('button', { name: /^Needs attention/ }));
    expect(app.visible.length).toBe(6);
    await click(screen.getByRole('button', { name: /^Video/ }));
    expect(app.view).toEqual({ kind: 'category', category: 'video' });
    expect(palette('show.status.failed') && palette('show.category.video')).toBe(true);
  });

  it('settings: 1 click to open, every setting visible there, export and import 2 clicks', async () => {
    const { click, palette } = await setup();
    await click(button(/^Settings/));
    for (const label of [
      'Appearance',
      'Start Wirecatch when you sign in',
      'Download folder',
      'Folders by file type',
      'Show a window before each download',
      'Keep the window open to show progress',
      'Sites to leave to the browser',
      'Speed limit',
      'Connections per download',
      'Downloads at the same time',
    ]) {
      expect(screen.getAllByText(label).length).toBeGreaterThan(0);
    }
    await click(button('Export settings'));
    expect(within(dialog()).getByText('Saved in the file')).toBeInTheDocument();
    expect(palette('setting.export') && palette('setting.import') && palette('setting.theme')).toBe(
      true,
    );
  });

  it('command palette and shortcuts: 1 click each', async () => {
    const { click } = await setup();
    await click(button(/Find any action/));
    expect(screen.getByRole('combobox', { name: 'Type an action or setting' })).toBeInTheDocument();
    await click(button(/Keyboard shortcuts/));
    expect(
      within(dialog()).getByRole('heading', { name: 'Keyboard shortcuts' }),
    ).toBeInTheDocument();
  });

  it('browser capture without a window: toast with Change and Open folder, 1 click each', async () => {
    const { app, backend, click } = await setup();
    await app.setAskBeforeDownload(false);
    backend.simulateCapture();
    await settle();
    const toasts = screen.getByRole('region', { name: 'Notifications' });
    expect(within(toasts).getByText(/^Downloading/)).toBeInTheDocument();
    const inToast = (name: string) => within(toasts).getByRole('button', { name });
    expect(inToast('Open folder')).toBeInTheDocument();
    expect(inToast('Show a window first next time')).toBeInTheDocument();
    await click(inToast('Change'));
    expect(screen.getByText(/Saving to/)).toBeInTheDocument();
  });

  it('a browser file opens its own window, which then shows its progress, 1 click', async () => {
    const { app, backend, click } = await setup();
    backend.simulateConfirmation();
    const window = within(await downloadWindow());
    expect(window.getByRole('heading', { name: 'Download this file?' })).toBeInTheDocument();
    // Never a dialog in the main window.
    expect(screen.queryByRole('dialog')).not.toBeInTheDocument();
    const before = app.downloads.size;
    await click(window.getByRole('button', { name: 'Download now' }));
    expect(app.questions.pending).toHaveLength(0);
    expect(app.downloads.size).toBe(before + 1);
    expect(window.getByRole('heading', { name: 'app-setup-3.2.0.exe' })).toBeInTheDocument();
    expect(window.getByRole('progressbar', { name: 'Downloaded' })).toBeInTheDocument();
    expect(window.getByRole('button', { name: 'Pause' })).toBeInTheDocument();
  });

  it('a progress window for any download: row, then Show progress window, 2 clicks', async () => {
    const { click, palette } = await setup();
    await click(rowByName('ubuntu-24.04.3'));
    expect(palette('sel.progress')).toBe(true);
    await click(button('Show progress window'));
    const window = within(await downloadWindow());
    expect(window.getByRole('heading', { level: 1 })).toHaveTextContent('ubuntu-24.04.3');
  });

  it('media add-on: Settings, then Install or Remove, 2 clicks', async () => {
    const { app, backend, click, palette } = await setup();
    expect(palette('addon.install.media')).toBe(true);
    await click(button(/^Settings/));
    await click(button('Install'));
    expect(app.addons.list?.[0]?.state.type).toBe('installing');
    backend.tick(4500);
    await settle();
    expect(app.addons.list?.[0]?.state.type).toBe('installed');
    expect(palette('addon.remove.media')).toBe(true);
    await click(button('Remove the media add-on'));
    expect(app.addons.list?.[0]?.state.type).toBe('not_installed');
  });

  it('check for add-on updates: Settings, then Check for updates, 2 clicks; the result in words', async () => {
    const { app, backend, click, palette } = await setup();
    await app.addons.install('media');
    backend.tick(4500);
    await click(button(/^Settings/));
    expect(palette('addon.check.media')).toBe(true);
    const note = () => app.toasts.items.at(-1);

    await click(button('Check for updates'));
    expect(note()).toMatchObject({ kind: 'info', message: 'The media add-on is up to date.' });

    backend.nextUpdateCheck = 'unreachable';
    await click(button('Check for updates'));
    expect(note()?.kind).toBe('error');
    expect(note()?.message).toMatch(/^Wirecatch could not check for a newer media add-on/);

    // D21.1: a release younger than two days waits, and the result says so plainly.
    backend.nextUpdateCheck = 'too-new';
    await click(button('Check for updates'));
    const waiting =
      'A newer version came out today; Wirecatch installs it once it is two days old.';
    expect(note()).toMatchObject({ kind: 'info', message: waiting });
    expect(app.addons.list?.[0]?.state.type).toBe('installed');
    expect(screen.getByText(waiting, { selector: 'p.note' })).toBeInTheDocument();

    backend.nextUpdateCheck = 'newer';
    await click(button('Check for updates'));
    expect(note()?.message).toMatch(/^A newer media add-on is ready \(yt-dlp 2026\.10\.30/);
    expect(button('Update')).toBeInTheDocument();
  });

  it('the list of public domains: Settings, then Check now, 2 clicks; its date and the result in words', async () => {
    const { app, backend, click } = await setup();
    await click(button(/^Settings/));
    // The month as the system writes it short: "Sep", or "Sept" with newer locale data.
    expect(screen.getByText(/^List of 15 Sept? 2026, not checked yet\.$/)).toBeInTheDocument();
    const note = () => app.toasts.items.at(-1);

    await click(button('Check now'));
    expect(note()).toMatchObject({
      kind: 'info',
      message: 'The list of public domains is up to date.',
    });
    expect(screen.getByText(/^List of 15 Sept? 2026, checked today at /)).toBeInTheDocument();

    backend.nextSuffixCheck = 'unreachable';
    await click(button('Check now'));
    expect(note()?.kind).toBe('error');
    expect(note()?.message).toMatch(/^Wirecatch could not update its list of public domains/);
    expect(
      screen.getByText(/^List of 15 Sept? 2026, the last check failed today at /),
    ).toBeInTheDocument();

    backend.nextSuffixCheck = 'newer';
    await click(button('Check now'));
    expect(note()).toMatchObject({
      kind: 'info',
      message: 'Wirecatch now uses a newer list of public domains.',
    });
    expect(screen.getByText(/^List of 1 Oct 2026, checked today at /)).toBeInTheDocument();
  });

  it('the list of public domains: the weekly check shows on an open settings page', async () => {
    const { app, backend, click } = await setup();
    await click(button(/^Settings/));
    backend.simulateWeeklySuffixCheck('newer');
    await settle();
    expect(screen.getByText(/^List of 1 Oct 2026, checked today at /)).toBeInTheDocument();

    backend.nextSuffixCheck = 'up-to-date';
    await click(button('Check now'));
    expect(app.toasts.items.at(-1)?.message).toBe('The list of public domains is up to date.');
  });

  it('stopping an update keeps the installed add-on; stopping a first install leaves none', async () => {
    const { app, backend, click } = await setup();
    await click(button(/^Settings/));
    await click(button('Install'));
    await click(button('Stop installing the media add-on'));
    expect(app.addons.list?.[0]?.state.type).toBe('not_installed');

    await click(button('Install'));
    backend.tick(4500);
    await settle();
    const version = app.addons.list?.[0]?.installed_version;
    expect(version).toBeTruthy();
    await app.addons.update('media');
    await settle();
    await click(button('Stop updating the media add-on'));
    expect(app.addons.list?.[0]?.state.type).toBe('installed');
    expect(app.addons.list?.[0]?.installed_version).toBe(version);
  });

  it('import shows what the file would change and asks first; a startup folder is refused', async () => {
    const { app, backend, click } = await setup();
    const plain = <T>(value: T): T => JSON.parse(JSON.stringify(value)) as T;
    if (!app.settings) throw new Error('no settings');
    const before = plain(app.settings);
    const profile = {
      format_version: PROFILE_FORMAT_VERSION,
      app_version: '0.1.0',
      exported_at: 0,
      settings: {
        ...before,
        default_folder:
          'C:\\Users\\ada\\AppData\\Roaming\\Microsoft\\Windows\\Start Menu\\Programs\\Startup',
        connections_per_download: 3,
      },
      queues: plain(app.queues.list),
      addons: [],
    };
    backend.putSettingsFile('C:\\Users\\ada\\Downloads\\evil.json', profile);
    app.openDialog({ kind: 'import' });
    await settle();
    await click(button('Choose a settings file'));
    await settle();
    const preview = within(dialog());
    expect(preview.getByText(/Nothing has changed yet/)).toBeInTheDocument();
    expect(preview.getByText(/is a system or startup folder/)).toBeInTheDocument();
    expect(preview.getByRole('button', { name: 'Import' })).toHaveFocus();
    // Nothing changed yet; Cancel leaves it that way.
    expect(plain(app.settings)).toEqual(before);
    await click(preview.getByRole('button', { name: 'Cancel' }));
    expect(plain(app.settings)).toEqual(before);
    expect(app.transfer.importPreview).toBeNull();

    // Imported after all: the startup folder is replaced by Downloads.
    app.openDialog({ kind: 'import' });
    await settle();
    await click(button('Choose a settings file'));
    await settle();
    await click(button('Import'));
    await settle();
    expect(app.settings.connections_per_download).toBe(3);
    expect(app.settings.default_folder).toBe('{Downloads}');
  });

  it('import asks before installing an add-on the file lists: Install in the dialog, 1 click', async () => {
    const { app, backend, click } = await setup();
    await app.addons.install('media');
    backend.tick(4500);
    await app.transfer.exportSettings();
    await app.addons.remove('media');
    await settle();
    app.openDialog({ kind: 'import' });
    await settle();
    await click(button('Choose a settings file'));
    await settle();
    // First what the file would change; nothing is applied until Import.
    expect(within(dialog()).getByText(/Wirecatch offers it after the import/)).toBeInTheDocument();
    await click(button('Import'));
    await settle();
    const offer = within(dialog());
    expect(offer.getByText(/The file lists the media add-on/)).toBeInTheDocument();
    expect(app.addons.list?.[0]?.state.type).toBe('not_installed');
    // The button that started the import is gone; Done has the focus.
    expect(offer.getByRole('button', { name: 'Done' })).toHaveFocus();
    await click(offer.getByRole('button', { name: 'Install' }));
    expect(app.addons.list?.[0]?.state.type).toBe('installing');
    // The same button now stops the install, so the focus stays on it.
    const stop = offer.getByRole('button', { name: 'Stop installing the media add-on' });
    expect(stop).toHaveFocus();
    backend.tick(4500);
    await settle();
    // Installed: Stop is gone, and the focus moves to the card's next button rather than being lost.
    expect(offer.getByRole('button', { name: 'Check for updates' })).toHaveFocus();
  });

  it('quality picker: its own window; closed, Waiting for you brings it back, 1 click', async () => {
    const { app, backend, click, palette } = await setup();
    backend.simulateMediaChoice();
    let window = within(await downloadWindow());
    expect(window.getByRole('heading', { name: 'Pick a quality' })).toBeInTheDocument();
    await click(window.getByRole('button', { name: 'Close this window' }));
    expect(screen.queryByRole('region', { name: /^Wirecatch download window/ })).toBeNull();
    // Closing is not an answer: it waits, counted in the sidebar and offered in the palette.
    expect(palette('questions.show')).toBe(true);
    await click(button(/Waiting for you/));
    window = within(await downloadWindow());
    await click(window.getByRole('radio', { name: /720p/ }));
    const before = app.downloads.size;
    await click(window.getByRole('button', { name: 'Download now' }));
    expect(app.downloads.size).toBe(before + 1);
  });
});
