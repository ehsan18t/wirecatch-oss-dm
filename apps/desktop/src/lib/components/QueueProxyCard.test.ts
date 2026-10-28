// The queue editor's proxy card (D73.2, D73.7): a queue follows the app's proxy and exceptions or
// has its own, and New proxy saves a profile and picks it without leaving the editor.

import { fireEvent, screen, within } from '@testing-library/svelte';
import { describe, expect, it } from 'vitest';
import { renderApp, settle } from '$lib/testing/setup';

async function openQueueDialog(name: string) {
  const r = await renderApp();
  const id = r.app.queues.list.find((q) => q.settings.name === name)?.id ?? null;
  r.app.openDialog({ kind: 'queue', id });
  await settle();
  return r;
}

async function click(target: HTMLElement) {
  await fireEvent.click(target);
  await settle();
}

async function type(field: HTMLElement, text: string) {
  await fireEvent.input(field, { target: { value: text } });
  await fireEvent.blur(field);
  await settle();
}

async function pickProxy(name: RegExp | string) {
  await click(screen.getByRole('combobox', { name: 'Proxy' }));
  await click(screen.getByRole('option', { name }));
}

const queueDialog = () => within(screen.getAllByRole('dialog')[0] ?? document.body);

describe('a queue’s proxy', () => {
  it('follows the app’s by default and saves one of its own', async () => {
    const { app } = await openQueueDialog('Overnight');
    expect(screen.getByRole('combobox', { name: 'Proxy' })).toHaveTextContent(
      'Same as the app (now the computer’s proxy)',
    );
    await pickProxy('Office');
    await click(queueDialog().getByRole('button', { name: 'Save' }));
    const overnight = app.queues.list.find((q) => q.settings.name === 'Overnight');
    expect(overnight?.settings.proxy).toEqual({
      source: 'own',
      value: { kind: 'profile', id: app.proxies.list[0]?.id },
    });
    expect(overnight?.settings.exceptions).toEqual({ source: 'follow_app' });
  });

  it('chooses its own hosts to go directly to, starting from the app’s', async () => {
    const { app } = await openQueueDialog('Work files');
    await click(screen.getByRole('radio', { name: 'Choose' }));
    expect(
      screen.getByRole('checkbox', { name: 'The hosts the computer’s proxy skips' }),
    ).toBeChecked();
    // The app lists no hosts of its own in the sample, so there is nothing to take from it.
    expect(screen.getByRole('checkbox', { name: /^The app’s hosts: none yet/ })).toBeDisabled();
    await click(screen.getByRole('checkbox', { name: 'This queue’s hosts' }));
    const hosts = screen.getByPlaceholderText('Add a host, like intranet.example.com');
    await fireEvent.input(hosts, { target: { value: 'intranet.example' } });
    await fireEvent.keyDown(hosts, { key: 'Enter' });
    await settle();
    await click(queueDialog().getByRole('button', { name: 'Save' }));
    const work = app.queues.list.find((q) => q.settings.name === 'Work files');
    expect(work?.settings.exceptions).toEqual({
      source: 'own',
      value: { system: true, app_hosts: true, use_hosts: true, hosts: ['intranet.example'] },
    });
  });

  it('has no hosts to go around while it uses no proxy', async () => {
    await openQueueDialog('Work files');
    expect(screen.getByRole('radio', { name: 'Same as the app' })).toBeInTheDocument();
    await pickProxy('No proxy');
    expect(screen.queryByRole('radio', { name: 'Same as the app' })).toBeNull();
  });

  it('saves a new proxy from the editor and picks it', async () => {
    const { app } = await openQueueDialog('Overnight');
    await click(screen.getByRole('button', { name: 'New proxy' }));
    const created = within(screen.getAllByRole('dialog').at(-1) ?? document.body);
    await type(created.getByLabelText('Name (optional)'), 'Night proxy');
    await type(created.getByLabelText('Server'), 'night.example');
    expect(created.getByText('Saved for the whole app, and used by Overnight.')).toBeInTheDocument();
    await click(created.getByRole('button', { name: 'Save and use' }));
    expect(screen.getAllByRole('dialog')).toHaveLength(1);
    expect(screen.getByRole('combobox', { name: 'Proxy' })).toHaveTextContent('Night proxy');
    expect(app.proxies.list.map((p) => p.settings.name)).toContain('Night proxy');
  });
});
