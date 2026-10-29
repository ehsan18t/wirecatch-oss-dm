// The queue editor's rules card and Test a link: the rules written are what is saved, and Test a link answers for the rules as the form has them.

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

const AUTO = { name: /^Add matching downloads automatically/ };

/** Turns the rules on and checks each kind named ("By site"). */
async function useRules(...kinds: string[]) {
  const dialog = within(screen.getByRole('dialog'));
  await click(dialog.getByRole('switch', AUTO));
  for (const kind of kinds) await click(dialog.getByRole('checkbox', { name: kind }));
}

async function save() {
  await click(screen.getByRole('button', { name: /^(Save|Create)/ }));
}

async function tryLink(root: HTMLElement, url: string) {
  const scope = within(root);
  await typeInto(scope.getByLabelText('Test a link'), url);
  await click(scope.getByRole('button', { name: 'Try' }));
  return scope.getAllByRole('status').find((s) => s.textContent.includes('Queue'));
}

const addDownloadsOf = (app: Awaited<ReturnType<typeof renderApp>>['app'], name: string) =>
  app.queues.list.find((q) => q.settings.name === name)?.settings.add_downloads;

describe('the rules card', () => {
  it('saves an auto queue with the rules of each kind as they were written', async () => {
    const { app, dialog } = await openQueueDialog(null);
    await typeInto(dialog.getByLabelText('Name'), 'Videos');
    expect(dialog.queryByRole('checkbox', { name: 'By site' })).toBeNull();
    await useRules('By site', 'By file name', 'By file type', 'By category', 'By protocol');
    await typeInto(dialog.getByLabelText('Sites'), 'YouTube.com');
    await typeInto(dialog.getByLabelText('File types'), '.ISO tar.gz');
    await typeInto(dialog.getByLabelText('File names'), String.raw`^clip-\d+$`);
    await click(dialog.getByRole('radio', { name: 'Regular expression' }));
    await click(dialog.getByRole('button', { name: 'Video' }));
    await click(dialog.getByRole('button', { name: 'FTP and FTPS' }));
    await save();

    expect(addDownloadsOf(app, 'Videos')).toEqual({
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

  it('shows a kind of rule only while it is checked', async () => {
    const { dialog } = await openQueueDialog(null);
    await useRules();
    expect(dialog.queryByLabelText('Sites')).toBeNull();
    expect(dialog.getByText(/Add a rule/)).toBeInTheDocument();
    await click(dialog.getByRole('checkbox', { name: 'By site' }));
    expect(dialog.getByLabelText('Sites')).toBeInTheDocument();
  });

  it('keeps the rules of a queue switched off, as Auto off', async () => {
    const { app, dialog } = await openQueueDialog('Work files');
    await useRules('By site');
    await typeInto(dialog.getByLabelText('Sites'), 'example.com');
    await click(dialog.getByRole('switch', AUTO));
    await click(dialog.getByRole('switch', AUTO));
    expect(dialog.getByRole('button', { name: /Remove example\.com/ })).toBeInTheDocument();
    await click(dialog.getByRole('switch', AUTO));
    await save();
    expect(addDownloadsOf(app, 'Work files')).toMatchObject({
      kind: 'auto',
      enabled: false,
      sites: ['example.com'],
    });
  });

  it('saves a queue switched off with no rules as manual', async () => {
    const { app, dialog } = await openQueueDialog('Work files');
    await click(dialog.getByRole('switch', AUTO));
    await click(dialog.getByRole('switch', AUTO));
    await save();
    expect(addDownloadsOf(app, 'Work files')).toEqual({ kind: 'manual' });
  });

  it('brings an unchecked kind back when checked again, and drops it when saved unchecked', async () => {
    const { app, dialog } = await openQueueDialog(null);
    await typeInto(dialog.getByLabelText('Name'), 'Mixed');
    await useRules('By site', 'By file type');
    await typeInto(dialog.getByLabelText('Sites'), 'example.org');
    await typeInto(dialog.getByLabelText('File types'), 'iso');
    await click(dialog.getByRole('checkbox', { name: 'By site' }));
    expect(dialog.queryByLabelText('Sites')).toBeNull();
    await click(dialog.getByRole('checkbox', { name: 'By site' }));
    expect(dialog.getByRole('button', { name: /Remove example\.org/ })).toBeInTheDocument();
    await click(dialog.getByRole('checkbox', { name: 'By site' }));
    await save();
    expect(addDownloadsOf(app, 'Mixed')).toMatchObject({
      kind: 'auto',
      enabled: true,
      sites: [],
      file_types: ['iso'],
    });
  });
});

describe('Test a link', () => {
  it('tries the rules as the form has them, before they are saved', async () => {
    const { dialog } = await openQueueDialog(null);
    await typeInto(dialog.getByLabelText('Name'), 'Videos');
    await useRules('By site');
    await typeInto(dialog.getByLabelText('Sites'), 'youtube.com');
    const result = await tryLink(screen.getByRole('dialog'), 'https://www.youtube.com/watch?v=1');
    expect(result).toHaveTextContent('Queue: Videos. Its rules match youtube.com.');
    await typeInto(dialog.getByRole('textbox', { name: 'Sites' }), 'vimeo.com');
    expect(dialog.queryByText(/^Queue: Videos/)).toBeNull();
    const elsewhere = await tryLink(screen.getByRole('dialog'), 'https://example.org/a.zip');
    expect(elsewhere).toHaveTextContent('Queue: Main. No queue’s rules match it.');
  });

  it('names a switched-off queue that would have matched the link', async () => {
    const { app, dialog } = await openQueueDialog('Work files');
    await useRules('By site');
    await typeInto(dialog.getByLabelText('Sites'), 'example.com');
    await click(dialog.getByRole('switch', AUTO));
    await save();
    app.openSettings('queues');
    await settle();
    const result = await tryLink(screen.getByRole('main'), 'https://files.example.com/a.zip');
    expect(result).toHaveTextContent('Queue: Main. No queue’s rules match it.');
    expect(result).toHaveTextContent('Work files would match example.com, but its rules are off.');
  });

  it('marks the link when it is the link that is wrong, and says why until it changes', async () => {
    const { dialog } = await openQueueDialog(null);
    expect(dialog.queryByLabelText('Test a link')).toBeNull();
    await useRules('By site');
    expect(dialog.queryByLabelText('Test a link')).toBeNull();
    await typeInto(dialog.getByLabelText('Sites'), 'example.org');
    await typeInto(dialog.getByLabelText('Test a link'), 'not a link');
    await click(dialog.getByRole('button', { name: 'Try' }));
    expect(dialog.getByText(/is not a valid link/)).toBeInTheDocument();
    expect(dialog.getByLabelText('Test a link')).toHaveAttribute('aria-invalid', 'true');
    await fireEvent.input(dialog.getByLabelText('Test a link'), { target: { value: 'https://' } });
    await settle();
    expect(dialog.queryByText(/is not a valid link/)).toBeNull();
  });

  it('says a rule it cannot read without marking the link', async () => {
    const { dialog } = await openQueueDialog(null);
    await useRules('By file name');
    await typeInto(dialog.getByRole('textbox', { name: 'File names' }), '*-final.*');
    await click(dialog.getByRole('radio', { name: 'Regular expression' }));
    await tryLink(screen.getByRole('dialog'), 'https://example.org/a-final.zip');
    expect(dialog.getByText(/is not a regular expression/)).toBeInTheDocument();
    expect(dialog.getByLabelText('Test a link')).not.toHaveAttribute('aria-invalid', 'true');
  });

  it('tries the saved rules in Settings', async () => {
    const { app, dialog } = await openQueueDialog('Work files');
    await useRules('By site');
    await typeInto(dialog.getByLabelText('Sites'), 'share.example.com');
    await save();
    app.openSettings('queues');
    await settle();
    expect(screen.getByRole('main')).toHaveTextContent('Adds share.example.com');
    const result = await tryLink(screen.getByRole('main'), 'https://share.example.com/report.pdf');
    expect(result).toHaveTextContent('Queue: Work files. Its rules match share.example.com.');
  });
});
