import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { describe, expect, it } from 'vitest';
import { isProgramName, SAFE_FILE_TYPES } from './filetypes';

describe('file types that open without asking', () => {
  it('are exactly the Rust list', () => {
    // Vitest runs from apps/desktop.
    const api = readFileSync(resolve(process.cwd(), 'src-tauri/src/api.rs'), 'utf8');
    const body = /pub const SAFE_EXTENSIONS: &\[&str\] = &\[([\s\S]*?)\];/.exec(api)?.[1] ?? '';
    const rust = [...body.matchAll(/"([^"]+)"/g)].map((m) => m[1]);
    expect(rust.length).toBeGreaterThan(20);
    expect([...SAFE_FILE_TYPES].sort()).toEqual(rust.sort());
  });

  it('leave out documents that can run code or formulas', () => {
    for (const name of ['a.rtf', 'a.odt', 'a.ods', 'a.odp', 'a.csv', 'a.docm', 'a.xlsm']) {
      expect(isProgramName(name), name).toBe(true);
    }
    for (const name of ['a.docx', 'a.xlsx', 'a.pptx', 'a.pdf']) {
      expect(isProgramName(name), name).toBe(false);
    }
  });
});
