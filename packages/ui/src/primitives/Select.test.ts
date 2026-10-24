import { render, screen } from '@testing-library/svelte';
import userEvent from '@testing-library/user-event';
import { describe, expect, it, vi } from 'vitest';
import SelectAsideHost from '../testing/SelectAsideHost.svelte';
import Select from './Select.svelte';

const options = [
  { value: 'best', label: 'Best available' },
  { value: '1080', label: '1080p' },
  { value: '720', label: '720p', disabled: true },
  { value: '480', label: '480p' },
];

function setup(value: string | null = 'best') {
  const onchange = vi.fn();
  render(Select<string>, { value, options, label: 'Quality', onchange });
  const combobox = screen.getByRole('combobox', { name: 'Quality' });
  return { combobox, onchange };
}

describe('Select', () => {
  it('is a collapsed combobox that shows its value', () => {
    const { combobox } = setup();
    expect(combobox.getAttribute('aria-expanded')).toBe('false');
    expect(combobox.getAttribute('aria-haspopup')).toBe('listbox');
    expect(combobox.textContent).toContain('Best available');
  });

  it('looks disabled and stays closed while disabled, as a field with a disabled input does', async () => {
    render(Select<string>, { value: 'best', options, label: 'Quality', disabled: true });
    const combobox = screen.getByRole('combobox', { name: 'Quality' });
    // The trigger is the field box itself, so the box's own disabled state must dim it.
    expect(combobox.className.split(' ')).toContain('disabled:opacity-disabled');
    await userEvent.click(combobox);
    expect(combobox.getAttribute('aria-expanded')).toBe('false');
  });

  it('opens on the down arrow with the chosen option highlighted, and keeps focus on the trigger', async () => {
    const { combobox } = setup('1080');
    combobox.focus();
    await userEvent.keyboard('{ArrowDown}');
    expect(combobox.getAttribute('aria-expanded')).toBe('true');
    const listbox = screen.getByRole('listbox');
    expect(combobox.getAttribute('aria-controls')).toBe(listbox.id);
    const active = combobox.getAttribute('aria-activedescendant') ?? '';
    expect(document.getElementById(active)?.textContent).toContain('1080p');
    expect(document.activeElement).toBe(combobox);
  });

  it('moves past disabled options and chooses with Enter', async () => {
    const { combobox, onchange } = setup('1080');
    combobox.focus();
    await userEvent.keyboard('{ArrowDown}{ArrowDown}{Enter}');
    expect(onchange).toHaveBeenCalledWith('480');
    expect(combobox.getAttribute('aria-expanded')).toBe('false');
    expect(document.activeElement).toBe(combobox);
  });

  it('closes on Escape without choosing', async () => {
    const { combobox, onchange } = setup();
    combobox.focus();
    await userEvent.keyboard('{ArrowDown}{End}{Escape}');
    expect(combobox.getAttribute('aria-expanded')).toBe('false');
    expect(onchange).not.toHaveBeenCalled();
  });

  it('chooses by typing while closed, as a native select does', async () => {
    const { combobox, onchange } = setup();
    combobox.focus();
    await userEvent.keyboard('4');
    expect(onchange).toHaveBeenCalledWith('480');
  });

  it('searches by typing from the chosen option while closed, so the same letter moves on', async () => {
    const onchange = vi.fn();
    const formats = [
      { value: 'audio', label: 'Audio only' },
      { value: 'best', label: 'Best' },
      { value: 'balanced', label: 'Balanced' },
    ];
    render(Select<string>, { value: 'best', options: formats, label: 'Format', onchange });
    screen.getByRole('combobox', { name: 'Format' }).focus();
    await userEvent.keyboard('b');
    expect(onchange).toHaveBeenCalledWith('balanced');
  });

  it('marks the chosen option selected and the disabled one disabled', async () => {
    const { combobox } = setup('best');
    await userEvent.click(combobox);
    const [best, , disabled] = screen.getAllByRole('option');
    expect(best?.getAttribute('aria-selected')).toBe('true');
    expect(disabled?.getAttribute('aria-disabled')).toBe('true');
  });
});

describe('Select, searchable, with what sets each option apart', () => {
  it('ends each option, and the field, with its aside', async () => {
    render(SelectAsideHost, { onchange: vi.fn() });
    const combobox = screen.getByRole('combobox', { name: 'Quality' });
    expect(combobox.textContent).toContain('1.80 GB');
    await userEvent.click(combobox);
    expect(
      screen.getAllByRole('option').map((o) => o.textContent.replaceAll(/\s+/g, ' ').trim()),
    ).toEqual(['2160p 1.80 GB', '1080p 612 MB', '720p 298 MB']);
  });

  it('searches from a field that takes the focus, and chooses with Enter', async () => {
    const onchange = vi.fn();
    render(SelectAsideHost, { onchange });
    await userEvent.click(screen.getByRole('combobox', { name: 'Quality' }));
    const search = screen.getByRole('combobox', { name: 'Search quality' });
    expect(document.activeElement).toBe(search);
    await userEvent.keyboard('72');
    expect(screen.getAllByRole('option')).toHaveLength(1);
    await userEvent.keyboard('{Enter}');
    expect(onchange).toHaveBeenCalledWith('720');
    expect(document.activeElement).toBe(screen.getByRole('combobox', { name: 'Quality' }));
  });
});
