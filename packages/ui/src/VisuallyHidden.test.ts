import { render, screen } from '@testing-library/svelte';
import { createRawSnippet } from 'svelte';
import { describe, expect, it } from 'vitest';
import { VisuallyHidden } from './index';

const text = (value: string) => createRawSnippet(() => ({ render: () => `<span>${value}</span>` }));

describe('VisuallyHidden', () => {
  it('keeps its text for screen readers while hiding it on screen', () => {
    render(VisuallyHidden, { children: text('Size unit') });
    const shown = screen.getByText('Size unit').parentElement;
    expect(shown?.classList.contains('sr-only')).toBe(true);
    expect(shown?.classList.contains('focus-within:not-sr-only')).toBe(false);
  });

  it('shows itself again while it holds focus when focusable', () => {
    render(VisuallyHidden, { focusable: true, children: text('Skip to the list') });
    const shown = screen.getByText('Skip to the list').parentElement;
    expect(shown?.className).toBe('sr-only focus-within:not-sr-only');
  });
});
