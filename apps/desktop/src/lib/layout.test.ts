import { describe, expect, it } from 'vitest';
import { cssPixels, DOCK_WIDTH, ROW_HEIGHT } from './layout';

describe('layout lengths from app.css', () => {
  it('reads the tokens the code shares with the styles', () => {
    expect(ROW_HEIGHT).toBe(44);
    // Sidebar, detail panel and the narrowest list beside it.
    expect(DOCK_WIDTH).toBe(236 + 316 + 600);
  });

  it('matches the whole token name, not one that starts the same', () => {
    const css = ':root {\n  --row-height: 50px;\n  --row: 30px;\n}';
    expect(cssPixels('--row', css)).toBe(30);
    expect(cssPixels('--row-height', css)).toBe(50);
  });

  it('fails loudly for a missing token or one not in pixels', () => {
    expect(() => cssPixels('--nowhere')).toThrow('--nowhere');
    expect(() => cssPixels('--gap', ':root { --gap: 1rem; }')).toThrow('--gap');
  });
});
