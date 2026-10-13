// The detail panel says how many connections a download may open only from the settings it has.

import { screen } from '@testing-library/svelte';
import { describe, expect, it } from 'vitest';
import { renderApp, settle } from '$lib/testing/setup';

/** Not the connections of a fresh install, so a default shown in their place would differ. */
const CHOSEN_CONNECTIONS = 16;

/** What the panel says under Connections. */
function connections(): string {
  return screen.getByText('Connections').nextElementSibling?.textContent.trim() ?? '';
}

describe('the detail panel', () => {
  it('shows the connection limit from the settings, and none before they are known', async () => {
    const { app } = await renderApp();
    const first = app.visible[0];
    if (first === undefined) throw new Error('the sample list is empty');
    app.selection.only(first);
    const settings = app.settings;
    if (!settings) throw new Error('the settings did not load');
    app.settings = { ...settings, connections_per_download: CHOSEN_CONNECTIONS };
    await settle();
    expect(connections()).toMatch(new RegExp(`up to ${String(CHOSEN_CONNECTIONS)}$`, 'i'));

    app.settings = null;
    await settle();
    expect(connections()).not.toMatch(/up to/i);
  });
});
