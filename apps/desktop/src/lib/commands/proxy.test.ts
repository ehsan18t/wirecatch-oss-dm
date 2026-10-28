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
    await fireEvent.input(search, { target: { value: 'proxy' } });
    await settle();
    expect(screen.queryByRole('option', { name: /use the computer’s proxy/ })).toBeNull();
    expect(
      screen.getByRole('option', { name: /the app uses the computer’s proxy now/ }),
    ).toBeInTheDocument();
    expect(screen.getByRole('option', { name: /turn it off for the app/ })).toBeInTheDocument();
    await fireEvent.click(screen.getByRole('option', { name: /use “Office” for the app/ }));
    await settle();
    expect(app.settings?.proxy.proxy).toEqual({ kind: 'profile', id: app.proxies.list[0]?.id });
  });
});
