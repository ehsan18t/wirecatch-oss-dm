// The queue dialog accepts the limits the core accepts, shows a speed limit in the unit it reads best in, and says its hours in the app's own 24-hour times.

import { fireEvent, screen, within } from '@testing-library/svelte';
import { MAX_CONCURRENT_DOWNLOADS, NEW_QUEUE_MAX_CONCURRENT } from '@wirecatch/protocol';
import { KB, MB } from '@wirecatch/units';
import { describe, expect, it } from 'vitest';
import { renderApp, settle } from '$lib/testing/setup';

/** More downloads at once than the desktop used to allow (32), and fewer than the core's limit. */
const MANY_AT_ONCE = 50;
/** A queue speed limit below a megabyte, typed in kilobytes. */
const SLOW_LIMIT_KB = 300;

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
  it('starts a new queue where the core does and accepts as many downloads as it allows', async () => {
    const { app, dialog } = await openQueueDialog(null);
    const max = dialog.getByRole('spinbutton', { name: 'Downloads at the same time' });
    expect(max).toHaveAttribute('aria-valuenow', String(NEW_QUEUE_MAX_CONCURRENT));
    expect(max).toHaveAttribute('aria-valuemax', String(MAX_CONCURRENT_DOWNLOADS));

    await typeInto(dialog.getByLabelText('Name'), 'Big batch');
    await typeInto(max, String(MANY_AT_ONCE));
    await fireEvent.click(dialog.getByRole('switch', { name: "Limit this queue's speed" }));
    await settle();
    await fireEvent.click(dialog.getByRole('radio', { name: 'KB/s' }));
    await typeInto(dialog.getByRole('spinbutton', { name: 'Speed limit' }), String(SLOW_LIMIT_KB));
    await save();

    const made = app.queues.list.find((q) => q.settings.name === 'Big batch');
    expect(made?.settings.max_concurrent).toBe(MANY_AT_ONCE);
    expect(made?.settings.speed_limit).toBe(SLOW_LIMIT_KB * KB);
  });

  it('keeps downloads at once within what the core allows', async () => {
    const { dialog } = await openQueueDialog(null);
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
    expect(limit).toBe(4 * MB);
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
