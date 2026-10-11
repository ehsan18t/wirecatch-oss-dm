import type { AppSettings, AppSettingsPatch } from '@wirecatch/protocol';
import { describe, expect, it } from 'vitest';
import { MockBackend } from '../backend/mock';
import {
  applyPatch,
  changeSettings,
  type SettingsHolder,
  SettingsWriter,
  settingsPatch,
  withoutPatch,
} from './settings-changes.svelte';

async function settings(): Promise<AppSettings> {
  return new MockBackend({ empty: true }).getSettings();
}

describe('settingsPatch', () => {
  it('holds only the settings that changed, down to the setting in a group', async () => {
    const before = await settings();
    const after = structuredClone(before);
    after.theme = 'dark';
    after.extension.ask_before_download = !before.extension.ask_before_download;
    after.media.audio.chapters = !before.media.audio.chapters;
    expect(settingsPatch(before, after)).toEqual({
      theme: 'dark',
      extension: { ask_before_download: after.extension.ask_before_download },
      media: { audio: { chapters: after.media.audio.chapters } },
    });
  });

  it('replaces a list whole', async () => {
    const before = await settings();
    const after = structuredClone(before);
    after.extension.ignored_sites = [...before.extension.ignored_sites, 'example.com'];
    expect(settingsPatch(before, after)).toEqual({
      extension: { ignored_sites: after.extension.ignored_sites },
    });
  });

  // A setting that can be empty must reach the app as null, which clears it, and back.
  it('sends a setting that became empty as null', async () => {
    const before = await settings();
    const limited = structuredClone(before);
    limited.global_speed_limit = 500_000;
    limited.media.audio.audio_kbps = 192;
    expect(settingsPatch(before, limited)).toEqual({
      global_speed_limit: 500_000,
      media: { audio: { audio_kbps: 192 } },
    });
    const cleared = structuredClone(limited);
    cleared.global_speed_limit = null;
    cleared.media.audio.audio_kbps = null;
    const patch = settingsPatch(limited, cleared);
    expect(patch).toEqual({ global_speed_limit: null, media: { audio: { audio_kbps: null } } });
    expect(applyPatch(limited, patch ?? {})).toEqual(cleared);
  });

  it('has nothing to send when nothing changed', async () => {
    const before = await settings();
    expect(settingsPatch(before, structuredClone(before))).toBeNull();
  });
});

describe('withoutPatch', () => {
  it('takes back a failed change unless something newer replaced it', async () => {
    const before = await settings();
    const failed: AppSettingsPatch = {
      theme: 'dark',
      notify_on_complete: !before.notify_on_complete,
      extension: { capture_enabled: !before.extension.capture_enabled },
    };
    const current = applyPatch(before, failed);
    current.theme = 'light'; // a newer change, made while the failed one was on its way
    const restored = withoutPatch(current, failed, before);
    expect(restored.theme).toBe('light');
    expect(restored.notify_on_complete).toBe(before.notify_on_complete);
    expect(restored.extension.capture_enabled).toBe(before.extension.capture_enabled);
  });
});

describe('SettingsWriter', () => {
  it('sends patches in the order they were made, even when an earlier one is slower', async () => {
    const sent: number[] = [];
    let releaseFirst: () => void = () => undefined;
    const firstDone = new Promise<void>((resolve) => (releaseFirst = resolve));
    const writer = new SettingsWriter(async (patch) => {
      const order = patch.max_concurrent_downloads ?? 0;
      if (order === 1) await firstDone;
      sent.push(order);
      return true;
    });
    const first = writer.write({ max_concurrent_downloads: 1 });
    const second = writer.write({ max_concurrent_downloads: 2 });
    releaseFirst();
    await Promise.all([first, second]);
    expect(sent).toEqual([1, 2]);
  });

  it('keeps going after a patch fails', async () => {
    const writer = new SettingsWriter((patch) =>
      patch.theme === 'dark' ? Promise.reject(new Error('refused')) : Promise.resolve(true),
    );
    await expect(writer.write({ theme: 'dark' })).rejects.toThrow('refused');
    await expect(writer.write({ theme: 'light' })).resolves.toBe(true);
  });
});

// Each window sent its whole copy of the settings, so a change made in another window that had not
// reached it yet was quietly undone.
describe('two windows changing settings at once', () => {
  it('keeps both changes', async () => {
    const backend = new MockBackend({ empty: true });
    const start = await backend.getSettings();
    const send = (patch: AppSettingsPatch) => backend.changeSettings(patch).then(() => true);
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
