import { describe, expect, it } from 'vitest';
import { BackendError } from './backend';
import { mediaAddon } from './backend/mock';
import {
  addonAction,
  addonAnnouncement,
  addonStatus,
  installFraction,
  isAddonRequired,
  isInstalled,
} from './addons';

describe('a missing add-on', () => {
  it('is told by its code, whatever the wording', () => {
    expect(isAddonRequired(new BackendError('Reworded later.', 'addon_required'))).toBe(true);
    expect(
      isAddonRequired(new BackendError('This needs the media add-on.', 'invalid_request')),
    ).toBe(false);
    expect(isAddonRequired(new BackendError('This needs the media add-on.'))).toBe(false);
    expect(isAddonRequired('This needs the media add-on.')).toBe(false);
  });
});

describe('add-on wording', () => {
  it('says where the add-on stands in plain words', () => {
    const a = mediaAddon();
    expect(addonStatus(a)).toBe('Not installed.');
    expect(addonAction(a)).toBe('install');

    a.state = { type: 'installing', data: { done: 48 * 1024 * 1024, total: 96 * 1024 * 1024 } };
    expect(addonStatus(a)).toBe('Installing, 50%.');
    expect(installFraction(a.state)).toBe(0.5);
    expect(addonAction(a)).toBeNull();
    // All downloaded: the core checks and unpacks before it says installed.
    a.state = { type: 'installing', data: { done: 96, total: 96 } };
    expect(addonStatus(a)).toBe('Installing: verifying files.');

    a.state = { type: 'installed' };
    a.installed_version = '2026.10.02';
    expect(addonStatus(a)).toBe('Installed: 2026.10.02.');
    expect(isInstalled(a)).toBe(true);

    a.state = { type: 'update_available' };
    expect(addonAction(a)).toBe('update');
    expect(isInstalled(a)).toBe(true);

    a.state = {
      type: 'failed',
      data: { message: 'Installing the media add-on failed. The download was interrupted.' },
    };
    expect(addonStatus(a)).toBe(
      'Installing the media add-on failed. The download was interrupted.',
    );
    expect(addonAction(a)).toBe('retry');
  });

  it('announces the start, every quarter and the end, not every percent', () => {
    const a = mediaAddon();
    const at = (done: number) => {
      a.state = { type: 'installing', data: { done, total: 100 } };
      return addonAnnouncement(a);
    };
    const heard = new Set([0, 1, 10, 24, 25, 26, 49, 50, 74, 75, 99].map(at));
    expect([...heard]).toEqual([
      'Installing the media add-on.',
      'Media add-on: 25% installed.',
      'Media add-on: 50% installed.',
      'Media add-on: 75% installed.',
    ]);
    a.state = { type: 'installed' };
    expect(addonAnnouncement(a)).toBe('Media add-on installed.');
  });

  it('does not invent a percentage when the size is unknown', () => {
    expect(installFraction({ type: 'installing', data: { done: 5, total: null } })).toBeNull();
    const a = mediaAddon();
    a.state = { type: 'installing', data: { done: 5, total: null } };
    expect(addonStatus(a)).toBe('Installing.');
  });
});
