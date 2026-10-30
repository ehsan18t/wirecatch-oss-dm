// A queue saves to its own folder or follows the folders by file type, and a download window follows the queue picked in it (D94).

import { ownSetting } from '@wirecatch/protocol';
import { screen, within } from '@testing-library/svelte';
import { describe, expect, it, vi } from 'vitest';
import { DOWNLOADS, PICKED_FOLDERS } from '$lib/backend/mock';
import { button, downloadWindow, settle, setupWithUser } from './testing/setup';

const OVERNIGHT = String.raw`D:\Overnight`;

describe('a queue can save to its own folder', () => {
  it('asks for the folder as it is turned on, and stays off when none is picked', async () => {
    const { app, backend, user, click } = await setupWithUser();
    app.openDialog({ kind: 'queue', id: null });
    await settle();
    await user.type(screen.getByLabelText('Name'), 'Weekend');
    expect(screen.getByText('Global: by file type')).toBeInTheDocument();
    const own = screen.getByRole('switch', { name: /Save to its own folder/ });
    vi.spyOn(backend, 'pickFolder').mockResolvedValueOnce(null);
    await click(own);
    expect(own).not.toBeChecked();

    await click(own);
    expect(own).toBeChecked();
    expect(screen.getByText(PICKED_FOLDERS[0] ?? '')).toBeInTheDocument();
    await click(button('Create queue'));
    const made = app.queues.list.find((q) => q.settings.name === 'Weekend');
    expect(made?.settings.folder).toEqual(ownSetting(PICKED_FOLDERS[0]));
  });

  it('turned off, the queue follows the app again', async () => {
    const { app, click } = await setupWithUser();
    const overnight = app.queues.list.find((q) => q.settings.name === 'Overnight');
    if (!overnight) throw new Error('no Overnight queue in the sample');
    app.openDialog({ kind: 'queue', id: overnight.id });
    await settle();
    expect(screen.getByText(OVERNIGHT)).toBeInTheDocument();
    await click(screen.getByRole('switch', { name: /Save to its own folder/ }));
    await click(button('Save'));
    expect(app.queues.byId(overnight.id)?.settings.folder).toEqual({ source: 'global' });
  });
});

describe('a download window follows the queue picked in it', () => {
  it('shows the picked queue’s folder and saves there, until a folder is picked by hand', async () => {
    const { backend, click } = await setupWithUser();
    const confirm = vi.spyOn(backend, 'confirmCapture');
    backend.simulateConfirmation();
    const window = within(await downloadWindow());
    const programs = String.raw`${DOWNLOADS}\Programs`;
    expect(window.getByText(programs)).toBeInTheDocument();

    await click(window.getByRole('combobox', { name: 'Queue' }));
    await click(screen.getByRole('option', { name: /Overnight/ }));
    expect(window.getByText(OVERNIGHT)).toBeInTheDocument();
    expect(window.queryByRole('button', { name: 'Use default' })).toBeNull();
    await click(window.getByRole('button', { name: 'Download now' }));

    expect(confirm).toHaveBeenCalledWith(
      expect.anything(),
      expect.objectContaining({ folder: null }),
    );
    const answered = confirm.mock.results[0];
    if (answered?.type !== 'return') throw new Error('the window sent no answer');
    expect(backend.folderOf(await answered.value)).toBe(OVERNIGHT);
  });

  it('a folder picked in the window stays when the queue changes', async () => {
    const { backend, click } = await setupWithUser();
    backend.simulateConfirmation();
    const window = within(await downloadWindow());
    await click(window.getByRole('button', { name: 'Change' }));
    await click(window.getByRole('combobox', { name: 'Queue' }));
    await click(screen.getByRole('option', { name: /Overnight/ }));
    expect(window.getByText(PICKED_FOLDERS[0] ?? '')).toBeInTheDocument();
    await click(window.getByRole('button', { name: 'Use default' }));
    expect(window.getByText(OVERNIGHT)).toBeInTheDocument();
  });

  it('says who chose the folder, and a pick of the app’s own folder stays the app’s', async () => {
    const { backend, click } = await setupWithUser();
    backend.simulateConfirmation();
    const window = within(await downloadWindow());
    expect(window.getByText('Default folder for this file type')).toBeInTheDocument();

    await click(window.getByRole('combobox', { name: 'Queue' }));
    await click(screen.getByRole('option', { name: /Overnight/ }));
    expect(window.getByText('Overnight queue folder')).toBeInTheDocument();

    vi.spyOn(backend, 'pickFolder').mockResolvedValueOnce(OVERNIGHT);
    await click(window.getByRole('button', { name: 'Change' }));
    expect(window.getByText('Overnight queue folder')).toBeInTheDocument();
    expect(window.queryByRole('button', { name: 'Use default' })).toBeNull();

    await click(window.getByRole('button', { name: 'Change' }));
    expect(window.getByText('Custom folder')).toBeInTheDocument();
  });
});
