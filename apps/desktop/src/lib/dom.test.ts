import { describe, expect, it } from 'vitest';
import { dataIdAt, findAll, findElement } from './dom';

function page(html: string): HTMLElement {
  const root = document.createElement('div');
  root.innerHTML = html;
  return root;
}

describe('finding elements', () => {
  it('reads a row id from inside the row, and only a whole number', () => {
    const root = page(
      '<div data-id="7"><span id="in">a</span></div><div data-id="x"><span id="bad">b</span></div>',
    );
    expect(dataIdAt(root.querySelector('#in'))).toBe(7);
    expect(dataIdAt(root.querySelector('#bad'))).toBeNull();
    expect(dataIdAt(root)).toBeNull();
    for (const id of ['', ' 7 ', '0x10', '1e3', '-3', '7.5']) {
      const row = page(`<div data-id="${id}"><span>a</span></div>`).querySelector('span');
      expect(dataIdAt(row)).toBeNull();
    }
    expect(dataIdAt(null)).toBeNull();
  });

  it('finds HTML elements and elements of a type', () => {
    const root = page('<svg><g class="x"></g></svg><button class="x">b</button>');
    expect(findElement(root, 'g.x')).toBeNull();
    expect(findElement(root, 'button')?.textContent).toBe('b');
    expect(findAll(root, '.x', HTMLButtonElement)).toHaveLength(1);
    expect(findAll(null, '.x', HTMLButtonElement)).toEqual([]);
  });
});
