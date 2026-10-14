import { describe, expect, it } from 'vitest';
import { placeFlyout, pointBox } from './flyout';
import {
  listKeyIndex,
  nextEnabledIndex,
  Typeahead,
  typeaheadIndex,
  TYPEAHEAD_RESET_MS,
} from './list-nav';
import { fraction, wholePercent } from './percent';
import { cn } from './cn';
import { closestElement, findElement, findElements } from './dom';
import { textIfCut } from '../attachments/tooltip.svelte';

const items = [
  { label: 'Alpha' },
  { label: 'Beta', disabled: true },
  { label: 'Gamma' },
  { label: 'Delta' },
];

describe('list keys', () => {
  it('skips disabled items and wraps', () => {
    expect(nextEnabledIndex(items, 0, 1)).toBe(2);
    expect(nextEnabledIndex(items, 3, 1)).toBe(0);
    expect(nextEnabledIndex(items, 0, -1)).toBe(3);
    expect(nextEnabledIndex([{ disabled: true }], 0, 1)).toBe(-1);
  });

  it('answers only the keys of its orientation', () => {
    expect(listKeyIndex(items, 0, 'ArrowDown', 'vertical')).toBe(2);
    expect(listKeyIndex(items, 0, 'ArrowRight', 'vertical')).toBeNull();
    expect(listKeyIndex(items, 2, 'ArrowLeft', 'horizontal')).toBe(0);
    expect(listKeyIndex(items, 2, 'Home')).toBe(0);
    expect(listKeyIndex(items, 0, 'End')).toBe(3);
    expect(listKeyIndex(items, 0, 'Enter')).toBeNull();
  });

  it('finds by typed prefix, moving on when the same letter is typed again', () => {
    expect(typeaheadIndex(items, -1, 'g')).toBe(2);
    expect(typeaheadIndex(items, 2, 'd')).toBe(3);
    expect(typeaheadIndex(items, 0, 'a')).toBe(0);
    expect(typeaheadIndex(items, 0, 'be')).toBe(-1);
    expect(typeaheadIndex(items, 0, '')).toBe(-1);
    // The same letter typed again within the pause (`gg`) moves on, as one letter does.
    expect(typeaheadIndex(items, 2, 'gg')).toBe(typeaheadIndex(items, 2, 'g'));
  });

  it('collects a word until a pause', () => {
    const typeahead = new Typeahead();
    expect(typeahead.push('d', 0)).toBe('d');
    expect(typeahead.push('e', 100)).toBe('de');
    expect(typeahead.push('g', 100 + TYPEAHEAD_RESET_MS + 1)).toBe('g');
  });
});

describe('flyout placement', () => {
  const viewport = { width: 800, height: 600 };
  const placement = { side: 'below', align: 'start', gap: 4, inset: 8 } as const;

  it('opens below its anchor, lined up with its start', () => {
    const anchor = { top: 100, bottom: 130, left: 50, right: 150 };
    expect(placeFlyout(anchor, { width: 200, height: 100 }, viewport, placement)).toEqual({
      top: 134,
      left: 50,
      above: false,
    });
  });

  it('flips above when it would leave the window, and stays inside it sideways', () => {
    const anchor = { top: 500, bottom: 530, left: 700, right: 790 };
    expect(placeFlyout(anchor, { width: 200, height: 100 }, viewport, placement)).toEqual({
      top: 396,
      left: 592,
      above: true,
    });
  });

  it('opens at a point for a context menu', () => {
    expect(placeFlyout(pointBox(10, 20), { width: 100, height: 50 }, viewport, placement)).toEqual({
      top: 24,
      left: 10,
      above: false,
    });
  });
});

describe('percent', () => {
  it('clamps and floors, so a bar never reads 100% early', () => {
    expect(fraction(5, 10)).toBe(0.5);
    expect(fraction(15, 10)).toBe(1);
    expect(fraction(1, 0)).toBe(0);
    expect(wholePercent(0.999)).toBe(99);
  });
});

describe('cn', () => {
  it('lets a later class win for the same property, token names included', () => {
    expect(cn('h-8', 'h-control')).toBe('h-control');
    expect(cn('text-ui text-ink', 'text-body')).toBe('text-ink text-body');
    expect(cn('shadow-raised', 'shadow-flyout')).toBe('shadow-flyout');
    expect(cn('z-raised', 'z-popover')).toBe('z-popover');
    expect(cn('a', false, null, undefined, ['b', ['c']])).toBe('a b c');
  });

  it('keeps the scroll anchoring utility beside an overflow, which a later overflow still replaces', () => {
    expect(cn('overflow-y-auto overflow-anchor-none', 'overflow-hidden')).toBe(
      'overflow-anchor-none overflow-hidden',
    );
  });
});

describe('finding elements', () => {
  it('returns only HTML elements, never what a selector merely matches', () => {
    const root = document.createElement('div');
    root.innerHTML =
      '<svg class="hit"><g class="hit"></g></svg><p class="hit"><b class="hit"></b></p>';
    const html = findElements(root, '.hit');
    expect(html.map((e) => e.tagName)).toEqual(['P', 'B']);
    expect(findElement(root, 'svg.hit')).toBeNull();
    expect(findElement(root, '.hit')?.tagName).toBe('P');
    expect(findElement(null, '.hit')).toBeNull();
    expect(findElements(undefined, '.hit')).toEqual([]);
    const inner = root.querySelector('g');
    const bold = root.querySelector('b');
    if (!inner || !bold) throw new Error('the fixture lost its elements');
    expect(closestElement(inner, '.hit')).toBeNull();
    expect(closestElement(bold, 'p')?.tagName).toBe('P');
  });
});

describe('textIfCut', () => {
  function box(
    sizes: Partial<Record<'scrollWidth' | 'clientWidth' | 'scrollHeight' | 'clientHeight', number>>,
  ): HTMLElement {
    const node = document.createElement('span');
    for (const [name, value] of Object.entries(sizes)) {
      Object.defineProperty(node, name, { value });
    }
    return node;
  }

  it('gives the text only while the label is cut, across or below its last line', () => {
    const tip = textIfCut('episode-01-the-wire.mp3');
    expect(
      tip(box({ scrollWidth: 100, clientWidth: 100, scrollHeight: 36, clientHeight: 36 })),
    ).toBeNull();
    expect(tip(box({ scrollWidth: 180, clientWidth: 100 }))).toBe('episode-01-the-wire.mp3');
    // A label clamped to two lines is cut below them, not across.
    expect(
      tip(box({ scrollWidth: 100, clientWidth: 100, scrollHeight: 54, clientHeight: 36 })),
    ).toBe('episode-01-the-wire.mp3');
  });
});
