// Dialogs keep Tab inside (Playwright checks the same in a real browser), and removing the media
// add-on from the palette asks first.

import { fireEvent, screen, within } from '@testing-library/svelte';
import userEvent from '@testing-library/user-event';
import { describe, expect, it } from 'vitest';
import { buildCommands } from '$lib/commands';
import { renderApp, settle } from '$lib/testing/setup';

async function setup() {
  const r = await renderApp({ empty: true });
  return r;
}

describe('dialogs', () => {
  it('wrap Tab and Shift+Tab around their own controls', async () => {
    const { app } = await setup();
    app.openDialog({ kind: 'queue', id: null });
    await settle();
    const dialog = screen.getByRole('dialog');
    // The first stop is the name to type; the last, the close button.
    const first = within(dialog).getByLabelText('Name');
    const last = within(dialog).getByRole('button', { name: 'Close' });

    last.focus();
    await fireEvent.keyDown(dialog, { key: 'Tab' });
    expect(first).toHaveFocus();

    await fireEvent.keyDown(dialog, { key: 'Tab', shiftKey: true });
    expect(last).toHaveFocus();
  });

  it('ask before the palette removes the media add-on', async () => {
    const { app, backend } = await setup();
    await app.addons.install('media');
    backend.tick(4500);
    await settle();
    buildCommands(app)
      .find((c) => c.id === 'addon.remove.media')
      ?.run();
    await settle();
    const dialog = within(screen.getByRole('alertdialog', { name: 'Remove the media add-on?' }));
    // Nothing is removed until the person says so.
    expect(app.addons.list?.[0]?.state.type).toBe('installed');
    await userEvent.setup().click(dialog.getByRole('button', { name: 'Remove' }));
    await settle();
    expect(app.addons.list?.[0]?.state.type).toBe('not_installed');
  });
});
