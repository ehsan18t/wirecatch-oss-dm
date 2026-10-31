import Download from '~icons/lucide/download';
import { fireEvent, render, screen } from '@testing-library/svelte';
import userEvent from '@testing-library/user-event';
import { describe, expect, it, vi } from 'vitest';
import DisclosureHost from '../testing/DisclosureHost.svelte';
import IconButton from './IconButton.svelte';
import Kbd from './Kbd.svelte';
import MultiSelect from './MultiSelect.svelte';
import NumberInput from './NumberInput.svelte';
import Switch from './Switch.svelte';

describe('IconButton', () => {
  it('is named by its label and hides its icon', () => {
    render(IconButton, { icon: Download, label: 'Download' });
    const button = screen.getByRole('button', { name: 'Download' });
    expect(button.querySelector('svg')?.getAttribute('aria-hidden')).toBe('true');
  });

  it('keeps focus while loading, and ignores clicks until done', async () => {
    const onclick = vi.fn();
    const { rerender } = render(IconButton, { icon: Download, label: 'Download', onclick });
    const button = screen.getByRole('button', { name: 'Download' });
    button.focus();
    await rerender({ icon: Download, label: 'Download', onclick, loading: true });
    expect(document.activeElement).toBe(button);
    expect(button.getAttribute('aria-disabled')).toBe('true');
    await userEvent.click(button);
    expect(onclick).not.toHaveBeenCalled();
  });

  it('stays focusable while disabled with a reason, and says it', () => {
    render(IconButton, {
      icon: Download,
      label: 'Retry',
      disabled: true,
      tooltip: 'Nothing failed',
    });
    const button = screen.getByRole('button', { name: 'Retry' });
    expect(button.hasAttribute('disabled')).toBe(false);
    expect(button.getAttribute('aria-disabled')).toBe('true');
    const reason = document.getElementById(button.getAttribute('aria-describedby') ?? '');
    expect(reason?.textContent).toBe('Nothing failed');
  });
});

describe('tooltip', () => {
  it('stays while the pointer moves onto it, and goes once the pointer leaves both (WCAG 1.4.13)', async () => {
    vi.useFakeTimers();
    render(IconButton, { icon: Download, label: 'Download' });
    const button = screen.getByRole('button', { name: 'Download' });
    await fireEvent.mouseEnter(button);
    vi.runAllTimers();
    const tip = screen.getByRole('tooltip');
    await fireEvent.mouseLeave(button);
    await fireEvent.pointerEnter(tip);
    vi.runAllTimers();
    expect(screen.getByRole('tooltip')).toBe(tip);
    await fireEvent.pointerLeave(tip);
    vi.runAllTimers();
    expect(screen.queryByRole('tooltip')).toBeNull();
  });

  it('works outside a secure context, where crypto.randomUUID is missing, with ids of its own', () => {
    vi.stubGlobal('crypto', {});
    render(IconButton, { icon: Download, label: 'Download', tooltip: 'Save to disk' });
    render(IconButton, { icon: Download, label: 'Pause', tooltip: 'Hold the transfer' });
    vi.useFakeTimers();
    for (const name of ['Download', 'Pause']) {
      void fireEvent.mouseEnter(screen.getByRole('button', { name }));
      vi.runAllTimers();
    }
    const ids = screen.getAllByRole('tooltip').map((tip) => tip.id);
    expect(new Set(ids).size).toBe(ids.length);
  });
});

describe('Switch', () => {
  it('is a switch named by its label that toggles on Enter and Space', async () => {
    const onchange = vi.fn();
    render(Switch, { checked: false, label: 'Start with Windows', onchange });
    const toggle = screen.getByRole('switch', { name: 'Start with Windows' });
    expect(toggle.getAttribute('aria-checked')).toBe('false');
    toggle.focus();
    await userEvent.keyboard('{Enter}');
    await userEvent.keyboard(' ');
    expect(onchange).toHaveBeenCalledTimes(2);
    expect(onchange).toHaveBeenCalledWith(true);
  });

  it('ignores input while loading', async () => {
    const onchange = vi.fn();
    render(Switch, { checked: true, label: 'Sync', loading: true, onchange });
    const toggle = screen.getByRole('switch', { name: 'Sync' });
    expect(toggle.getAttribute('aria-busy')).toBe('true');
    await userEvent.click(toggle);
    expect(onchange).not.toHaveBeenCalled();
  });
});

