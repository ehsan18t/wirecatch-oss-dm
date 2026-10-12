// The list's one-time hints: "Got it" dismisses a hint for good, which is remembered in the
// webview's storage.

import { screen } from '@testing-library/svelte';
import userEvent from '@testing-library/user-event';
import { afterEach, describe, expect, it } from 'vitest';
import { HINT_TEXT, Hints } from '$lib/state/hints.svelte';
import { renderApp, settle } from '$lib/testing/setup';

afterEach(() => {
  localStorage.clear();
});

describe('the list hints', () => {
  it('go away with Got it and stay dismissed the next time the app starts', async () => {
    localStorage.clear();
    await renderApp();
    const note = screen.getByRole('note');
    expect(note).toHaveTextContent(HINT_TEXT['drag-to-queue']);

    await userEvent.setup().click(screen.getByRole('button', { name: 'Got it' }));
    await settle();
    expect(screen.queryByText(HINT_TEXT['drag-to-queue'])).toBeNull();
    // The next hint takes its place.
    expect(screen.getByRole('note')).toHaveTextContent(HINT_TEXT.palette);

    const later = new Hints();
    expect(later.isOpen('drag-to-queue')).toBe(false);
    expect(later.isOpen('palette')).toBe(true);
  });
});
