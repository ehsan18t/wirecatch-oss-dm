// The queue editor's "Takes new downloads" card and Try a link: what an auto queue claims is what is saved, and Try a link answers for the claims as the form has them.

import { fireEvent, screen, within } from '@testing-library/svelte';
import { describe, expect, it } from 'vitest';
import { renderApp, settle } from '$lib/testing/setup';

async function openQueueDialog(name: string | null) {
  const r = await renderApp();
  const id =
    name === null ? null : (r.app.queues.list.find((q) => q.settings.name === name)?.id ?? null);
  r.app.openDialog({ kind: 'queue', id });
  await settle();
  return { ...r, dialog: within(screen.getByRole('dialog')) };
}

async function typeInto(field: HTMLElement, text: string) {
  await fireEvent.input(field, { target: { value: text } });
  await fireEvent.blur(field);
  await settle();
}

async function click(target: HTMLElement) {
  await fireEvent.click(target);
  await settle();
}

async function save() {
  await click(screen.getByRole('button', { name: /^(Save|Create)/ }));
}

async function tryLink(root: HTMLElement, url: string) {
  const scope = within(root);
  await typeInto(scope.getByLabelText('Try a link'), url);
  await click(scope.getByRole('button', { name: 'Try' }));
  return scope.getAllByRole('status').find((s) => s.textContent.includes('Queue'));
}

describe('the claims card', () => {
  it('saves an auto queue with the claims of each kind as they were written', async () => {
    const { app, dialog } = await openQueueDialog(null);
    await typeInto(dialog.getByLabelText('Name'), 'Videos');
    expect(dialog.queryByLabelText('Sites')).toBeNull();
    await click(dialog.getByRole('radio', { name: 'Auto' }));
    await typeInto(dialog.getByLabelText('Sites'), 'YouTube.com');
    await typeInto(dialog.getByLabelText('File types'), '.ISO tar.gz');
    await typeInto(dialog.getByLabelText('File names'), String.raw`^clip-\d+$`);
    await click(dialog.getByRole('radio', { name: 'Regular expression' }));
    await click(dialog.getByRole('button', { name: 'Video' }));
    await click(dialog.getByRole('button', { name: 'FTP' }));
    await save();

    const made = app.queues.list.find((q) => q.settings.name === 'Videos');
    expect(made?.settings.takes).toEqual({
      kind: 'auto',
      enabled: true,
      sites: ['youtube.com'],
      file_names: [String.raw`^clip-\d+$`],
      file_name_syntax: 'regex',
      file_types: ['iso', 'tar.gz'],
      categories: ['video'],
      protocols: ['ftp'],
    });
  });

  it('keeps the claims while switched to Manual, and saves a manual queue without them', async () => {
    const { app, dialog } = await openQueueDialog('Work files');
    await click(dialog.getByRole('radio', { name: 'Auto' }));
    await typeInto(dialog.getByLabelText('Sites'), 'example.com');
    await click(dialog.getByRole('radio', { name: 'Manual' }));
    await click(dialog.getByRole('radio', { name: 'Auto' }));
    expect(dialog.getByRole('button', { name: /Remove example\.com/ })).toBeInTheDocument();
    await click(dialog.getByRole('radio', { name: 'Manual' }));
    await save();
    expect(app.queues.list.find((q) => q.settings.name === 'Work files')?.settings.takes).toEqual({
      kind: 'manual',
    });
  });

  it('keeps the claims of a queue whose claiming is switched off', async () => {
    const { app, dialog } = await openQueueDialog('Overnight');
    await click(dialog.getByRole('radio', { name: 'Auto' }));
    await typeInto(dialog.getByLabelText('Sites'), 'example.org');
    await click(dialog.getByRole('switch', { name: /^Claim new downloads/ }));
    await save();
    expect(
      app.queues.list.find((q) => q.settings.name === 'Overnight')?.settings.takes,
    ).toMatchObject({ kind: 'auto', enabled: false, sites: ['example.org'] });
  });
});

describe('Try a link', () => {
  it('tries the claims as the form has them, before they are saved', async () => {
    const { dialog } = await openQueueDialog(null);
    await typeInto(dialog.getByLabelText('Name'), 'Videos');
    await click(dialog.getByRole('radio', { name: 'Auto' }));
    await typeInto(dialog.getByLabelText('Sites'), 'youtube.com');
    const result = await tryLink(screen.getByRole('dialog'), 'https://www.youtube.com/watch?v=1');
    expect(result).toHaveTextContent('Queue: Videos. It takes youtube.com.');
    const elsewhere = await tryLink(screen.getByRole('dialog'), 'https://example.org/a.zip');
    expect(elsewhere).toHaveTextContent('Queue: Main. No queue claims it.');
  });

  it('names a switched-off queue that would have taken the link', async () => {
    const { dialog } = await openQueueDialog('Work files');
    await click(dialog.getByRole('radio', { name: 'Auto' }));
    await typeInto(dialog.getByLabelText('Sites'), 'example.com');
    await click(dialog.getByRole('switch', { name: /^Claim new downloads/ }));
    const result = await tryLink(screen.getByRole('dialog'), 'https://files.example.com/a.zip');
    expect(result).toHaveTextContent('Queue: Main. No queue claims it.');
    expect(result).toHaveTextContent('Work files would take example.com, but its claims are off.');
  });

  it('says why a link cannot be tried', async () => {
    const { dialog } = await openQueueDialog(null);
    await typeInto(dialog.getByLabelText('Try a link'), 'not a link');
    await click(dialog.getByRole('button', { name: 'Try' }));
    expect(dialog.getByText(/is not a valid link/)).toBeInTheDocument();
    expect(dialog.getByLabelText('Try a link')).toHaveAttribute('aria-invalid', 'true');
  });

  it('tries the saved claims in Settings', async () => {
    const { app, dialog } = await openQueueDialog('Work files');
    await click(dialog.getByRole('radio', { name: 'Auto' }));
    await typeInto(dialog.getByLabelText('Sites'), 'share.example.com');
    await save();
    app.openSettings('queues');
    await settle();
    const result = await tryLink(screen.getByRole('main'), 'https://share.example.com/report.pdf');
    expect(result).toHaveTextContent('Queue: Work files. It takes share.example.com.');
  });
});
