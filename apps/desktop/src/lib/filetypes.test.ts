import { describe, expect, it } from 'vitest';
import { fileType, isProgramName } from './filetypes';

describe('file types that open without asking', () => {
  it('leave out documents that can run code or formulas', () => {
    for (const name of ['a.rtf', 'a.odt', 'a.ods', 'a.odp', 'a.csv', 'a.docm', 'a.xlsm']) {
      expect(isProgramName(name), name).toBe(true);
    }
    for (const name of ['a.docx', 'a.xlsx', 'a.pptx', 'a.pdf']) {
      expect(isProgramName(name), name).toBe(false);
    }
  });
});

describe('the type of a file', () => {
  it('is read the way Windows reads it', () => {
    expect(fileType('video.mp4. ')).toBe('mp4');
    expect(fileType('report.PDF.EXE')).toBe('exe');
    expect(fileType('README')).toBeNull();
    expect(fileType('trailing.')).toBeNull();
  });
});
