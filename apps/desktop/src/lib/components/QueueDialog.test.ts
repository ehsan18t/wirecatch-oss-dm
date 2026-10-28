// The queue dialog follows the app's limits until one is switched on, accepts the limits the core accepts, shows a speed limit in the unit it reads best in, and says its hours in the app's own 24-hour times.

import { fireEvent, screen, within } from '@testing-library/svelte';
import {
  FOLLOW_APP,
  MAX_CONCURRENT_DOWNLOADS,
  MAX_CONNECTIONS_PER_DOWNLOAD,
  ownSetting,
} from '@wirecatch/protocol';
import { KB, MB } from '@wirecatch/units';
import { describe, expect, it } from 'vitest';
import { renderApp, settle } from '$lib/testing/setup';

/** More downloads at once than the desktop used to allow (32), and fewer than the core's limit. */
const MANY_AT_ONCE = 50;
/** A queue speed limit below a megabyte, typed in kilobytes. */
const SLOW_LIMIT_KB = 300;
/** Connections per download other than the app's default. */
const OWN_CONNECTIONS = 3;

/** Opens the dialog for the sample queue named `name`, or for a new queue. */
async function openQueueDialog(name: string | null) {
  const r = await renderApp();
  const id =
    name === null ? null : (r.app.queues.list.find((q) => q.settings.name === name)?.id ?? null);
  r.app.openDialog({ kind: 'queue', id });
  await settle();
  return { ...r, dialog: within(screen.getByRole('dialog')) };
}

/** Types `text` into a field and commits it, as leaving the field does. */
async function typeInto(field: HTMLElement, text: string) {
  await fireEvent.input(field, { target: { value: text } });
  await fireEvent.blur(field);
  await settle();
}

async function save() {
  await fireEvent.click(screen.getByRole('button', { name: /^(Save|Create)/ }));
  await settle();
}

