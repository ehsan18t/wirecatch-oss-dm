import { fireEvent, render, screen } from '@testing-library/svelte';
import userEvent from '@testing-library/user-event';
import { describe, expect, it, vi } from 'vitest';
import ContextMenuHost from '../testing/ContextMenuHost.svelte';
import DialogHost from '../testing/DialogHost.svelte';
import MenuHost from '../testing/MenuHost.svelte';
import PopoverHost from '../testing/PopoverHost.svelte';
import SlideOverHost from '../testing/SlideOverHost.svelte';
import Dialog from './Dialog.svelte';

describe('Dialog', () => {
  it('starts on its panel when Close is all there is to press, so Close opens without a ring', () => {
    render(Dialog, { open: true, title: 'Keyboard shortcuts' });
    const dialog = screen.getByRole('dialog', { name: 'Keyboard shortcuts' });
    expect(document.activeElement).not.toBe(screen.getByRole('button', { name: 'Close' }));
    expect(dialog.contains(document.activeElement)).toBe(true);
  });

  async function open(dismissible = true) {
    render(DialogHost, { dismissible });
    const opener = screen.getByRole('button', { name: 'Remove' });
    await userEvent.click(opener);
    return { opener, dialog: screen.getByRole('dialog', { name: 'Remove 3 downloads?' }) };
  }

  it('is a named, described modal dialog that starts focus on its first control', async () => {
    const { dialog } = await open();
    expect(dialog.getAttribute('aria-modal')).toBe('true');
    expect(dialog.getAttribute('aria-describedby')).toBeTruthy();
    expect(document.activeElement).toBe(screen.getByRole('textbox', { name: 'Reason' }));
  });

  it('closes on Escape and gives focus back to its opener', async () => {
    const { opener } = await open();
    await userEvent.keyboard('{Escape}');
    expect(screen.queryByRole('dialog')).toBeNull();
    expect(document.activeElement).toBe(opener);
  });

  it('keeps Tab inside, wrapping at both ends', async () => {
    const { dialog } = await open();
    const close = screen.getByRole('button', { name: 'Close' });
    close.focus();
    await userEvent.tab();
    expect(document.activeElement).toBe(screen.getByRole('textbox', { name: 'Reason' }));
    await userEvent.tab({ shift: true });
    expect(document.activeElement).toBe(close);
    expect(dialog.contains(document.activeElement)).toBe(true);
  });

  it('closes only the topmost of two dialogs on Escape', async () => {
    await open();
    await userEvent.click(screen.getByRole('button', { name: 'Details' }));
    expect(screen.getByRole('dialog', { name: 'Details' })).toBeTruthy();
    await userEvent.keyboard('{Escape}');
    expect(screen.queryByRole('dialog', { name: 'Details' })).toBeNull();
    expect(screen.getByRole('dialog', { name: 'Remove 3 downloads?' })).toBeTruthy();
  });

  it('follows a native close, such as a form with method="dialog", and opens again', async () => {
    const { dialog } = await open();
    dialog.removeAttribute('open');
    await fireEvent(dialog, new Event('close'));
    expect(screen.queryByRole('dialog')).toBeNull();
    await userEvent.click(screen.getByRole('button', { name: 'Remove' }));
    expect(screen.getByRole('dialog', { name: 'Remove 3 downloads?' })).toBeTruthy();
  });

  it('stays open on Escape when it must be answered', async () => {
    await open(false);
    await userEvent.keyboard('{Escape}');
    expect(screen.getByRole('dialog')).toBeTruthy();
    expect(screen.queryByRole('button', { name: 'Close' })).toBeNull();
  });
});

describe('SlideOver', () => {
  it('takes focus as it opens, and Escape from anywhere closes it and returns focus', async () => {
    render(SlideOverHost);
    const opener = screen.getByRole('button', { name: 'Show details' });
    await userEvent.click(opener);
    const panel = screen.getByRole('dialog', { name: 'ubuntu.iso' });
    expect(document.activeElement).toBe(panel);
    await userEvent.keyboard('{Escape}');
    expect(screen.queryByRole('dialog')).toBeNull();
    expect(document.activeElement).toBe(opener);
  });

  it('leaves the content beside it usable, and Escape there closes it too', async () => {
    render(SlideOverHost);
    await userEvent.click(screen.getByRole('button', { name: 'Show details' }));
    const search = screen.getByRole('textbox', { name: 'Search' });
    await userEvent.click(search);
    expect(document.activeElement).toBe(search);
    await userEvent.keyboard('{Escape}');
    expect(screen.queryByRole('dialog')).toBeNull();
  });
});

