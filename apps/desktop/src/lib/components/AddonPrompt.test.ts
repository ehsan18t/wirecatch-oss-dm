// The add-on prompt announces the install from a live region that is there before it starts, so
// screen readers hear the start; a region inserted together with its text is often not read.

import { screen, within } from '@testing-library/svelte';
import userEvent from '@testing-library/user-event';
import { afterEach, describe, expect, it } from 'vitest';
import type { AppState } from '$lib/state/app.svelte';
import { renderApp, settle } from '$lib/testing/setup';

let current: { app: AppState; unmount: () => void } | undefined;
afterEach(() => {
  current?.app.dispose();
  current?.unmount();
  current = undefined;
});

describe('the add-on prompt', () => {
  it('keeps one live region from before the install starts', async () => {
    const r = await renderApp({ empty: true });
    current = r;
    r.backend.simulateAddonRequired();
    await settle();
    const dialog = within(screen.getByRole('dialog'));
    const live = dialog.getByTestId('addon-live');
    expect(live).toHaveAttribute('aria-live', 'polite');
    expect(live.textContent?.trim()).toBe('');

    await userEvent.setup().click(dialog.getByRole('button', { name: 'Install', exact: true }));
    await settle();
    // The same element, now with the start of the install in it.
    expect(dialog.getByTestId('addon-live')).toBe(live);
    expect(live.textContent?.trim()).toBe('Installing the media add-on.');
  });
});
