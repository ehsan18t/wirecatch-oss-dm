// Proxy profiles in Settings, Network (D73): the app's proxy is chosen from them, each is saved,
// tested and deleted with a replacement, and the computer's proxy is copied as one.

import { fireEvent, screen, within } from '@testing-library/svelte';
import { describe, expect, it } from 'vitest';
import { renderApp, settle } from '$lib/testing/setup';

async function openNetwork() {
  const r = await renderApp();
  r.app.openSettingsSection('network');
  await settle();
  return r;
}

async function click(element: HTMLElement) {
  await fireEvent.click(element);
  await settle();
}

async function type(field: HTMLElement, value: string) {
  await fireEvent.input(field, { target: { value } });
  await fireEvent.blur(field);
  await settle();
}

const dialog = () => within(screen.getByRole('dialog'));
const list = () => within(screen.getByRole('list', { name: 'Proxy servers' }));

describe('proxy profiles in settings', () => {
  it('lists each saved profile once, with its last test and what uses it', async () => {
    const { app } = await openNetwork();
    expect(list().getByText(/^Works, tested /)).toBeInTheDocument();
    expect(list().getByText('Used by Work files')).toBeInTheDocument();
    expect(list().getByText('backup-proxy.example:443')).toBeInTheDocument();
    expect(list().getAllByText('Not used yet')).toHaveLength(1);
    const office = app.proxies.list[0];
    if (!office || !app.settings) throw new Error('no sample proxy');
    app.settings.proxy.proxy = { kind: 'profile', id: office.id };
    await settle();
    expect(
      list().getByText('Used by the global setting (Main, Overnight) and Work files'),
    ).toBeInTheDocument();
  });

  it('saves a new profile after testing it, and keeps its password out of sight', async () => {
    const { app } = await openNetwork();
    await click(screen.getByRole('button', { name: 'Add proxy' }));
    await type(dialog().getByLabelText('Name (optional)'), 'Lab');
    await type(dialog().getByLabelText('Server'), 'refuse.lab.example');
    await click(dialog().getByRole('button', { name: 'Test connection' }));
    expect(dialog().getByText(/The proxy requires a user name and password/)).toBeInTheDocument();
    await type(dialog().getByLabelText('User name (optional)'), 'ada');
    await type(dialog().getByLabelText('Password (optional)'), 'secret');
    await click(dialog().getByRole('button', { name: 'Test connection' }));
    expect(dialog().getByText(/The proxy works/)).toBeInTheDocument();
    await click(dialog().getByRole('button', { name: 'Save' }));
    const lab = app.proxies.list.find((p) => p.settings.name === 'Lab');
    expect(lab).toMatchObject({
      has_password: true,
      settings: { host: 'refuse.lab.example', port: 8080, user: 'ada' },
    });
    expect(JSON.stringify(app.proxies.list)).not.toContain('secret');
    expect(list().getByText('Lab')).toBeInTheDocument();
  });

  it('says a saved password does not follow the profile to another server', async () => {
    await openNetwork();
    await click(list().getByRole('button', { name: 'Edit Office' }));
    expect(dialog().getByText(/A password is saved/)).toBeInTheDocument();
    await type(dialog().getByLabelText('Server'), '10.0.0.6');
    expect(dialog().getByText(/belonged to the previous server or user name/)).toBeInTheDocument();
  });

  it('deletes a profile in use, giving what used it the replacement', async () => {
    const { app } = await openNetwork();
    const work = app.queues.list.find((q) => q.settings.name === 'Work files');
    await click(list().getByRole('button', { name: 'Delete Office' }));
    const confirm = within(screen.getByRole('alertdialog'));
    expect(confirm.getByText(/Used by Work files/)).toBeInTheDocument();
    await click(confirm.getByRole('button', { name: 'Delete proxy' }));
    expect(app.proxies.list.map((p) => p.settings.name)).not.toContain('Office');
    expect(app.queues.list.find((q) => q.id === work?.id)?.settings.proxy).toEqual({
      source: 'own',
      value: { kind: 'system' },
    });
  });

  it('copies the system proxy once', async () => {
    const { app } = await openNetwork();
    const before = app.proxies.list.length;
    await click(screen.getByRole('button', { name: 'Copy from system' }));
    await click(screen.getByRole('button', { name: 'Copy from system' }));
    expect(app.proxies.list).toHaveLength(before + 1);
    expect(list().getByText('proxy.corp.example:3128')).toBeInTheDocument();
  });

  it('leaves out the hosts to go around while nothing uses a proxy', async () => {
    const { app } = await openNetwork();
    expect(screen.getByRole('checkbox', { name: /^These hosts/ })).toBeInTheDocument();
    if (!app.settings) throw new Error('no settings');
    app.settings.proxy.proxy = { kind: 'direct' };
    await settle();
    // Work files still has the Office profile of its own and follows the app's exceptions.
    expect(screen.getByRole('checkbox', { name: /^These hosts/ })).toBeInTheDocument();
    const work = app.queues.list.find((q) => q.settings.name === 'Work files');
    if (!work) throw new Error('no sample queue');
    const settings = JSON.parse(JSON.stringify(work.settings)) as typeof work.settings;
    await app.queues.save(work.id, { ...settings, proxy: { source: 'follow_app' } });
    await settle();
    expect(screen.queryByRole('checkbox', { name: /^These hosts/ })).toBeNull();
    expect(screen.getByText(/nothing to bypass/)).toBeInTheDocument();
  });
});
