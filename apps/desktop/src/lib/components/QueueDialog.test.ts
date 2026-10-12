// The queue dialog accepts the limits the core accepts and shows a speed limit in the unit it
// reads best in.

import { fireEvent, screen, within } from '@testing-library/svelte';
import { MAX_CONCURRENT_DOWNLOADS, NEW_QUEUE_MAX_CONCURRENT } from '@wirecatch/protocol';
import { KB, MB } from '@wirecatch/units';
import { afterEach, describe, expect, it } from 'vitest';
import type { AppState } from '$lib/state/app.svelte';
import { renderApp, settle } from '$lib/testing/setup';

/** More downloads at once than the desktop used to allow (32), and fewer than the core's limit. */
const MANY_AT_ONCE = 50;
/** A queue speed limit below a megabyte, typed in kilobytes. */
const SLOW_LIMIT_KB = 300;

let current: { app: AppState; unmount: () => void } | undefined;
afterEach(() => {
  current?.app.dispose();
  current?.unmount();
  current = undefined;
});

/** Opens the dialog for the sample queue named `name`, or for a new queue. */
async function openQueueDialog(name: string | null) {
  const r = await renderApp();
  current = r;
  const id = name === null ? null : (r.app.queues.find((q) => q.name === name)?.id ?? null);
  r.app.openDialog({ kind: 'queue', id });
  await settle();
  return { ...r, dialog: within(screen.getByRole('dialog')) };
}

async function save() {
  await fireEvent.click(screen.getByRole('button', { name: /^(Save|Create)/ }));
  await settle();
}

describe('the queue dialog', () => {
  it('starts a new queue where the core does and accepts as many downloads as it allows', async () => {
    const { app, dialog } = await openQueueDialog(null);
    const max = dialog.getByLabelText('Downloads at the same time');
    expect(max).toHaveValue(NEW_QUEUE_MAX_CONCURRENT);
    expect(max).toHaveAttribute('max', String(MAX_CONCURRENT_DOWNLOADS));

    await fireEvent.input(dialog.getByLabelText('Name'), { target: { value: 'Big batch' } });
    await fireEvent.input(max, { target: { value: String(MANY_AT_ONCE) } });
    await fireEvent.click(dialog.getByLabelText("Limit this queue's speed"));
    await fireEvent.input(dialog.getByLabelText('Speed limit'), {
      target: { value: String(SLOW_LIMIT_KB) },
    });
    await fireEvent.change(dialog.getByLabelText('Unit'), { target: { value: 'KB' } });
    await save();

    const made = app.queues.find((q) => q.name === 'Big batch');
    expect(made?.max_concurrent).toBe(MANY_AT_ONCE);
    expect(made?.speed_limit).toBe(SLOW_LIMIT_KB * KB);
  });

  it('refuses more downloads at once than the core allows', async () => {
    const { app, dialog } = await openQueueDialog(null);
    await fireEvent.input(dialog.getByLabelText('Name'), { target: { value: 'Too many' } });
    const max = dialog.getByLabelText('Downloads at the same time');
    await fireEvent.input(max, { target: { value: String(MAX_CONCURRENT_DOWNLOADS + 1) } });
    await save();
    // The field's own range stops the form; limits.test.ts covers the message behind it.
    expect(max).toBeInvalid();
    expect(app.queues.some((q) => q.name === 'Too many')).toBe(false);
  });

  it('shows a queue speed limit in megabytes from 1 MB on', async () => {
    const { app, dialog } = await openQueueDialog('Overnight');
    const limit = app.queues.find((q) => q.name === 'Overnight')?.speed_limit;
    expect(limit).toBe(4 * MB);
    expect(dialog.getByLabelText('Speed limit')).toHaveValue(4);
    expect(dialog.getByLabelText('Unit')).toHaveValue('MB');
  });
});
