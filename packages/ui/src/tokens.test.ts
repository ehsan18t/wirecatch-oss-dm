// Holds tokens.css to the appearance contract, to WCAG AA contrast in both schemes and every accent, and to the names the TypeScript side reads from it.
import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { describe, expect, it } from 'vitest';
import { ACCENTS, SCHEMES } from './appearance';
import { GROUP_NAMES, THEME_NAMES } from './lib/cn';
import { MOTION_TOKENS } from './motion/motion';
import { contrast, mixOklab, over, parseColor, withAlpha, type Rgba } from './testing/color';

// Vitest runs from the package folder.
const css = readFileSync(resolve(process.cwd(), 'src/tokens.css'), 'utf8');

/** Every `--name: value;` declaration in the file, the last one winning. */
function declarations(text: string): Map<string, string> {
  const found = new Map<string, string>();
  for (const match of text.matchAll(/(--[\w-]+):\s*([^;]+);/g))
    found.set(match[1]!, match[2]!.trim());
  return found;
}

/** The declarations inside the rule whose selector list contains `selector`. */
function rule(selector: string): Map<string, string> {
  const start = css.indexOf(selector);
  if (start < 0) throw new Error(`no rule for ${selector}`);
  const open = css.indexOf('{', start);
  return declarations(css.slice(open + 1, css.indexOf('}', open)));
}

/** The colour tokens, declared on the root and on every element with its own scheme. */
const colours = rule('[data-scheme] {');

/** A colour token as each scheme sees it: a `light-dark(light, dark)` pair, or another token's pair at an opacity (`--alpha(var(--token) / 86%)`). */
function pair(name: string, value = colours.get(name)): [Rgba, Rgba] {
  const alpha = value ? /^--alpha\(var\((--[\w-]+)\) \/ ([\d.]+)%\)$/.exec(value) : null;
  if (alpha) {
    const [light, dark] = pair(alpha[1]!);
    const share = Number(alpha[2]) / 100;
    return [withAlpha(light, share), withAlpha(dark, share)];
  }
  const match = value ? /^light-dark\((.+?),\s*(.+)\)$/.exec(value) : null;
  if (!match) throw new Error(`${name} is not a light-dark() pair: ${String(value)}`);
  return [parseColor(match[1]!), parseColor(match[2]!)];
}

const TEXT = 4.5;
const UI = 3;
const WHITE = parseColor('#ffffff');
const ON_ACCENT_DARK = parseColor('#0b0d12');

