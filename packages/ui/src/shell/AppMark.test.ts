import { render } from '@testing-library/svelte';
import { describe, expect, it } from 'vitest';
import AppMark from './AppMark.svelte';

describe('AppMark', () => {
  it('is decoration sized from the tokens', () => {
    const { container } = render(AppMark, { size: 'lg' });
    const svg = container.querySelector('svg');
    expect(svg?.getAttribute('aria-hidden')).toBe('true');
    expect(svg?.getAttribute('class')).toContain('size-icon-lg');
  });
});
