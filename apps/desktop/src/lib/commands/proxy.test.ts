// The palette switches the app's proxy (D73.4): one command for each proxy it is not using.

import { fireEvent, screen } from '@testing-library/svelte';
import { describe, expect, it } from 'vitest';
import { renderApp, settle } from '$lib/testing/setup';

describe('the palette’s proxy commands', () => {
  it('offers each proxy the app is not using, and switches to it', async () => {
    const { app } = await renderApp();
    app.paletteOpen = true;
    await settle();
    const search = screen.getByRole('combobox');
    await fireEvent.input(search, { target: { value: 'for the app' } });
    await settle();
    expect(screen.queryByRole('option', { name: /the computer’s proxy for the app/ })).toBeNull();
    expect(screen.getByRole('option', { name: /no proxy for the app/ })).toBeInTheDocument();
    await fireEvent.click(screen.getByRole('option', { name: /Use the proxy Office for the app/ }));
    await settle();
    expect(app.settings?.proxy.proxy).toEqual({ kind: 'profile', id: app.proxies.list[0]?.id });
  });
});
