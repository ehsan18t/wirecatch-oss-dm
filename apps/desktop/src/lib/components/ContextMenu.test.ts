// The right-click menu stays inside the window whatever its real size, and the keyboard opens it
// under the active row's name.

import { fireEvent, screen } from '@testing-library/svelte';
import { describe, expect, it, vi } from 'vitest';
import { renderApp, rowByName, settle } from '$lib/testing/setup';

/** A menu wider and taller than any fixed guess, as long queue names make it. */
const MENU = { width: 260, height: 300 };
const EDGE = 4;

/** jsdom does no layout; give the menu the size a browser would. */
function measureMenusAs(size: { width: number; height: number }) {
  const sized = (dimension: 'width' | 'height') =>
    function (this: HTMLElement) {
      return this.getAttribute('role') === 'menu' ? size[dimension] : 0;
    };
  vi.spyOn(HTMLElement.prototype, 'offsetWidth', 'get').mockImplementation(sized('width'));
  vi.spyOn(HTMLElement.prototype, 'offsetHeight', 'get').mockImplementation(sized('height'));
}

describe('the right-click menu', () => {
  it('keeps its measured box inside the window near the bottom right corner', async () => {
    measureMenusAs(MENU);
    await renderApp();
    await fireEvent.contextMenu(rowByName('LibreOffice'), {
      clientX: window.innerWidth - 1,
      clientY: window.innerHeight - 1,
    });
    await settle();
    const menu = screen.getByRole('menu');
    expect(menu.style.left).toBe(`${window.innerWidth - MENU.width - EDGE}px`);
    expect(menu.style.top).toBe(`${window.innerHeight - MENU.height - EDGE}px`);
  });

  it('opens from the keyboard under the active row, lined up with its name', async () => {
    await renderApp();
    const row = rowByName('LibreOffice');
    const name = row.querySelector('[data-menu-anchor]');
    if (!name) throw new Error('the row has no name cell');
    vi.spyOn(row, 'getBoundingClientRect').mockReturnValue(new DOMRect(10, 100, 900, 44));
    vi.spyOn(name, 'getBoundingClientRect').mockReturnValue(new DOMRect(70, 112, 300, 20));
    await fireEvent.click(row);
    await fireEvent.keyDown(row, { key: 'ContextMenu' });
    await settle();
    const menu = screen.getByRole('menu');
    expect(menu.style.left).toBe('70px');
    expect(menu.style.top).toBe('144px');
  });
});
