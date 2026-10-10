// Acceptance item 1: every feature is reachable from visible UI in at most two clicks, and from
// the command palette. Each case below performs the visible path for real and checks the
// outcome, then checks that the palette offers the same action.

import { fireEvent, screen, within } from '@testing-library/svelte';
import userEvent from '@testing-library/user-event';
import { afterEach, describe, expect, it } from 'vitest';
import { buildCommands } from './commands';
import type { AppState } from './state/app.svelte';
import { renderApp, rowByName, settle } from './testing/setup';

let current: { app: AppState; unmount: () => void } | undefined;
afterEach(() => {
  current?.app.dispose();
  current?.unmount();
  current = undefined;
});

async function setup() {
  const r = await renderApp();
  current = r;
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

describe('feature paths (two clicks or fewer, and in the palette)', () => {
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
    fireEvent.paste(document.body, { clipboardData });
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
    expect(app.selectedRows[0]?.status).toBe('paused');
    expect(palette('sel.pause')).toBe(true);
    await click(button('Resume'));
    expect(app.selectedRows[0]?.status).toBe('queued');
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
    expect(app.downloads.get(Number(nightly.dataset.id))?.status).toBe('queued');

    const climate = rowByName('climate-dataset');
    await click(within(climate).getByRole('button', { name: 'Change folder' }));
    const climateRow = app.downloads.get(Number(climate.dataset.id));
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
    expect(app.selectedRows[0]?.queue).toBe(3);
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
    expect(app.queues.some((q) => q.name === 'Overnight')).toBe(false);
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
    expect(app.visible.length).toBe(5);
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
      'Ask before each download',
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

  it('browser capture: toast with Change and Open folder, 1 click each', async () => {
    const { backend, click } = await setup();
    backend.simulateCapture();
    await settle();
    const toasts = screen.getByRole('region', { name: 'Notifications' });
    expect(within(toasts).getByText(/^Downloading/)).toBeInTheDocument();
    const inToast = (name: string) => within(toasts).getByRole('button', { name });
    expect(inToast('Open folder')).toBeInTheDocument();
    expect(inToast('Ask me first next time')).toBeInTheDocument();
    await click(inToast('Change'));
    expect(screen.getByText(/Saving to/)).toBeInTheDocument();
  });

  it('ask before downloading: the confirmation dialog appears on its own', async () => {
    const { app, backend, click } = await setup();
    backend.simulateConfirmation();
    await settle();
    expect(within(dialog()).getByText('Download this file?')).toBeInTheDocument();
    const before = app.downloads.size;
    await click(button('Download'));
    expect(app.pending).toHaveLength(0);
    expect(app.downloads.size).toBe(before + 1);
  });
});
