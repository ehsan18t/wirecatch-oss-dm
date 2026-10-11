import type { AppSettings, SettingsChange } from '@wirecatch/protocol';
import { describe, expect, it } from 'vitest';
import { MockBackend } from '../backend/mock';
import {
  applyChanges,
  changeSettings,
  type SettingsHolder,
  SettingsWriter,
  settingsChanges,
  withoutChanges,
} from './settings-changes.svelte';

async function settings(): Promise<AppSettings> {
  return new MockBackend({ empty: true }).getSettings();
}

describe('settingsChanges', () => {
  it('names only the settings that changed, down to the setting in a group', async () => {
    const before = await settings();
    const after = structuredClone(before);
    after.theme = 'dark';
    after.extension.ask_before_download = !before.extension.ask_before_download;
    expect(settingsChanges(before, after)).toEqual(
      [
        { path: ['extension', 'ask_before_download'], value: after.extension.ask_before_download },
        { path: ['theme'], value: 'dark' },
      ].sort((a, b) => a.path.join('.').localeCompare(b.path.join('.'))),
    );
  });

  it('replaces a list whole', async () => {
    const before = await settings();
    const after = structuredClone(before);
    after.extension.ignored_sites = [...before.extension.ignored_sites, 'example.com'];
    expect(settingsChanges(before, after)).toEqual([
      { path: ['extension', 'ignored_sites'], value: after.extension.ignored_sites },
    ]);
  });

  it('has nothing to send when nothing changed', async () => {
    const before = await settings();
    expect(settingsChanges(before, structuredClone(before))).toEqual([]);
  });
});

describe('withoutChanges', () => {
  it('takes back a failed change unless something newer replaced it', async () => {
    const before = await settings();
    const failed: SettingsChange[] = [
      { path: ['theme'], value: 'dark' },
      { path: ['notify_on_complete'], value: !before.notify_on_complete },
    ];
    const current = applyChanges(before, failed);
    current.theme = 'light'; // a newer change, made while the failed one was on its way
    const restored = withoutChanges(current, failed, before);
    expect(restored.theme).toBe('light');
    expect(restored.notify_on_complete).toBe(before.notify_on_complete);
  });
});

describe('SettingsWriter', () => {
  it('sends batches in the order they were made, even when an earlier one is slower', async () => {
    const sent: string[] = [];
    let releaseFirst: () => void = () => undefined;
    const firstDone = new Promise<void>((resolve) => (releaseFirst = resolve));
    const writer = new SettingsWriter(async (changes) => {
      const name = changes[0]?.path.join('.') ?? '';
      if (name === 'first') await firstDone;
      sent.push(name);
      return true;
    });
    const first = writer.write([{ path: ['first'], value: 1 }]);
    const second = writer.write([{ path: ['second'], value: 2 }]);
    releaseFirst();
    await Promise.all([first, second]);
    expect(sent).toEqual(['first', 'second']);
  });

  it('keeps going after a batch fails', async () => {
    const writer = new SettingsWriter((changes) =>
      changes[0]?.path[0] === 'bad' ? Promise.reject(new Error('refused')) : Promise.resolve(true),
    );
    await expect(writer.write([{ path: ['bad'], value: 0 }])).rejects.toThrow('refused');
    await expect(writer.write([{ path: ['good'], value: 0 }])).resolves.toBe(true);
  });
});

// Each window sent its whole copy of the settings, so a change made in another window that had not
// reached it yet was quietly undone.
describe('two windows changing settings at once', () => {
  it('keeps both changes', async () => {
    const backend = new MockBackend({ empty: true });
    const start = await backend.getSettings();
    const send = (changes: SettingsChange[]) => backend.changeSettings(changes).then(() => true);
    const main: SettingsHolder = { settings: structuredClone(start) };
    const download: SettingsHolder = { settings: structuredClone(start) };

    await changeSettings(main, (d) => void (d.theme = 'dark'), new SettingsWriter(send));
    await changeSettings(
      download,
      (d) => void (d.show_progress_windows = !d.show_progress_windows),
      new SettingsWriter(send),
    );

    const saved = await backend.getSettings();
    expect(saved.theme).toBe('dark');
    expect(saved.show_progress_windows).toBe(!start.show_progress_windows);
  });

  it('takes back only the refused change in the window that made it', async () => {
    const start = await settings();
    const holder: SettingsHolder = { settings: structuredClone(start) };
    const writer = new SettingsWriter(() => Promise.resolve(false));

    await changeSettings(holder, (d) => void (d.theme = 'dark'), writer);

    expect(holder.settings).toEqual(start);
  });
});
