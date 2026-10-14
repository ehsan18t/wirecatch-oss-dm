import { screen, within } from '@testing-library/svelte';
import userEvent from '@testing-library/user-event';
import { describe, expect, it } from 'vitest';
import { renderApp, rowByName, settle } from '$lib/testing/setup';

async function setup() {
  const r = await renderApp();
  const user = userEvent.setup();
  const press = async (keys: string) => {
    await user.keyboard(keys);
    await settle();
  };
  return { ...r, user, press };
}

const grid = () => screen.getByRole('grid', { name: 'Downloads' });
const panel = (name: RegExp | string) => screen.getByRole('dialog', { name });

describe('the details panel', () => {
  it('slides in for the keyboard row with Alt+Enter, takes the focus, and gives it back on Escape', async () => {
    const { app, press } = await setup();
    grid().focus();
    await press('{ArrowDown}');
    const row = app.selection.rows[0]!;
    await press('{Alt>}{Enter}{/Alt}');
    const details = panel(row.file_name);
    expect(details.contains(document.activeElement)).toBe(true);
    // Non-modal: the list beside it stays usable.
    expect(details).not.toHaveAttribute('aria-modal');
    await press('{Escape}');
    expect(screen.queryByRole('dialog')).toBeNull();
    expect(document.activeElement).toBe(grid());
    // Escape closed the details first; the selection they showed is still there.
    expect(app.selection.list).toEqual([row.id]);
    await press('{Escape}');
    expect(app.selection.size).toBe(0);
  });

  it('follows the selection: another row, several, or none', async () => {
    const { app, user } = await setup();
    await user.click(
      within(rowByName('LibreOffice')).getByRole('button', { name: 'Show details' }),
    );
    await settle();
    expect(panel(/^LibreOffice/)).toBeInTheDocument();
    await user.click(rowByName('ubuntu-24.04.3'));
    await settle();
    expect(panel(/^ubuntu-24.04.3/)).toBeInTheDocument();
    await user.keyboard('{Shift>}');
    await user.click(rowByName('LibreOffice'));
    await user.keyboard('{/Shift}');
    await settle();
    const several = panel(`${String(app.selection.size)} downloads selected`);
    expect(within(several).getByRole('combobox', { name: 'Move to queue' })).toBeInTheDocument();
    expect(
      within(several).getByRole('button', {
        name: `Remove ${String(app.selection.size)} downloads`,
      }),
    ).toBeInTheDocument();
    app.selection.clear();
    await settle();
    expect(panel('Details')).toHaveTextContent('Select a download');
  });

  it('shows what the old detail panel did: progress, queue, connections, the whole file path, link, dates', async () => {
    const { app, user } = await setup();
    await user.click(
      within(rowByName('ubuntu-24.04.3')).getByRole('button', { name: 'Show details' }),
    );
    await settle();
    const details = within(panel(/^ubuntu-24.04.3/));
    expect(details.getByRole('progressbar', { name: /^Progress of ubuntu/ })).toBeInTheDocument();
    for (const label of ['Queue', 'Connections', 'Saved as', 'Link', 'Added']) {
      expect(details.getByText(label)).toBeInTheDocument();
    }
    // The title may be cut in a narrow panel; the file's whole name is always there, in its path.
    const row = app.selection.rows[0]!;
    expect(details.getByText(row.path)).toBeInTheDocument();
    for (const name of [
      'Pause',
      'Show in folder',
      'Show progress window',
      'Remove',
      'Change folder',
      'Copy link',
      'Cancel and discard the partial file',
    ]) {
      expect(details.getByRole('button', { name })).toBeInTheDocument();
    }
    // A download that is moving along is not asked whether its link expired.
    expect(details.queryByText('Link expired?')).toBeNull();
  });

  it('asks whether the link expired only where it may have: a paused download', async () => {
    const { user } = await setup();
    await user.click(
      within(rowByName('android-studio')).getByRole('button', { name: 'Show details' }),
    );
    await settle();
    const details = within(panel(/^android-studio/));
    expect(details.getByText('Link expired?')).toBeInTheDocument();
    expect(details.getByRole('button', { name: 'Get a new link' })).toBeInTheDocument();
  });

  it('closes with its own button and with the Details button above the list', async () => {
    const { app, user } = await setup();
    await user.click(rowByName('LibreOffice'));
    await user.click(screen.getByRole('button', { name: 'Details' }));
    await settle();
    expect(app.detailsOpen).toBe(true);
    expect(screen.getByRole('button', { name: 'Details' })).toHaveAttribute('aria-pressed', 'true');
    await user.click(within(panel(/^LibreOffice/)).getByRole('button', { name: 'Close' }));
    await settle();
    expect(app.detailsOpen).toBe(false);
  });
});