describe('NumberInput', () => {
  it('stays usable at its minimum: only the step down is disabled, not the whole field', () => {
    render(NumberInput, { label: 'Connections', value: 1, min: 1, max: 8 });
    const input = screen.getByRole('spinbutton', { name: 'Connections' });
    const box = input.parentElement;
    expect(input.hasAttribute('disabled')).toBe(false);
    expect(box?.matches(':has(:disabled)')).toBe(true);
    expect(box?.className).not.toMatch(/(^|\s)has-disabled:/);
  });

  it('is a spin button that steps with the arrows and commits typed text', async () => {
    const onchange = vi.fn();
    render(NumberInput, {
      value: 4,
      min: 1,
      max: 16,
      unit: 'connections',
      label: 'Connections',
      onchange,
    });
    const input = screen.getByRole('spinbutton', { name: 'Connections' });
    expect(input.getAttribute('aria-valuenow')).toBe('4');
    expect(input.getAttribute('aria-valuetext')).toBe('4 connections');
    input.focus();
    await userEvent.keyboard('{ArrowUp}');
    expect(onchange).toHaveBeenLastCalledWith(5);
    await userEvent.clear(input);
    await userEvent.type(input, '40{Enter}');
    expect(onchange).toHaveBeenLastCalledWith(16);
  });

  it('keeps the last value when the text is not a number', async () => {
    const onchange = vi.fn();
    render(NumberInput, { value: 4, label: 'Connections', onchange });
    const input = screen.getByRole<HTMLInputElement>('spinbutton');
    await userEvent.clear(input);
    await userEvent.type(input, 'abc');
    await fireEvent.blur(input);
    expect(onchange).not.toHaveBeenCalled();
    expect(input.value).toBe('4');
  });

  it('steps from the last value when the typed text is not a number, never reporting NaN', async () => {
    const onchange = vi.fn();
    render(NumberInput, { value: 12, label: 'Speed limit', onchange });
    const input = screen.getByRole<HTMLInputElement>('spinbutton');
    await userEvent.clear(input);
    await userEvent.type(input, '12a{ArrowUp}');
    expect(onchange).toHaveBeenCalledWith(13);
    expect(onchange.mock.calls.flat().some((value) => Number.isNaN(value))).toBe(false);
  });
});

describe('Kbd', () => {
  it('draws caps side by side, reads them joined by plus, and draws arrows as arrows', () => {
    const { container } = render(Kbd, { keys: ['Alt', 'Up'] });
    const caps = [...container.querySelectorAll('kbd kbd')].map((k) => k.textContent);
    expect(caps).toEqual(['Alt', '↑']);
    expect(container.textContent.replaceAll(/\s+/g, '')).toBe('Alt+↑');
    // The plus is for screen readers and copies, not drawn.
    expect(container.querySelector('.sr-only')?.textContent).toBe('+');
  });
});

describe('Disclosure', () => {
  /** Opens the host's disclosure with a click, after checking it starts folded; answers its toggle. */
  async function unfold(variant: 'inline' | 'card'): Promise<HTMLElement> {
    render(DisclosureHost, { variant });
    const toggle = screen.getByRole('button', { name: /^Subtitles/ });
    expect(toggle.getAttribute('aria-expanded')).toBe('false');
    expect(toggle.textContent).toContain('English · chapters');
    const region = document.getElementById(toggle.getAttribute('aria-controls') ?? '');
    expect(region?.hidden).toBe(true);
    await userEvent.click(toggle);
    expect(toggle.getAttribute('aria-expanded')).toBe('true');
    expect(region?.hidden).toBe(false);
    return toggle;
  }

  it('folds its choices into a line that says what they are set to, and opens on a click', async () => {
    const toggle = await unfold('inline');
    // Open, the summary gives way to the choices themselves.
    expect(toggle.textContent).not.toContain('English · chapters');
  });

  it('as a card, keeps its summary while open, in a heading', async () => {
    const toggle = await unfold('card');
    // The header row keeps its height, so it does not jump under the pointer.
    expect(toggle.closest('h2')?.textContent).toContain('English · chapters');
  });
});

describe('MultiSelect', () => {
  const languages = [
    { value: 'ar', label: 'Arabic' },
    { value: 'bn', label: 'Bangla', keywords: 'Bengali' },
    { value: 'en', label: 'English' },
    { value: 'pt', label: 'Portuguese (Brazil)' },
    { value: 'es', label: 'Spanish' },
  ];

  it('searches a long list and ticks with Enter, the search keeping the focus', async () => {
    const onchange = vi.fn();
    render(MultiSelect, { values: [], options: languages, label: 'Subtitles', onchange });
    const field = screen.getByRole('button', { name: 'Subtitles' });
    expect(field.textContent).toContain('None');
    await userEvent.click(field);
    const search = screen.getByRole('combobox', { name: 'Search subtitles' });
    expect(document.activeElement).toBe(search);
    await userEvent.keyboard('port');
    expect(screen.getAllByRole('option').map((o) => o.textContent.trim())).toEqual([
      'Portuguese (Brazil)',
    ]);
    await userEvent.keyboard('{Enter}');
    expect(onchange).toHaveBeenLastCalledWith(['pt']);
    // An option is found by words it does not show, too.
    await userEvent.clear(search);
    await userEvent.keyboard('bengali');
    expect(screen.getAllByRole('option').map((o) => o.textContent.trim())).toEqual(['Bangla']);
  });

  it('lists what is picked first, says it on the field, and closes on Escape back to it', async () => {
    const onchange = vi.fn();
    render(MultiSelect, { values: ['es', 'en'], options: languages, label: 'Subtitles', onchange });
    const field = screen.getByRole('button', { name: 'Subtitles' });
    expect(field.textContent).toContain('English, Spanish');
    await userEvent.click(field);
    const options = screen.getAllByRole('option');
    expect(options.slice(0, 2).map((o) => o.getAttribute('aria-selected'))).toEqual([
      'true',
      'true',
    ]);
    await userEvent.click(screen.getByRole('button', { name: 'Clear' }));
    expect(onchange).toHaveBeenLastCalledWith([]);
    await userEvent.keyboard('{Escape}');
    expect(screen.queryByRole('listbox')).toBeNull();
    expect(document.activeElement).toBe(field);
  });
});
