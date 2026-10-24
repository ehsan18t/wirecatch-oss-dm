import Download from '~icons/lucide/download';
import { render, screen } from '@testing-library/svelte';
import userEvent from '@testing-library/user-event';
import { createRawSnippet } from 'svelte';
import { afterEach, describe, expect, it, vi } from 'vitest';
import Button from './Button.svelte';
import IconButton from './IconButton.svelte';

const text = (value: string) => createRawSnippet(() => ({ render: () => `<span>${value}</span>` }));

describe('Button', () => {
  it('is a plain button that runs its action', async () => {
    const onclick = vi.fn();
    render(Button, { icon: Download, onclick, children: text('Download') });
    const button = screen.getByRole('button', { name: 'Download' });
    expect(button.getAttribute('type')).toBe('button');
    expect(button.querySelector('svg')?.getAttribute('aria-hidden')).toBe('true');
    await userEvent.click(button);
    expect(onclick).toHaveBeenCalledOnce();
  });

  it('stays focusable and says why when disabled with a reason', async () => {
    const onclick = vi.fn();
    render(Button, {
      disabled: true,
      tooltip: 'Nothing is selected',
      onclick,
      children: text('Remove'),
    });
    const button = screen.getByRole('button', { name: 'Remove' });
    expect(button.hasAttribute('disabled')).toBe(false);
    expect(button.getAttribute('aria-disabled')).toBe('true');
    const describedBy = button.getAttribute('aria-describedby') ?? '';
    expect(document.getElementById(describedBy)?.textContent).toBe('Nothing is selected');
    await userEvent.click(button);
    expect(onclick).not.toHaveBeenCalled();
  });

  it('is disabled outright without a reason', () => {
    render(Button, { disabled: true, children: text('Remove') });
    expect(screen.getByRole('button', { name: 'Remove' }).hasAttribute('disabled')).toBe(true);
  });

  it('shows a spinner and refuses clicks while loading', async () => {
    const onclick = vi.fn();
    render(Button, { loading: true, onclick, children: text('Saving') });
    const button = screen.getByRole('button', { name: 'Saving' });
    expect(button.getAttribute('aria-busy')).toBe('true');
    await userEvent.click(button);
    expect(onclick).not.toHaveBeenCalled();
  });
});

describe('a button described by its caller', () => {
  afterEach(() => {
    document.getElementById('site-help')?.remove();
  });

  function hint(): HTMLElement {
    const p = document.createElement('p');
    p.id = 'site-help';
    p.textContent = 'Downloads from example.com go to Wirecatch.';
    document.body.append(p);
    return p;
  }

  it("keeps the caller's description, and adds the disabled reason after it", async () => {
    hint();
    const props = {
      'aria-describedby': 'site-help',
      tooltip: 'Wirecatch is not connected.',
      children: text('Ignore this site'),
    };
    const { rerender } = render(Button, props);
    const button = screen.getByRole('button', { name: 'Ignore this site' });
    expect(button.getAttribute('aria-describedby')).toBe('site-help');
    await rerender({ ...props, disabled: true });
    expect(button).toBe(
      screen.getByRole('button', {
        name: 'Ignore this site',
        description: 'Downloads from example.com go to Wirecatch. Wirecatch is not connected.',
      }),
    );
    expect(button.getAttribute('aria-disabled')).toBe('true');
  });

  it('does the same for an icon button', () => {
    hint();
    render(IconButton, {
      icon: Download,
      label: 'Remove',
      'aria-describedby': 'site-help',
      tooltip: 'Wirecatch is not connected.',
      disabled: true,
    });
    expect(
      screen.getByRole('button', {
        name: 'Remove',
        description: 'Downloads from example.com go to Wirecatch. Wirecatch is not connected.',
      }),
    ).toBeTruthy();
  });
});