describe('the appearance contract', () => {
  it.each(SCHEMES)('has a rule for the %s scheme', (scheme) => {
    expect(css).toContain(`[data-scheme='${scheme}']`);
  });

  it.each(ACCENTS)('has a rule for the %s accent', (accent) => {
    expect(css).toContain(`[data-accent='${accent}']`);
  });

  it('reads no window material, since every window is opaque (D45)', () => {
    expect(css).not.toContain('data-material');
  });

  it('keeps Wirecatch’s cyan exactly, as the default', () => {
    const cyan = rule("[data-accent='cyan'] {");
    expect(css).toMatch(/:root,\s*\[data-accent='cyan'\] \{/);
    expect(cyan.get('--wc-accent-light')).toBe('#0a6b70');
    expect(cyan.get('--wc-accent-dark')).toBe('#4fd1c7');
  });

  it('derives the system accent from --accent-system', () => {
    const system = rule("[data-accent='system'] {");
    expect(system.get('--wc-accent-light')).toContain('var(--accent-system');
    expect(system.get('--wc-accent-dark')).toContain('var(--accent-system');
  });
});

const NAMED = ACCENTS.filter((accent) => accent !== 'system');

const SIDES = [
  ['light', 0],
  ['dark', 1],
] as const;

describe.each(SIDES)('contrast in the %s scheme', (_scheme, side) => {
  const at = (name: string) => pair(name)[side];
  const base = at('--wc-base');
  const layer = over(at('--wc-layer'), base);
  const overlay = over(at('--wc-overlay'), layer);
  const raised = over(at('--wc-raised'), layer);
  const surfaces = { base, layer, overlay, raised };

  it.each(['--wc-ink', '--wc-ink-muted', '--wc-ink-subtle'])('%s reads on every surface', (ink) => {
    for (const [name, surface] of Object.entries(surfaces)) {
      expect(contrast(at(ink), surface), name).toBeGreaterThanOrEqual(TEXT);
    }
  });

  it.each(['--wc-success', '--wc-warning', '--wc-danger', '--wc-info'])(
    '%s reads as text and carries its own text',
    (status) => {
      expect(contrast(at(status), base)).toBeGreaterThanOrEqual(TEXT);
      expect(contrast(at(status), layer)).toBeGreaterThanOrEqual(TEXT);
      expect(contrast(at('--wc-on-fill'), at(status))).toBeGreaterThanOrEqual(TEXT);
    },
  );

  it.each(['--wc-progress-done', '--wc-progress-queued', '--wc-warning', '--wc-danger'])(
    'the %s progress fill stands out from its track at 3:1 (WCAG 1.4.11)',
    (fill) => {
      for (const surface of [layer, raised]) {
        const track = over(at('--wc-track'), surface);
        expect(contrast(at(fill), track)).toBeGreaterThanOrEqual(UI);
      }
    },
  );

  it('outlines controls at 3:1', () => {
    expect(contrast(at('--wc-line-strong'), base)).toBeGreaterThanOrEqual(UI);
    expect(contrast(at('--wc-line-strong'), layer)).toBeGreaterThanOrEqual(UI);
  });

  it.each(NAMED)('the %s accent reads as text, as a fill and as a focus ring', (accent) => {
    const tones = rule(`[data-accent='${accent}'] {`);
    const color = parseColor(tones.get(side === 0 ? '--wc-accent-light' : '--wc-accent-dark')!);
    const onAccent =
      side === 0
        ? WHITE
        : mixOklab(parseColor(tones.get('--wc-accent-dark')!), 0.14, ON_ACCENT_DARK);
    for (const [name, surface] of Object.entries(surfaces)) {
      expect(contrast(color, surface), `text on ${name}`).toBeGreaterThanOrEqual(TEXT);
    }
    expect(contrast(onAccent, color), 'text on the accent').toBeGreaterThanOrEqual(TEXT);
    const soft = over(withAlpha(color, 0.14), layer);
    expect(contrast(color, soft), 'text on its soft tint').toBeGreaterThanOrEqual(TEXT);
    const hover = mixOklab(color, 0.86, side === 0 ? parseColor('#000000') : WHITE);
    expect(contrast(onAccent, hover), 'text on the hovered accent').toBeGreaterThanOrEqual(TEXT);
  });
});

describe('names the TypeScript side reads', () => {
  const tokens = declarations(css);
  const names = (prefix: string) =>
    [...tokens.keys()]
      .filter((name) => name.startsWith(prefix) && !name.includes('--', 2))
      .map((name) => name.slice(prefix.length))
      .filter((name) => !name.includes('--'));

  it.each([
    ['--transition-duration-', MOTION_TOKENS.speeds],
    ['--transition-delay-', MOTION_TOKENS.delays],
    ['--ease-', MOTION_TOKENS.curves],
    ['--motion-distance-', MOTION_TOKENS.distances],
  ])('every %s token motion.ts reads exists', (prefix, wanted) => {
    expect(names(prefix)).toEqual(expect.arrayContaining([...wanted]));
  });

  it.each([
    ['text', '--text-'],
    ['spacing', '--spacing-'],
    ['radius', '--radius-'],
    ['shadow', '--shadow-'],
    ['blur', '--blur-'],
    ['container', '--container-'],
    ['breakpoint', '--breakpoint-'],
    ['ease', '--ease-'],
    ['animate', '--animate-'],
    ['tracking', '--tracking-'],
    ['leading', '--leading-'],
  ] as const)('class merging knows every %s token', (group, prefix) => {
    expect([...THEME_NAMES[group]].sort()).toEqual(names(prefix).sort());
  });

  it.each([
    ['w', '--width-'],
    ['z', '--z-index-'],
    ['duration', '--transition-duration-'],
    ['delay', '--transition-delay-'],
    ['opacity', '--opacity-'],
  ] as const)('class merging knows every %s token', (group, prefix) => {
    expect([...GROUP_NAMES[group]].sort()).toEqual(names(prefix).sort());
  });

  it('class merging knows every font family', () => {
    const families = names('--font-').filter((name) => !name.startsWith('weight-'));
    expect([...THEME_NAMES.font].sort()).toEqual(families.sort());
  });
});
