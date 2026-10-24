import Download from '~icons/lucide/download';
import { fireEvent, render, screen } from '@testing-library/svelte';
import userEvent from '@testing-library/user-event';
import { createRawSnippet } from 'svelte';
import { describe, expect, it, vi } from 'vitest';
import DisclosureHost from '../testing/DisclosureHost.svelte';
import FieldHost from '../testing/FieldHost.svelte';
import RemovedFieldHost from '../testing/RemovedFieldHost.svelte';
import StrippingInputHost from '../testing/StrippingInputHost.svelte';
import Checkbox from './Checkbox.svelte';
import IconButton from './IconButton.svelte';
import Kbd from './Kbd.svelte';
import MultiSelect from './MultiSelect.svelte';
import Input from './Input.svelte';
import NumberInput from './NumberInput.svelte';
import RadioGroup from './RadioGroup.svelte';
import ProgressBar from './ProgressBar.svelte';
import SegmentedControl from './SegmentedControl.svelte';
import Slider from './Slider.svelte';
import Switch from './Switch.svelte';
import TagInput from './TagInput.svelte';
import Tabs from './Tabs.svelte';
import TextArea from './TextArea.svelte';

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

describe('text controls', () => {
  it('show what the owner keeps when it rewrites the text', async () => {
    render(StrippingInputHost);
    const input = screen.getByRole<HTMLInputElement>('textbox', { name: 'File name' });
    await userEvent.type(input, 'a b');
    expect(input.value).toBe('ab');
    // A trailing space changes nothing the owner keeps; it goes once the field is left.
    await userEvent.type(input, ' ');
    await userEvent.tab();
    expect(input.value).toBe('ab');
  });

  it('go back to their value when the owner refuses the change, once the field is left', async () => {
    const onchange = vi.fn();
    render(Input, { value: 'fixed', label: 'Address', onchange });
    const input = screen.getByRole<HTMLInputElement>('textbox', { name: 'Address' });
    await userEvent.type(input, 'x');
    expect(onchange).toHaveBeenCalledWith('fixedx');
    await userEvent.tab();
    expect(input.value).toBe('fixed');
    render(TextArea, { value: 'one', label: 'Links', onchange });
    const area = screen.getByRole<HTMLTextAreaElement>('textbox', { name: 'Links' });
    await userEvent.type(area, 'x');
    await userEvent.tab();
    expect(area.value).toBe('one');
  });

  it('keep every keystroke while an owner that answers later catches up', async () => {
    const onchange = vi.fn();
    const props = { label: 'Folder', onchange };
    const { rerender } = render(Input, { ...props, value: '' });
    const input = screen.getByRole<HTMLInputElement>('textbox', { name: 'Folder' });
    // A settings field: the value comes back from the store only after the round trip.
    await userEvent.type(input, 'ab');
    expect(onchange.mock.calls).toEqual([['a'], ['ab']]);
    expect(input.value).toBe('ab');
    await rerender({ ...props, value: 'a' });
    expect(input.value).toBe('ab');
    await rerender({ ...props, value: 'ab' });
    expect(input.value).toBe('ab');
    expect(input.selectionStart).toBe(2);
  });

  it('answer the blur a removed field gets without reading their torn-down owner', async () => {
    const warn = vi.spyOn(console, 'warn');
    render(RemovedFieldHost);
    const fields = screen.getAllByRole('textbox');
    await userEvent.click(screen.getByRole('button', { name: 'Done' }));
    expect(screen.queryByRole('textbox')).toBeNull();
    // What Chromium does as a focused field leaves the page; jsdom sends nothing.
    for (const field of fields) field.dispatchEvent(new FocusEvent('blur'));
    expect(warn.mock.calls.flat().join(' ')).not.toMatch(/derived_inert/);
  });

  it('show a value set from outside while they are not being edited', async () => {
    const props = { label: 'Folder', onchange: vi.fn() };
    const { rerender } = render(TextArea, { ...props, value: 'one' });
    await rerender({ ...props, value: 'two' });
    expect(screen.getByRole<HTMLTextAreaElement>('textbox', { name: 'Folder' }).value).toBe('two');
  });
});

