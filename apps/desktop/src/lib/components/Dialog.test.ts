// Dialogs keep Tab inside (Playwright checks the same in a real browser), and removing the media
// add-on from the palette asks first.

import { fireEvent, screen, within } from '@testing-library/svelte';
import userEvent from '@testing-library/user-event';
import { afterEach, describe, expect, it } from 'vitest';
import { buildCommands } from '$lib/commands';
import type { AppState } from '$lib/state/app.svelte';
import { renderApp, settle } from '$lib/testing/setup';

let current: { app: AppState; unmount: () => void } | undefined;
afterEach(() => {
  current?.app.dispose();
  current?.unmount();
  current = undefined;
});

async function setup() {
  const r = await renderApp({ empty: true });
  current = r;
  return r;
}

describe('dialogs', () => {
  it('wrap Tab and Shift+Tab around their own buttons', async () => {
    const { backend } = await setup();
    backend.simulateMediaChoice();
    await settle();
    const dialog = screen.getByRole('dialog');
    const buttons = within(dialog).getAllByRole('button');
    const first = buttons[0] as HTMLElement;
    const last = buttons.at(-1) as HTMLElement;

    last.focus();
    await fireEvent.keyDown(dialog, { key: 'Tab' });
    expect(first).toHaveFocus();

    await fireEvent.keyDown(dialog, { key: 'Tab', shiftKey: true });
    expect(last).toHaveFocus();
  });

  it('ask before the palette removes the media add-on', async () => {
    const { app, backend } = await setup();
    await app.installAddon('media');
    backend.tick(4500);
    await settle();
    buildCommands(app)
      .find((c) => c.id === 'addon.remove.media')
      ?.run();
    await settle();
    const dialog = within(screen.getByRole('dialog', { name: 'Remove the media add-on?' }));
    // Nothing is removed until the person says so.
    expect(app.addons?.[0]?.state.type).toBe('installed');
    await userEvent.setup().click(dialog.getByRole('button', { name: 'Remove' }));
    await settle();
    expect(app.addons?.[0]?.state.type).toBe('not_installed');
  });
});
