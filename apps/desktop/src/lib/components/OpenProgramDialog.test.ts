// The open-a-program question names the real file type and keeps the name from reordering the
// text around it.

import { screen, within } from '@testing-library/svelte';
import { afterEach, describe, expect, it } from 'vitest';
import { fileType } from '$lib/filetypes';
import type { AppState } from '$lib/state/app.svelte';
import { renderApp, settle } from '$lib/testing/setup';

let current: { app: AppState; unmount: () => void } | undefined;
afterEach(() => {
  current?.app.dispose();
  current?.unmount();
  current = undefined;
});

async function askToOpen(url: string) {
  const r = await renderApp({ empty: true });
  current = r;
  const id = await r.backend.addDownload({
    url,
    queue: null,
    folder: null,
    file_name: null,
    start: true,
  });
  for (let i = 0; i < 2000 && r.app.downloads.get(id)?.status !== 'completed'; i++) {
    r.backend.tick(5000);
  }
  await r.app.actions.openFile(id);
  await settle();
  return within(screen.getByRole('dialog'));
}

describe('opening a file that can run code', () => {
  it('says which type it is', async () => {
    const dialog = await askToOpen('https://example.com/files/report.pdf.exe');
    expect(dialog.getByText(/This is a/).textContent).toMatch(/This is a \.exe file/);
    // The name in the title is isolated (U+2068 ... U+2069).
    const title = dialog.getByRole('heading').textContent;
    expect(title).toBe(
      `Open "${String.fromCodePoint(0x2068)}report.pdf.exe${String.fromCodePoint(0x2069)}"?`,
    );
  });

  it('says so when the name has no type', async () => {
    const dialog = await askToOpen('https://example.com/files/installer');
    expect(dialog.getByText(/has no file type/)).toBeInTheDocument();
    // The name is isolated from the sentence around it.
    expect(dialog.getByText('installer').tagName).toBe('BDI');
  });

  it('reads the type the way Windows does', () => {
    expect(fileType('video.mp4. ')).toBe('mp4');
    expect(fileType('report.PDF.EXE')).toBe('exe');
    expect(fileType('README')).toBeNull();
    expect(fileType('trailing.')).toBeNull();
  });
});
