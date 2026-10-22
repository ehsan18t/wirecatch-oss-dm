import { screen, within } from '@testing-library/svelte';
import userEvent from '@testing-library/user-event';
import { describe, expect, it } from 'vitest';
import { renderApp, settle } from '$lib/testing/setup';

const main = () => screen.getByRole('main', { name: 'Downloads' });

describe('the list header', () => {
  it('says what is happening now: how many download, about how long, and how many are next', async () => {
    await renderApp();
    // The speed is the title bar's to say.
    expect(
      within(main()).getByText(/^\d+ downloading · about .+ left · \d+ up next$/),
    ).toBeInTheDocument();
  });

  it('says how many downloads match a search, in words spaced as written', async () => {
    const { app } = await renderApp();
    app.search = 'zzzz';
    await settle();
    expect(
      within(main())
        .getByText(/downloads? matching/)
        .textContent.trim(),
    ).toBe('0 downloads matching “zzzz”');
  });

  it('leaves New download to the empty list on first run, so the window offers it once', async () => {
    await renderApp({ empty: true });
    const tools = within(main()).getByRole('group', { name: 'List tools' });
    expect(within(tools).queryAllByRole('button')).toEqual([]);
    expect(within(main()).getAllByRole('button', { name: 'New download' })).toHaveLength(1);
    expect(within(main()).queryByText(/^0 downloads/)).toBeNull();
    expect(within(main()).getByRole('heading', { level: 1 })).toBeInTheDocument();
  });

  it('keeps its tools when a filter, not the list, is empty', async () => {
    const { app } = await renderApp();
    const user = userEvent.setup();
    app.search = 'zzzz';
    await settle();
    expect(within(main()).getByRole('group', { name: 'List tools' })).toBeInTheDocument();
    await user.click(within(main()).getByRole('button', { name: 'New download' }));
    expect(app.dialog).toEqual({ kind: 'add', urls: [] });
  });
});
