// Acceptance item 3: the interface works with the keyboard alone, and has no automatically
// detectable accessibility problems. (Color contrast is checked from the tokens in
// contrast.test.ts and in a real browser by e2e/a11y.spec.ts; jsdom has no layout.)

import { screen, within } from '@testing-library/svelte';
import userEvent from '@testing-library/user-event';
import axe from 'axe-core';
import { describe, expect, it } from 'vitest';
import { renderApp, settle } from './testing/setup';

async function setup() {
  const r = await renderApp();
  const user = userEvent.setup();
  const press = async (keys: string) => {
    await user.keyboard(keys);
    await settle();
  };
  return { ...r, user, press };
}

async function axeViolations(): Promise<string[]> {
  const result = await axe.run(document.body, {
    rules: { 'color-contrast': { enabled: false } },
    resultTypes: ['violations'],
  });
  return result.violations.map(
    (v) => `${v.id}: ${v.nodes.map((n) => n.target.join(' ')).join(', ')}`,
  );
}

describe('keyboard only', () => {
  it('moves through the list, selects, pauses and removes', async () => {
    const { app, press } = await setup();
    const list = screen.getByRole('grid', { name: 'Downloads' });
    list.focus();
    await press('{ArrowDown}');
    const first = app.visible[0];
    expect(app.selection.active).toBe(first);
    expect(list).toHaveAttribute('aria-activedescendant', `row-${first}`);
    await press('{Shift>}{ArrowDown}{ArrowDown}{/Shift}');
    expect(app.selection.size).toBe(3);
    await press('{End}');
    expect(app.selection.active).toBe(app.visible.at(-1));
    await press('{Home}');
    await press(' ');
    expect(app.selection.rows[0]?.status).toBe('paused');
    await press('{Delete}');
    expect(within(screen.getByRole('dialog')).getByText(/^Remove "/)).toBeInTheDocument();
    await press('{Escape}');
    expect(screen.queryByRole('dialog')).not.toBeInTheDocument();
    expect(document.activeElement).toBe(list);
    await press('{Control>}a{/Control}');
    expect(app.selection.size).toBe(app.visible.length);
    await press('{Escape}');
    expect(app.selection.size).toBe(0);
  });

  it('reorders inside a queue with Alt and the arrow keys', async () => {
    const { app, press } = await setup();
    app.show({ kind: 'queue', id: 1 });
    await settle();
    const list = screen.getByRole('grid', { name: 'Downloads' });
    list.focus();
    await press('{ArrowDown}{ArrowDown}');
    const moving = app.selection.active;
    expect(app.downloads.idsInQueue(1).indexOf(moving!)).toBe(1);
    await press('{Alt>}{ArrowUp}{/Alt}');
    expect(app.downloads.idsInQueue(1).indexOf(moving!)).toBe(0);
    await press('{Alt>}{ArrowDown}{ArrowDown}{/Alt}');
    expect(app.downloads.idsInQueue(1).indexOf(moving!)).toBe(2);
  });

  it('runs any action from the command palette', async () => {
    const { app, press } = await setup();
    await press('{Control>}k{/Control}');
    const input = screen.getByRole('combobox', { name: 'Type an action or setting' });
    expect(document.activeElement).toBe(input);
    await press('dark theme');
    await press('{Enter}');
    expect(app.paletteOpen).toBe(false);
    expect(app.settings?.theme).toBe('dark');
    expect(document.documentElement.dataset.theme).toBe('dark');
  });

  it('jumps from the palette straight to a setting and focuses it', async () => {
    const { app, press } = await setup();
    await press('{Control>}k{/Control}');
    await press('window before');
    const option = screen
      .getAllByRole('option')
      .find((o) => o.textContent.includes('Browser: Show a window before each download'));
    expect(option).toBeDefined();
    while (
      screen
        .getByRole('combobox', { name: 'Type an action or setting' })
        .getAttribute('aria-activedescendant') !== option?.id
    ) {
      await press('{ArrowDown}');
    }
    await press('{Enter}');
    expect(app.screen).toBe('settings');
    const toggle = screen.getByRole('switch', { name: 'Show a window before each download' });
    expect(document.activeElement).toBe(toggle);
    await press(' ');
    expect(app.settings?.extension.ask_before_download).toBe(false);
  });

  it('has shortcuts for add, settings, search and help', async () => {
    const { app, press } = await setup();
    await press('{Control>}n{/Control}');
    expect(app.dialog?.kind).toBe('add');
    await press('{Escape}');
    await press('{F1}');
    expect(app.dialog?.kind).toBe('shortcuts');
    await press('{Escape}');
    await press('{Control>}f{/Control}');
    expect(document.activeElement).toBe(
      screen.getByRole('searchbox', { name: 'Search downloads' }),
    );
    await press('blender');
    expect(app.visible.length).toBe(1);
    await press('{Escape}');
    expect(app.search).toBe('');
    await press('{Control>},{/Control}');
    expect(app.screen).toBe('settings');
    await press('{Escape}');
    expect(app.screen).toBe('downloads');
  });

  it('lets the theme be picked with arrow keys', async () => {
    const { app, press } = await setup();
    app.openSettings('theme');
    await settle();
    const group = screen.getByRole('radiogroup', { name: 'Appearance' });
    within(group).getByRole('radio', { name: 'Match system' }).focus();
    await press('{ArrowRight}');
    expect(app.settings?.theme).toBe('light');
    expect(document.activeElement).toBe(within(group).getByRole('radio', { name: 'Light' }));
  });
});

describe('automated accessibility checks (axe)', { timeout: 60_000 }, () => {
  it('main window, with a failed download selected', async () => {
    const { app } = await setup();
    const failed = app.visible.find((id) => app.downloads.get(id)?.status === 'failed');
    if (failed !== undefined) app.selection.only(failed);
    await settle();
    expect(await axeViolations()).toEqual([]);
  });

  it('settings', async () => {
    const { app } = await setup();
    app.openSettings();
    await settle();
    expect(await axeViolations()).toEqual([]);
  });

  it('command palette, dialogs, toast and confirmation', async () => {
    const { app, backend } = await setup();
    app.paletteOpen = true;
    await settle();
    expect(await axeViolations()).toEqual([]);
    app.paletteOpen = false;
    for (const d of [
      { kind: 'add', urls: ['https://example.com/a.zip'] },
      { kind: 'queue', id: 2 },
      { kind: 'export' },
      { kind: 'shortcuts' },
    ] as const) {
      app.openDialog(d.kind === 'add' ? { kind: 'add', urls: [...d.urls] } : d);
      await settle();
      expect(await axeViolations()).toEqual([]);
      app.closeDialog();
      await settle();
    }
    backend.simulateCapture();
    backend.simulateConfirmation();
    await settle();
    expect(await axeViolations()).toEqual([]);
  });

  it('first run', async () => {
    await renderApp({ empty: true });
    expect(screen.getByText(/then press/)).toBeInTheDocument();
    expect(await axeViolations()).toEqual([]);
  });
});