describe('the queue dialog', () => {
  it('makes a queue that follows the app in every limit it leaves off', async () => {
    const { app, dialog } = await openQueueDialog(null);
    expect(dialog.queryByRole('spinbutton')).toBeNull();
    await typeInto(dialog.getByLabelText('Name'), 'Plain');
    await typeInto(dialog.getByLabelText('Description (optional)'), '  Everything else  ');
    await save();

    const made = app.queues.list.find((q) => q.settings.name === 'Plain');
    expect(made?.settings).toMatchObject({
      description: 'Everything else',
      max_concurrent: FOLLOW_APP,
      speed_limit: FOLLOW_APP,
      connections: FOLLOW_APP,
    });
  });

  it('gives a queue its own limits once they are switched on, as many as the core allows', async () => {
    const { app, dialog } = await openQueueDialog(null);
    await typeInto(dialog.getByLabelText('Name'), 'Big batch');
    await fireEvent.click(dialog.getByRole('switch', { name: 'Downloads at the same time' }));
    await settle();
    const max = dialog.getByRole('spinbutton', { name: 'Downloads at the same time' });
    expect(max).toHaveAttribute('aria-valuemax', String(MAX_CONCURRENT_DOWNLOADS));
    await typeInto(max, String(MANY_AT_ONCE));
    await fireEvent.click(dialog.getByRole('switch', { name: 'Speed limit' }));
    await settle();
    await fireEvent.click(dialog.getByRole('radio', { name: 'KB/s' }));
    await typeInto(dialog.getByRole('spinbutton', { name: 'Speed limit' }), String(SLOW_LIMIT_KB));
    await fireEvent.click(dialog.getByRole('switch', { name: 'Connections per download' }));
    await settle();
    const connections = dialog.getByRole('spinbutton', { name: 'Connections per download' });
    expect(connections).toHaveAttribute('aria-valuemax', String(MAX_CONNECTIONS_PER_DOWNLOAD));
    await typeInto(connections, String(OWN_CONNECTIONS));
    await save();

    const made = app.queues.list.find((q) => q.settings.name === 'Big batch');
    expect(made?.settings.max_concurrent).toEqual(ownSetting(MANY_AT_ONCE));
    expect(made?.settings.speed_limit).toEqual(ownSetting(SLOW_LIMIT_KB * KB));
    expect(made?.settings.connections).toEqual(ownSetting(OWN_CONNECTIONS));
  });

  it('keeps downloads at once within what the core allows', async () => {
    const { dialog } = await openQueueDialog(null);
    await fireEvent.click(dialog.getByRole('switch', { name: 'Downloads at the same time' }));
    await settle();
    const max = dialog.getByRole('spinbutton', { name: 'Downloads at the same time' });
    await typeInto(max, String(MAX_CONCURRENT_DOWNLOADS + 1));
    expect(max).toHaveAttribute('aria-valuenow', String(MAX_CONCURRENT_DOWNLOADS));
  });

  it('makes no queue when the form it would get is refused', async () => {
    const { app, dialog } = await openQueueDialog(null);
    await typeInto(dialog.getByLabelText('Name'), 'Never runs');
    await fireEvent.click(dialog.getByRole('switch', { name: 'Only run at certain times' }));
    await settle();
    const start = dialog.getByLabelText('Starts at');
    if (!(start instanceof HTMLInputElement)) throw new Error('no start time field');
    await typeInto(dialog.getByLabelText('Stops at'), start.value);
    await save();
    expect(
      dialog
        .getAllByRole('alert')
        .some((a) => a.textContent.includes('The start and stop times are the same')),
    ).toBe(true);
    expect(app.queues.list.some((q) => q.settings.name === 'Never runs')).toBe(false);
  });

  it('says its hours in 24-hour times and steps them by a quarter hour', async () => {
    const { dialog } = await openQueueDialog('Overnight');
    const start = dialog.getByLabelText('Starts at');
    expect(start).toHaveValue('01:00');
    await fireEvent.keyDown(start, { key: 'ArrowDown' });
    await settle();
    expect(start).toHaveValue('00:45');
  });

  it('shows a queue speed limit in megabytes from 1 MB on', async () => {
    const { app, dialog } = await openQueueDialog('Overnight');
    const limit = app.queues.list.find((q) => q.settings.name === 'Overnight')?.settings
      .speed_limit;
    expect(limit).toEqual(ownSetting(4 * MB));
    expect(dialog.getByRole('spinbutton', { name: 'Speed limit' })).toHaveAttribute(
      'aria-valuenow',
      '4',
    );
    expect(dialog.getByRole('radio', { name: 'MB/s' })).toBeChecked();
  });

  it('tells once the queue is done when asked, and says failures still come right away', async () => {
    const { app, dialog } = await openQueueDialog('Main');
    expect(dialog.getByRole('radio', { name: 'Each download' })).toBeChecked();
    await fireEvent.click(dialog.getByRole('radio', { name: 'When the queue is done' }));
    await settle();
    expect(dialog.getByText(/You still hear about a failed download right away\.$/)).toBeVisible();
    await save();
    expect(app.queues.list.find((q) => q.settings.name === 'Main')?.settings.notify).toBe(
      'when_done',
    );
  });

  it('asks before deleting, saying where the downloads go', async () => {
    const { app, dialog } = await openQueueDialog('Overnight');
    await fireEvent.click(dialog.getByRole('button', { name: 'Delete queue' }));
    await settle();
    // The button pressed went: the focus moves to the safe answer, not to the page.
    expect(dialog.getByRole('button', { name: 'Keep it' })).toHaveFocus();
    expect(dialog.getByText(/downloads? moves? to Main\.$/)).toHaveAttribute('role', 'status');
    await fireEvent.click(dialog.getByRole('button', { name: 'Keep it' }));
    await settle();
    expect(dialog.getByRole('button', { name: 'Delete queue' })).toHaveFocus();
    await fireEvent.click(dialog.getByRole('button', { name: 'Delete queue' }));
    await settle();
    expect(app.queues.list.some((q) => q.settings.name === 'Overnight')).toBe(true);
    await fireEvent.click(dialog.getByRole('button', { name: 'Delete Overnight' }));
    await settle();
    expect(app.queues.list.some((q) => q.settings.name === 'Overnight')).toBe(false);
  });
});
