import { describe, expect, it } from 'vitest';
import { fileKind, typedExtension } from './file-names';

describe('file names', () => {
  it('says a kind of file by its extension, or by its category without one', () => {
    expect(fileKind('ubuntu-24.04.3-desktop-amd64.iso', 'Archives')).toBe('ISO file');
    expect(fileKind('README', 'Documents')).toBe('Documents');
    expect(fileKind('.bashrc', 'Other')).toBe('Other');
  });

  it('keeps an extension as typed without its dot, and nothing as nothing', () => {
    expect(typedExtension(' .MP4 ')).toBe('mp4');
    expect(typedExtension('.')).toBeNull();
  });
});