describe('Field', () => {
  it('hands its control one spreadable set: the label, the hint and the error point at it', async () => {
    const { rerender } = render(FieldHost);
    const input = screen.getByRole('textbox', { name: 'Address' });
    const described = () =>
      document.getElementById(input.getAttribute('aria-describedby') ?? '')?.textContent;
    expect(described()).toBe('A link to a file.');
    expect(input.hasAttribute('aria-invalid')).toBe(false);
    await userEvent.type(input, 'x');
    expect(screen.getByTestId('value').textContent).toBe('https://x');
    await rerender({ error: 'Not a link.' });
    expect(input.getAttribute('aria-invalid')).toBe('true');
    expect(described()).toBe('Not a link.');
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

describe('Checkbox', () => {
  it('is a native checkbox named by its label, described by its hint', () => {
    render(Checkbox, { label: 'Delete the files too', description: 'They go to the recycle bin.' });
    const box = screen.getByRole('checkbox', { name: 'Delete the files too' });
    expect(box.getAttribute('aria-describedby')).toBeTruthy();
  });

  it('is controlled, like every other control: it asks, and shows what it is given', async () => {
    const onchange = vi.fn();
    const { rerender } = render(Checkbox, { checked: false, label: 'Delete', onchange });
    const box = screen.getByRole<HTMLInputElement>('checkbox', { name: 'Delete' });
    await userEvent.click(box);
    expect(onchange).toHaveBeenCalledWith(true);
    expect(box.checked).toBe(false);
    await rerender({ checked: true, label: 'Delete', onchange });
    expect(box.checked).toBe(true);
  });

  it('can show its label to screen readers only', () => {
    render(Checkbox, { label: 'Select ubuntu.iso', hideLabel: true });
    const box = screen.getByRole('checkbox', { name: 'Select ubuntu.iso' });
    const label = document.getElementById(box.getAttribute('aria-labelledby') ?? '');
    expect(label?.className).toContain('sr-only');
  });
});

describe('RadioGroup', () => {
  const options = [
    { value: 'uhd', label: '2160p' },
    { value: 'hd', label: '1080p' },
  ];
  const aside = createRawSnippet((option: () => { value: string }) => ({
    render: () =>
      `<span>${option().value === 'uhd' ? 'Best, about 1.2 GB' : 'About 600 MB'}</span>`,
  }));

  it('as cards, marks the chosen row and reads what each row adds as its description', async () => {
    const onchange = vi.fn();
    const { rerender } = render(RadioGroup<string>, {
      value: 'uhd',
      options,
      label: 'Quality',
      cards: true,
      aside,
      onchange,
    });
    const best = screen.getByRole<HTMLInputElement>('radio', { name: '2160p' });
    expect(best.checked).toBe(true);
    const described = document.getElementById(best.getAttribute('aria-describedby') ?? '');
    expect(described?.textContent).toBe('Best, about 1.2 GB');
    expect(best.closest('label')?.dataset.checked).toBe('');
    const hd = screen.getByRole('radio', { name: '1080p' });
    expect(hd.closest('label')?.dataset.checked).toBeUndefined();
    await userEvent.click(screen.getByText('About 600 MB'));
    expect(onchange).toHaveBeenCalledWith('hd');
    await rerender({ value: 'hd' });
    expect(hd.closest('label')?.dataset.checked).toBe('');
  });
});

describe('SegmentedControl', () => {
  const options = [
    { value: 'list', label: 'List' },
    { value: 'grid', label: 'Grid', disabled: true },
    { value: 'compact', label: 'Compact' },
  ];

  it('is a radio group with one tab stop, and arrows choose, skipping disabled options', async () => {
    const onchange = vi.fn();
    render(SegmentedControl, { value: 'list', options, label: 'Layout', onchange });
    const radios = screen.getAllByRole('radio');
    expect(screen.getByRole('radiogroup', { name: 'Layout' })).toBeTruthy();
    expect(radios.map((radio) => radio.tabIndex)).toEqual([0, -1, -1]);
    expect(radios[0]?.getAttribute('aria-checked')).toBe('true');
    radios[0]?.focus();
    await userEvent.keyboard('{ArrowRight}');
    expect(onchange).toHaveBeenCalledWith('compact');
    expect(document.activeElement).toBe(radios[2]);
  });
});

describe('Tabs', () => {
  it('follows the tabs pattern: selection, arrows, and a labelled panel', async () => {
    const onchange = vi.fn();
    render(Tabs, {
      value: 'general',
      tabs: [
        { value: 'general', label: 'General' },
        { value: 'network', label: 'Network', count: 3 },
      ],
      label: 'Settings',
      onchange,
    });
    const [general, network] = screen.getAllByRole('tab');
    expect(general?.getAttribute('aria-selected')).toBe('true');
    expect(network?.tabIndex).toBe(-1);
    general?.focus();
    await userEvent.keyboard('{End}');
    expect(onchange).toHaveBeenCalledWith('network');
  });

  it('keeps a tab stop when no tab is chosen, and a disabled tab focusable but never shown', async () => {
    const onchange = vi.fn();
    render(Tabs, {
      value: 'gone',
      tabs: [
        { value: 'general', label: 'General' },
        { value: 'proxy', label: 'Proxy', disabled: true },
      ],
      label: 'Settings',
      onchange,
    });
    const [general, proxy] = screen.getAllByRole('tab');
    expect(general?.tabIndex).toBe(0);
    expect(proxy?.getAttribute('aria-disabled')).toBe('true');
    general?.focus();
    await userEvent.keyboard('{ArrowRight}');
    expect(document.activeElement).toBe(proxy);
    expect(onchange).not.toHaveBeenCalledWith('proxy');
  });
});

describe('NumberInput', () => {
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

describe('Slider', () => {
  it('is a native slider that reports what its value means', async () => {
    const onchange = vi.fn();
    render(Slider, {
      value: 3,
      min: 1,
      max: 8,
      label: 'Downloads at once',
      valueText: (value: number) => `${String(value)} at once`,
      onchange,
    });
    const slider = screen.getByRole('slider', { name: 'Downloads at once' });
    expect(slider.getAttribute('aria-valuetext')).toBe('3 at once');
    await fireEvent.change(slider, { target: { value: '5' } });
    expect(onchange).toHaveBeenCalledWith(5);
  });
});

describe('ProgressBar', () => {
  it('reports a whole, floored percentage and its state', () => {
    render(ProgressBar, { value: 0.996, label: 'ubuntu.iso', valueText: '99%, 4.2 MB/s' });
    const bar = screen.getByRole('progressbar', { name: 'ubuntu.iso' });
    expect(bar.getAttribute('aria-valuenow')).toBe('99');
    expect(bar.getAttribute('aria-valuetext')).toBe('99%, 4.2 MB/s');
    expect(bar.hasAttribute('aria-live')).toBe(false);
  });

  it('runs indeterminate without a value', () => {
    render(ProgressBar, { value: null, label: 'Reading the video' });
    const bar = screen.getByRole('progressbar', { name: 'Reading the video' });
    expect(bar.hasAttribute('aria-valuenow')).toBe(false);
    expect(bar.getAttribute('aria-busy')).toBe('true');
  });

  it('marks a paused or failed bar for its colour', () => {
    const { container } = render(ProgressBar, { value: 0.5, state: 'paused', label: 'Paused' });
    expect(container.querySelector('[data-state="paused"]')).toBeTruthy();
  });
});

describe('TagInput', () => {
  const site = (typed: string) => (/^[a-z0-9.-]+\.[a-z]+$/.test(typed) ? typed : null);

  it('adds what is typed on Enter, a comma or a paste of several, once each', async () => {
    const onchange = vi.fn();
    render(TagInput, { values: ['a.com'], label: 'Sites', normalize: site, onchange });
    const box = screen.getByRole('textbox', { name: 'Sites' });
    await userEvent.type(box, 'b.com{Enter}');
    expect(onchange).toHaveBeenLastCalledWith(['a.com', 'b.com']);
    await userEvent.type(box, 'a.com,');
    // Already in the list: nothing to add.
    expect(onchange).toHaveBeenCalledTimes(1);
    await userEvent.click(box);
    await userEvent.paste('c.com d.com');
    expect(onchange).toHaveBeenLastCalledWith(['a.com', 'c.com', 'd.com']);
  });

  it('keeps a word it cannot take in the box, marked, and takes the last chip back on Backspace', async () => {
    const onchange = vi.fn();
    render(TagInput, { values: ['a.com'], label: 'Sites', normalize: site, onchange });
    const box = screen.getByRole('textbox', { name: 'Sites' });
    await userEvent.type(box, 'nonsense{Enter}');
    expect(onchange).not.toHaveBeenCalled();
    expect((box as HTMLInputElement).value).toBe('nonsense');
    expect(box.getAttribute('aria-invalid')).toBe('true');
    // It says why, not only that something is wrong.
    expect(box.getAttribute('aria-describedby')).toBeTruthy();
    await userEvent.clear(box);
    await userEvent.type(box, '{Backspace}');
    expect(onchange).toHaveBeenLastCalledWith([]);
    expect((box as HTMLInputElement).value).toBe('a.com');
  });

  it('removes a chip with its own button', async () => {
    const onchange = vi.fn();
    render(TagInput, { values: ['zip', 'iso'], label: 'File types', onchange });
    // The chips are a list a screen reader can count, and a removal is said.
    const list = screen.getByRole('list', { name: 'File types' });
    expect(list.querySelectorAll('[role="listitem"]')).toHaveLength(2);
    await userEvent.click(screen.getByRole('button', { name: 'Remove zip from file types' }));
    expect(onchange).toHaveBeenCalledWith(['iso']);
    expect(document.querySelector('[aria-live="polite"]')?.textContent).toBe('Removed zip.');
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
