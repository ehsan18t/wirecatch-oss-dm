import { screen, within } from '@testing-library/svelte';
import userEvent from '@testing-library/user-event';
import { describe, expect, it } from 'vitest';
import { renderApp, settle } from '$lib/testing/setup';

const main = () => screen.getByRole('main', { name: 'Downloads' });

describe('the list header', () => {
  it('says how many downloads match a search, in words spaced as written', async () => {
    const { app } = await renderApp();
    app.search = 'zzzz';
    await settle();
    expect(
      within(main())
        .getByText(/downloads? matching/)
        .textContent.trim(),
    ).toBe('0 downloads matching "zzzz"');
  });

  it('leaves out its tools and count on first run, where there is nothing to act on', async () => {
    await renderApp({ empty: true });
    expect(within(main()).queryByRole('group', { name: 'List tools' })).toBeNull();
    expect(within(main()).queryByText(/^0 downloads/)).toBeNull();
    expect(within(main()).getByRole('heading', { level: 1 })).toBeInTheDocument();
  });

  it('keeps its tools when a filter, not the list, is empty', async () => {
    const { app } = await renderApp();
    const user = userEvent.setup();
    app.search = 'zzzz';
    await settle();
    expect(within(main()).getByRole('group', { name: 'List tools' })).toBeInTheDocument();
    await user.click(within(main()).getByRole('button', { name: 'Details' }));
    expect(app.detailsOpen).toBe(true);
  });
});
