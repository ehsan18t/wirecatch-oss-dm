import { render, screen } from '@testing-library/svelte';
import userEvent from '@testing-library/user-event';
import { describe, expect, it } from 'vitest';
import SettingsHost from '../testing/SettingsHost.svelte';
import AppMark from '../shell/AppMark.svelte';
import { matchesSearch, searchWords } from './search';

describe('settings rows', () => {
  it('groups rows in a region named by the section title', () => {
    render(SettingsHost);
    const section = screen.getByRole('region', { name: 'Saving' });
    expect(section.textContent).toContain('Where finished files go.');
    expect(section.querySelector('h2')?.textContent.trim()).toBe('Saving');
  });

  it('labels and describes a row control by the row', () => {
    render(SettingsHost);
    const input = screen.getByRole('textbox', { name: 'Folder' });
    expect(input).toHaveProperty('value', 'Downloads');
    expect(input.getAttribute('aria-describedby')).toBe(
      screen.getByText('New downloads go here.').id,
    );
    expect(input.hasAttribute('aria-invalid')).toBe(false);
  });

  it('announces an error under the control, marks it invalid and keeps the help', () => {
    render(SettingsHost, { error: 'That folder cannot be written to.' });
    const input = screen.getByRole('textbox', { name: 'Folder' });
    const alert = screen.getByRole('alert');
    expect(alert.textContent).toBe('That folder cannot be written to.');
    expect(input.getAttribute('aria-invalid')).toBe('true');
    const described = input.getAttribute('aria-describedby')?.split(' ');
    expect(described).toEqual([screen.getByText('New downloads go here.').id, alert.id]);
  });

  it('leaves a control with a name of its own alone when the row is not a label', () => {
    render(SettingsHost);
    const button = screen.getByRole('button', {
      name: 'Check for updates',
      description: 'Whether a newer version is out.',
    });
    expect(button).toBeTruthy();
    expect(screen.getByText('Version').tagName).toBe('P');
  });

  it('names a switch row by its label, describes it, and toggles it from the keyboard', async () => {
    render(SettingsHost);
    const toggle = screen.getByRole('switch', {
      name: 'Ask where to save',
      description: 'Choose a folder for each file.',
    });
    expect(toggle.getAttribute('aria-checked')).toBe('false');
    toggle.focus();
    await userEvent.keyboard(' ');
    expect(toggle.getAttribute('aria-checked')).toBe('true');
  });
});

describe('searching settings', () => {
  const capture = {
    label: 'Capture downloads',
    description: 'Send downloads from this browser to Wirecatch.',
    keywords: ['intercept'],
  };

  it('splits a search into lower-case words', () => {
    expect(searchWords('  Capture   DOWNLOADS ')).toEqual(['capture', 'downloads']);
    expect(searchWords('   ')).toEqual([]);
  });

  it('matches every word, in any order, in the label, the description or the keywords', () => {
    expect(matchesSearch(capture, '')).toBe(true);
    expect(matchesSearch(capture, 'downloads capture')).toBe(true);
    expect(matchesSearch(capture, 'browser')).toBe(true);
    expect(matchesSearch(capture, 'INTERCEPT')).toBe(true);
    expect(matchesSearch(capture, 'capture queue')).toBe(false);
    expect(matchesSearch({ label: 'Default queue' }, 'queue')).toBe(true);
  });
});

describe('AppMark', () => {
  it('is decoration sized from the tokens', () => {
    const { container } = render(AppMark, { size: 'lg' });
    const svg = container.querySelector('svg');
    expect(svg?.getAttribute('aria-hidden')).toBe('true');
    expect(svg?.getAttribute('class')).toContain('size-icon-lg');
  });
});
