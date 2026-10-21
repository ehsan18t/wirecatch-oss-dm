import { screen, within } from '@testing-library/svelte';
import userEvent from '@testing-library/user-event';
import { describe, expect, it } from 'vitest';
import { renderApp, settle } from '$lib/testing/setup';

const main = () => screen.getByRole('main', { name: 'Downloads' });

describe('the list header', () => {
  it('says what is happening now: how many download, how fast, and about how long', async () => {
    await renderApp();
    expect(within(main()).getByText(/^\d+ downloading at .+ · about .+ left$/)).toBeInTheDocument();
  });

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

  it('offers only New download on first run, where there is nothing else to act on', async () => {
    await renderApp({ empty: true });
    const tools = within(main()).getByRole('group', { name: 'List tools' });
    expect(
      within(tools)
        .getAllByRole('button')
        .map((b) => b.textContent.trim()),
    ).toEqual(['New download']);
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
