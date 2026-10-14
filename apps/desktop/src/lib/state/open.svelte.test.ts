// Opening finished files: a program opens only once the person agrees to the warning the shell shows itself; anything else opens at once.

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
  it('opens a program only once the person agrees to the warning, which names its type and site', async () => {
    const { app, backend, id } = await finished('https://downloads.example.com/Setup.EXE');
    const warnings: string[] = [];
    backend.answerProgramWarning = (warning) => {
      warnings.push(warning);
      return false;
    };
    await app.actions.openFile(id);
    expect(backend.opened).toEqual([]);
    expect(app.dialog).toBeNull();
    expect(warnings).toHaveLength(1);
    expect(warnings[0]).toMatch(/This is a \.exe file, a type that can run code/);
    expect(warnings[0]).toMatch(/trust downloads\.example\.com/);

    backend.answerProgramWarning = () => true;
    await app.actions.openFile(id);
    expect(backend.opened).toEqual([id]);
  });

  it('opens other files without asking', async () => {
    const { app, backend, id } = await finished('https://example.com/report.pdf');
    backend.answerProgramWarning = () => {
      throw new Error('a pdf asked before opening');
    };
    await app.actions.openFile(id);
    expect(backend.opened).toEqual([id]);
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
