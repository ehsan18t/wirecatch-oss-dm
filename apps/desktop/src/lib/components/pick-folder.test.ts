// A folder picker that fails says so in a note, wherever it was opened from, instead of failing
// silently with an unhandled rejection.

import { fireEvent, screen, within } from '@testing-library/svelte';
import { describe, expect, it, vi } from 'vitest';
import { renderApp, settle } from '$lib/testing/setup';

const NO_PICKER = 'The folder picker could not open.';

async function withFailingPicker() {
  const r = await renderApp();
  vi.spyOn(r.backend, 'pickFolder').mockRejectedValue(new Error(NO_PICKER));
  return r;
}

describe('picking a folder', () => {
  it('says why in the Add dialog', async () => {
    const { app } = await withFailingPicker();
    app.openDialog({ kind: 'add', urls: ['https://example.com/a.zip'] });
    await settle();
    const dialog = within(screen.getByRole('dialog'));
    await fireEvent.click(dialog.getByRole('button', { name: 'Change' }));
    await settle();
    expect(screen.getByRole('alert')).toHaveTextContent(NO_PICKER);
  });

  it('says why on the settings screen', async () => {
    const { app } = await withFailingPicker();
    app.openSettings('default_folder');
    await settle();
    const [change] = screen.getAllByRole('button', { name: 'Change' });
    if (!change) throw new Error('no Change button');
    await fireEvent.click(change);
    await settle();
    expect(screen.getByRole('alert')).toHaveTextContent(NO_PICKER);
  });
});