describe('SlideOver beside the content', () => {
  it('is a column of its own, non-modal, that takes focus and closes on Escape from anywhere', async () => {
    render(SlideOverHost, { docked: true });
    const opener = screen.getByRole('button', { name: 'Show details' });
    await userEvent.click(opener);
    const panel = screen.getByRole('dialog', { name: 'ubuntu.iso' });
    expect(panel.getAttribute('aria-modal')).toBeNull();
    expect(panel.classList.contains('absolute')).toBe(false);
    expect(document.activeElement).toBe(panel);
    await userEvent.click(screen.getByRole('textbox', { name: 'Search' }));
    await userEvent.keyboard('{Escape}');
    expect(screen.queryByRole('dialog')).toBeNull();
  });
});

describe('SlideOver over a veil', () => {
  async function open() {
    render(SlideOverHost, { modal: true });
    const opener = screen.getByRole('button', { name: 'Show details' });
    await userEvent.click(opener);
    // A modal dialog hides the page from the accessibility tree, as the browser does.
    const dialog = document.querySelector('dialog');
    if (!dialog) throw new Error('no dialog');
    return { opener, dialog };
  }

  it('takes focus into the panel, closes on Escape from there and gives focus back', async () => {
    const { opener, dialog } = await open();
    expect(document.activeElement?.closest('section')?.parentElement).toBe(dialog);
    await userEvent.keyboard('{Escape}');
    expect(document.querySelector('dialog')).toBeNull();
    expect(document.activeElement).toBe(opener);
  });

  it('keeps Tab inside the panel, wrapping at both ends', async () => {
    const { dialog } = await open();
    const close = screen.getByRole('button', { name: 'Close' });
    const folder = screen.getByRole('button', { name: 'Open folder' });
    folder.focus();
    await userEvent.tab();
    expect(document.activeElement).toBe(close);
    await userEvent.tab({ shift: true });
    expect(document.activeElement).toBe(folder);
    expect(dialog.contains(document.activeElement)).toBe(true);
  });

  it('follows a native close and opens again', async () => {
    const { dialog } = await open();
    dialog.removeAttribute('open');
    await fireEvent(dialog, new Event('close'));
    expect(document.querySelector('dialog')).toBeNull();
    await userEvent.click(screen.getByRole('button', { name: 'Show details' }));
    expect(document.querySelector('dialog')?.hasAttribute('open')).toBe(true);
  });
});

describe('Menu', () => {
  async function open() {
    const onselect = vi.fn();
    render(MenuHost, { onselect });
    const trigger = screen.getByRole('button', { name: 'More' });
    trigger.focus();
    await userEvent.keyboard('{ArrowDown}');
    return { onselect, trigger };
  }

  it('opens from its button with focus on the first item, and says it is open', async () => {
    const { trigger } = await open();
    expect(trigger.getAttribute('aria-haspopup')).toBe('menu');
    expect(trigger.getAttribute('aria-expanded')).toBe('true');
    expect(screen.getByRole('menu', { name: 'Download actions' })).toBeTruthy();
    expect(document.activeElement).toBe(screen.getByRole('menuitem', { name: 'Pause' }));
  });

  it('moves past disabled items and separators, and chooses with Enter', async () => {
    const { onselect, trigger } = await open();
    await userEvent.keyboard('{ArrowDown}');
    expect(document.activeElement).toBe(screen.getByRole('menuitem', { name: 'Copy link' }));
    await userEvent.keyboard('{Enter}');
    expect(onselect).toHaveBeenCalledWith('copy');
    expect(screen.queryByRole('menu')).toBeNull();
    expect(document.activeElement).toBe(trigger);
  });

  it('finds an item by its first letter and closes on Escape', async () => {
    const { onselect, trigger } = await open();
    await userEvent.keyboard('r');
    expect(document.activeElement).toBe(screen.getByRole('menuitem', { name: 'Remove' }));
    await userEvent.keyboard('{Escape}');
    expect(screen.queryByRole('menu')).toBeNull();
    expect(onselect).not.toHaveBeenCalled();
    expect(document.activeElement).toBe(trigger);
  });

  it('moves on to the next item with the same letter when it is typed again', async () => {
    await open();
    await userEvent.keyboard('s');
    expect(document.activeElement).toBe(screen.getByRole('menuitem', { name: 'Show in folder' }));
    await userEvent.keyboard('s');
    expect(document.activeElement).toBe(screen.getByRole('menuitem', { name: 'Show speed' }));
    await userEvent.keyboard('s');
    expect(document.activeElement).toBe(screen.getByRole('menuitem', { name: 'Show in folder' }));
  });

  it('names an item by its label alone and gives its shortcut as aria-keyshortcuts', async () => {
    await open();
    const copy = screen.getByRole('menuitem', { name: 'Copy link' });
    expect(copy.getAttribute('aria-keyshortcuts')).toBe('Control+C');
  });

  it('closes on Tab, and focus moves on from its button', async () => {
    await open();
    await userEvent.tab();
    expect(screen.queryByRole('menu')).toBeNull();
    expect(document.activeElement).toBe(screen.getByRole('button', { name: 'Elsewhere' }));
  });

  it('opens at the last item with the up arrow', async () => {
    render(MenuHost, { onselect: vi.fn() });
    screen.getByRole('button', { name: 'More' }).focus();
    await userEvent.keyboard('{ArrowUp}');
    expect(document.activeElement).toBe(screen.getByRole('menuitem', { name: 'Remove' }));
  });

  it('closes on a press elsewhere', async () => {
    await open();
    await userEvent.click(screen.getByRole('button', { name: 'Elsewhere' }));
    expect(screen.queryByRole('menu')).toBeNull();
  });
});

