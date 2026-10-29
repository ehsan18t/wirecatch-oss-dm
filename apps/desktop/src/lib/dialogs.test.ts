// Actions that cannot be undone ask first, with the focus on the safe choice.

import { screen } from '@testing-library/svelte';
import { describe, expect, it } from 'vitest';
import { button, detailsOf, rowByName, setupWithUser } from './testing/setup';

describe('destructive actions ask first, with focus on the safe choice', () => {
  it('cancel asks, focuses the choice to keep it, and only then discards', async () => {
    const { app, click } = await setupWithUser();
    await click(rowByName('LibreOffice'));
    const id = app.selection.rows[0]!.id;
    await click(detailsOf('LibreOffice').getByRole('button', { name: 'More actions' }));
    await click(screen.getByRole('menuitem', { name: 'Cancel download' }));
    // It waits in line, so nothing downloads to keep: the safe choice keeps it.
    expect(document.activeElement).toBe(button('Keep it'));
    expect(app.downloads.get(id)?.status).not.toBe('cancelled');
    await click(button('Cancel download'));
    expect(app.downloads.get(id)?.status).toBe('cancelled');
  });

  it('remove focuses Keep', async () => {
    const { click } = await setupWithUser();
    await click(rowByName('LibreOffice'));
    await click(detailsOf('LibreOffice').getByRole('button', { name: 'Remove' }));
    expect(document.activeElement).toBe(button('Keep'));
  });
});
