// The add-on offer in a video's own window announces the install from a live region that is
// there before it starts, so screen readers hear the start; a region inserted together with its
// text is often not read.

import { within } from '@testing-library/svelte';
import userEvent from '@testing-library/user-event';
import { describe, expect, it } from 'vitest';
import { downloadWindow, renderApp, settle } from '$lib/testing/setup';

describe('the add-on offer', () => {
  it('keeps one live region from before the install starts', async () => {
    const r = await renderApp({ empty: true });
    r.backend.simulateAddonRequired();
    const window = within(await downloadWindow());
    expect(window.getByRole('heading', { level: 1 })).toHaveTextContent(
      'Install the media add-on?',
    );
    expect(
      window.getByText(/^It is needed to download from www\.youtube\.com\./),
    ).toBeInTheDocument();
    const live = window.getByTestId('addon-live');
    expect(live).toHaveAttribute('aria-live', 'polite');
    expect(live.textContent.trim()).toBe('');

    // The button says what it downloads.
    expect(window.getByRole('button', { name: /^Install/ })).toHaveTextContent(
      /^Install \(\d+ MB\)$/,
    );
    await userEvent.setup().click(window.getByRole('button', { name: /^Install/ }));
    await settle();
    // The same element, now with the start of the install in it.
    expect(window.getByTestId('addon-live')).toBe(live);
    expect(live.textContent.trim()).toBe('Installing the media add-on.');
  });

  it('gives the focus to Hide when Install, which had it, goes', async () => {
    const r = await renderApp({ empty: true });
    r.backend.simulateAddonRequired();
    const window = within(await downloadWindow());
    const install = window.getByRole('button', { name: /^Install/ });
    expect(install).toHaveFocus();
    await userEvent.setup().keyboard('{Enter}');
    await settle();
    expect(window.queryByRole('button', { name: /^Install/ })).toBeNull();
    expect(window.getByRole('button', { name: 'Hide' })).toHaveFocus();
  });
});
