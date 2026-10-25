import { describe, expect, it, vi } from 'vitest';
import { appState, mockBackend } from '$lib/testing/setup';

async function make(options: { empty?: boolean } = {}) {
  const backend = mockBackend(options);
  const app = appState(backend);
  await app.init();
  return { app, backend, form: app.proxy };
}

describe('ProxyForm', () => {
  it('edits a copy of the saved setting until Save, and Discard goes back to it', async () => {
    const { app, form } = await make({ empty: true });
    expect(form.draft.mode).toBe('system');
    expect(form.dirty).toBe(false);

    form.edit((d) => {
      d.mode = 'manual';
      d.all.host = 'proxy.example';
      d.all.port = 3128;
    });
    expect(form.dirty).toBe(true);
    expect(app.settings?.proxy.mode).toBe('system');

    form.reset();
    expect(form.draft.mode).toBe('system');
    expect(form.dirty).toBe(false);
  });

  it('saves the setting with its password, which the window then only knows is set', async () => {
    const { app, backend, form } = await make({ empty: true });
    form.edit((d) => {
      d.mode = 'manual';
      d.all.host = 'proxy.example';
      d.all.port = 3128;
      d.all.user = 'alice';
    });
    form.setPassword('all', 'secret');
    const setProxy = vi.spyOn(backend, 'setProxy');
    await form.save();
    expect(setProxy).toHaveBeenCalledWith(
      expect.objectContaining({ passwords: { all: 'secret' }, restart_running: false }),
    );
    expect(form.dirty).toBe(false);
    expect(form.hasPassword('all')).toBe(true);
    await vi.waitFor(() => expect(app.settings?.proxy.all.has_password).toBe(true));
    expect(JSON.stringify(app.settings)).not.toContain('secret');

    form.edit((d) => (d.all.host = 'elsewhere.example'));
    expect(form.hasPassword('all')).toBe(false);
    form.edit((d) => (d.all.host = 'proxy.example'));
    expect(form.keepsSaved('all')).toBe(true);
    form.setPassword('all', null);
    expect(form.hasPassword('all')).toBe(false);
  });

  it('saves first, then asks whether to restart the running downloads on it', async () => {
    const { app, backend, form } = await make();
    expect(app.counts.status.active - app.counts.joining).toBeGreaterThan(0);
    form.edit((d) => (d.mode = 'direct'));
    const setProxy = vi.spyOn(backend, 'setProxy');
    await form.save();
    expect(setProxy).toHaveBeenLastCalledWith(expect.objectContaining({ restart_running: false }));
    expect(form.askingRestart).toBe(true);

    await form.restartRunning();
    expect(form.askingRestart).toBe(false);
    const restarted = setProxy.mock.lastCall?.[0];
    expect(restarted?.settings.mode).toBe('direct');
    expect(restarted?.passwords).toEqual({});
    expect(restarted?.restart_running).toBe(true);
  });

  it('asks nothing about running downloads when the setting is refused', async () => {
    const { form } = await make();
    form.edit((d) => (d.mode = 'manual'));
    await form.save();
    expect(form.askingRestart).toBe(false);
    expect(form.dirty).toBe(true);
  });

  it('tests the unsaved setting with the password typed', async () => {
    const { form } = await make({ empty: true });
    form.edit((d) => {
      d.mode = 'manual';
      d.all.host = 'refuse.example';
      d.all.port = 8080;
    });
    await form.test();
    expect(form.result?.outcome).toBe('login_refused');

    form.setPassword('all', 'secret');
    expect(form.result).toBeNull();
    await form.test();
    expect(form.result?.outcome).toBe('works');
  });

  it('copies the computer’s proxy into the form, to change before saving', async () => {
    const { form } = await make({ empty: true });
    await form.copyFromSystem();
    expect(form.draft.mode).toBe('manual');
    expect(form.draft.all.host).not.toBe('');
    expect(form.dirty).toBe(true);
    expect(form.notice).toContain('Passwords are not copied');
  });

  it('follows a setting saved elsewhere unless it is being edited', async () => {
    const { app, form } = await make({ empty: true });
    const elsewhere = (mode: 'direct' | 'system') => {
      const settings = structuredClone($state.snapshot(app.settings));
      if (!settings) throw new Error('settings did not load');
      settings.proxy.mode = mode;
      app.settingsArrived(settings);
    };
    elsewhere('direct');
    expect(form.draft.mode).toBe('direct');

    form.edit((d) => (d.mode = 'manual'));
    elsewhere('system');
    expect(form.draft.mode).toBe('manual');
  });
});
