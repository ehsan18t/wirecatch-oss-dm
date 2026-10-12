// Opening finished files: a program asks first; anything else opens at once.

import { describe, expect, it } from 'vitest';
import { appState, mockBackend } from '$lib/testing/setup';
import { isProgramName } from '../filetypes';

async function finished(url: string) {
  const backend = mockBackend({ empty: true });
  const app = appState(backend);
  await app.init();
  const id = await backend.addDownload({
    url,
    queue: null,
    folder: null,
    file_name: null,
    start: true,
  });
  for (let i = 0; i < 2000 && app.downloads.get(id)?.status !== 'completed'; i++) {
    backend.tick(5000);
  }
  expect(app.downloads.get(id)?.status).toBe('completed');
  return { app, backend, id };
}

describe('opening a finished file', () => {
  it('asks before opening a program, and Open then opens it', async () => {
    const { app, backend, id } = await finished('https://downloads.example.com/Setup.EXE');
    await app.actions.openFile(id);
    expect(app.dialog).toEqual({ kind: 'open-program', id });
    expect(backend.opened).toEqual([]);

    await app.actions.openProgram(id);
    expect(app.dialog).toBeNull();
    expect(backend.opened).toEqual([id]);
  });

  it('opens other files at once', async () => {
    const { app, backend, id } = await finished('https://example.com/report.pdf');
    await app.actions.openFile(id);
    expect(app.dialog).toBeNull();
    expect(backend.opened).toEqual([id]);
  });

  it('refuses a program that skipped the question', async () => {
    const { backend, id } = await finished('https://example.com/install.sh');
    await expect(backend.openFile(id)).rejects.toThrow(/can run code/);
  });

  it('asks for every type not known to be safe, in any case', () => {
    const asks = ['a.exe', 'B.MSI', 'c.Ps1', 'd.bat', 'e.AppImage', 'f.lnk', 'g.jar'];
    asks.push(
      'h.msc',
      'i.appref-ms',
      'j.iso',
      'k.vhd',
      'l.docm',
      'm.unknown',
      'readme',
      'n.pdf.exe',
    );
    for (const name of asks) {
      expect(isProgramName(name), name).toBe(true);
    }
    for (const name of ['a.mp4', 'b.PDF', 'c.zip', 'd.tar.gz', 'video.mp4.', 'song.mp3 ']) {
      expect(isProgramName(name), name).toBe(false);
    }
  });
});
