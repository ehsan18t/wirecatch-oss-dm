// A download whose file is already in its folder, asked in its window (D88): Keep both is the main
// button, each answer reaches the core, and "Do this every time" saves Replace or Keep both.

import { questionId } from '@wirecatch/protocol';
import { render, screen } from '@testing-library/svelte';
import userEvent from '@testing-library/user-event';
import { describe, expect, it } from 'vitest';
import type { MockBackend } from '$lib/backend/mock';
import { mockBackend, settle, windowState } from '$lib/testing/setup';
import DownloadWindow from './DownloadWindow.svelte';

async function ask() {
  const b = mockBackend({ empty: true });
  b.simulateFileExists();
  const [asked] = await b.pendingFileExists();
  const { win, host } = windowState(b, { question: asked?.id ?? questionId(0) });
  await win.init();
  render(DownloadWindow, { props: { win } });
  await settle();
  return { b, win, host, asked };
}

async function press(name: string) {
  await userEvent.setup().click(screen.getByRole('button', { name }));
  await settle();
}

async function rowOf(b: MockBackend, id: number | undefined) {
  return (await b.listDownloads()).find((r) => r.id === id);
}

describe('a download whose file is already in its folder', () => {
  it('says which file, with Keep both in focus', async () => {
    const { asked } = await ask();
    expect(
      screen.getByRole('heading', { name: 'This file is already in the folder' }),
    ).toBeInTheDocument();
    expect(screen.getByText(asked?.file_name ?? '')).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Keep both' })).toHaveFocus();
    expect(screen.getByRole('checkbox', { name: 'Do this every time' })).not.toBeChecked();
  });

  it('keeps both under a new name and shows the progress', async () => {
    const { b, win, host, asked } = await ask();
    await press('Keep both');
    expect(host.becameDownload).toHaveBeenCalledTimes(1);
    expect(win.phase).toBe('progress');
    expect((await rowOf(b, asked?.download))?.file_name).toBe('attention-at-scale (1).pdf');
    expect(await b.pendingFileExists()).toEqual([]);
    expect((await b.getSettings()).when_file_exists).toBe('ask');
  });

  it('remembers Replace when asked to', async () => {
    const { b } = await ask();
    await userEvent.setup().click(screen.getByRole('checkbox', { name: 'Do this every time' }));
    await press('Replace');
    expect((await b.getSettings()).when_file_exists).toBe('replace');
  });

  it('cancels the download and closes, remembering nothing', async () => {
    const { b, host, asked } = await ask();
    await userEvent.setup().click(screen.getByRole('checkbox', { name: 'Do this every time' }));
    await press('Cancel');
    expect(host.close).toHaveBeenCalledTimes(1);
    expect((await rowOf(b, asked?.download))?.status).toBe('cancelled');
    expect((await b.getSettings()).when_file_exists).toBe('ask');
  });
});
