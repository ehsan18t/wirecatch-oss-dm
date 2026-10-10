import { describe, expect, it } from 'vitest';
import { mediaAddon } from './backend/mock';
import { addonAction, addonStatus, installFraction, isInstalled } from './addons';

describe('add-on wording', () => {
  it('says where the add-on stands in plain words', () => {
    const a = mediaAddon();
    expect(addonStatus(a)).toBe('Not installed. Download size 96.0 MB.');
    expect(addonAction(a)).toBe('install');

    a.state = { type: 'installing', data: { done: 48 * 1024 * 1024, total: 96 * 1024 * 1024 } };
    expect(addonStatus(a)).toBe('Installing, 50%.');
    expect(installFraction(a.state)).toBe(0.5);
    expect(addonAction(a)).toBeNull();

    a.state = { type: 'installed' };
    a.installed_version = '2026.10.02';
    expect(addonStatus(a)).toBe('Installed, version 2026.10.02.');
    expect(isInstalled(a)).toBe(true);

    a.state = { type: 'update_available' };
    expect(addonAction(a)).toBe('update');
    expect(isInstalled(a)).toBe(true);

    a.state = { type: 'failed', data: { message: 'The download was interrupted.' } };
    expect(addonStatus(a)).toBe('Could not install: The download was interrupted.');
    expect(addonAction(a)).toBe('retry');
  });

  it('does not invent a percentage when the size is unknown', () => {
    expect(installFraction({ type: 'installing', data: { done: 5, total: null } })).toBeNull();
    const a = mediaAddon();
    a.state = { type: 'installing', data: { done: 5, total: null } };
    expect(addonStatus(a)).toBe('Installing.');
  });
});
