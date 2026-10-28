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
const list = () => within(screen.getByRole('list', { name: 'Saved proxies' }));

describe('proxy profiles in settings', () => {
  it('chooses the app’s proxy from the saved profiles, with each one’s last test', async () => {
    const { app } = await openNetwork();
    const office = screen.getByRole('radio', { name: /^Office/ });
    expect(office.closest('label')).toHaveTextContent('Works, tested');
    await click(office);
    expect(app.settings?.proxy.proxy).toEqual({ kind: 'profile', id: app.proxies.list[0]?.id });
    expect(screen.getByRole('radio', { name: /^backup-proxy\.example:443/ })).toBeInTheDocument();
  });

  it('saves a new profile after testing it, and keeps its password out of sight', async () => {
    const { app } = await openNetwork();
    await click(screen.getByRole('button', { name: 'Add proxy' }));
    await type(dialog().getByLabelText('Name (optional)'), 'Lab');
    await type(dialog().getByLabelText('Server'), 'refuse.lab.example');
    await click(dialog().getByRole('button', { name: 'Test connection' }));
    expect(dialog().getByText(/The proxy needs a login/)).toBeInTheDocument();
    await type(dialog().getByLabelText('User name (optional)'), 'ada');
    await type(dialog().getByLabelText('Password'), 'secret');
    await click(dialog().getByRole('button', { name: 'Test connection' }));
    expect(dialog().getByText(/The proxy works/)).toBeInTheDocument();
    await click(dialog().getByRole('button', { name: 'Save proxy' }));
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
    expect(dialog().getByText(/was for the old server or user name/)).toBeInTheDocument();
  });

  it('deletes a profile in use, giving what used it the replacement', async () => {
    const { app } = await openNetwork();
    const work = app.queues.list.find((q) => q.settings.name === 'Work files');
    await click(list().getByRole('button', { name: 'Delete Office' }));
    const confirm = within(screen.getByRole('alertdialog'));
    expect(confirm.getByText(/Work files uses it/)).toBeInTheDocument();
    await click(confirm.getByRole('button', { name: 'Delete Office' }));
    expect(app.proxies.list.map((p) => p.settings.name)).not.toContain('Office');
    expect(app.queues.list.find((q) => q.id === work?.id)?.settings.proxy).toEqual({
      source: 'own',
      value: { kind: 'system' },
    });
  });

  it('copies the computer’s proxy once', async () => {
    const { app } = await openNetwork();
    const before = app.proxies.list.length;
    await click(screen.getByRole('button', { name: 'Copy from system' }));
    await click(screen.getByRole('button', { name: 'Copy from system' }));
    expect(app.proxies.list).toHaveLength(before + 1);
    expect(list().getByText('proxy.corp.example:3128')).toBeInTheDocument();
  });

  it('leaves out the hosts to go around while nothing uses a proxy', async () => {
    const { app } = await openNetwork();
    expect(screen.getByRole('switch', { name: 'These hosts' })).toBeInTheDocument();
    await click(screen.getByRole('radio', { name: /^No proxy/ }));
    // Work files still has the Office profile of its own and follows the app's exceptions.
    expect(screen.getByRole('switch', { name: 'These hosts' })).toBeInTheDocument();
    const work = app.queues.list.find((q) => q.settings.name === 'Work files');
    if (!work) throw new Error('no sample queue');
    await app.queues.save(work.id, {
      ...JSON.parse(JSON.stringify(work.settings)),
      proxy: { source: 'follow_app' },
    });
    await settle();
    expect(screen.queryByRole('switch', { name: 'These hosts' })).toBeNull();
    expect(screen.getByText(/nothing to go around/)).toBeInTheDocument();
  });
});