describe('ContextMenu', () => {
  it('opens on right-click and from the keyboard', async () => {
    const onselect = vi.fn();
    render(ContextMenuHost, { onselect });
    const row = screen.getByRole('button', { name: 'ubuntu.iso' });
    await fireEvent.contextMenu(row, { clientX: 40, clientY: 50 });
    expect(screen.getByRole('menu', { name: 'Download' })).toBeTruthy();
    await userEvent.keyboard('{Escape}');
    expect(screen.queryByRole('menu')).toBeNull();
    row.focus();
    await userEvent.keyboard('{Shift>}{F10}{/Shift}');
    expect(document.activeElement).toBe(screen.getByRole('menuitem', { name: 'Open' }));
    await userEvent.keyboard('{ArrowDown}{Enter}');
    expect(onselect).toHaveBeenCalledWith('show');
    expect(document.activeElement).toBe(row);
  });

  it('opens from the keyboard about the row a grid points at, and gives focus back to the grid', async () => {
    const onopen = vi.fn<(target: Element) => boolean | undefined>(() => undefined);
    render(ContextMenuHost, { onselect: vi.fn(), onopen });
    const grid = screen.getByRole('grid', { name: 'Files' });
    grid.focus();
    await userEvent.keyboard('{Shift>}{F10}{/Shift}');
    expect(onopen).toHaveBeenCalledWith(document.getElementById('file-2'));
    await userEvent.keyboard('{Escape}');
    expect(document.activeElement).toBe(grid);
  });

  it('closes on Tab, and focus moves on from where it was opened', async () => {
    render(ContextMenuHost, { onselect: vi.fn() });
    const row = screen.getByRole('button', { name: 'ubuntu.iso' });
    row.focus();
    await userEvent.keyboard('{Shift>}{F10}{/Shift}');
    await userEvent.tab();
    expect(screen.queryByRole('menu')).toBeNull();
    expect(document.activeElement).toBe(screen.getByRole('button', { name: 'After' }));
  });
});

describe('Popover', () => {
  it('moves focus in, closes on Escape back to its button, and through its own close', async () => {
    render(PopoverHost);
    const trigger = screen.getByRole('button', { name: 'Limit' });
    await userEvent.click(trigger);
    expect(screen.getByRole('dialog', { name: 'Speed limit' })).toBeTruthy();
    expect(document.activeElement).toBe(
      screen.getByRole('textbox', { name: 'Kilobytes per second' }),
    );
    await userEvent.keyboard('{Escape}');
    expect(screen.queryByRole('dialog')).toBeNull();
    expect(document.activeElement).toBe(trigger);
    await userEvent.click(trigger);
    await userEvent.click(screen.getByRole('button', { name: 'Done' }));
    expect(screen.queryByRole('dialog')).toBeNull();
  });
});
