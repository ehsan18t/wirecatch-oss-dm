// What the production build makes of the styles, which jsdom cannot show: it computes no styles, so a focus ring that resolves to no outline, or a colour resolved on the wrong element, passes every component test. These read the real output of Tailwind and Lightning CSS instead. The gallery's browser check (apps/desktop/e2e/gallery.spec.ts) then confirms both in a real engine.
import { beforeAll, describe, expect, it } from 'vitest';
import { button, iconButton } from './recipes/button';
import { FOCUS_RING, FOCUS_RING_INSET } from './recipes/control';
import { field } from './recipes/field';
import {
  buildCss,
  computedValue,
  parseCss,
  type ElementModel,
  type Stylesheet,
} from './testing/built-css';

const classes = (list: string) => list.split(/\s+/).filter((name) => name !== '');

/** Before the fix: Tailwind 4's `outline-none` sets the variable the ring's `outline-style` reads. */
const RING_UNDER_OUTLINE_NONE = `outline-none ${FOCUS_RING}`;

let sheet: Stylesheet;

beforeAll(async () => {
  sheet = parseCss(await buildCss(classes(RING_UNDER_OUTLINE_NONE)));
}, 60_000);

describe('the focus ring in the built CSS', () => {
  const focused = (list: string, more: Partial<ElementModel> = {}): ElementModel => ({
    classes: classes(list),
    states: ['focus-visible'],
    ...more,
  });

  it('is caught when it resolves to no outline', () => {
    expect(computedValue(sheet, focused(RING_UNDER_OUTLINE_NONE), 'outline-style')).toBe('none');
  });

  const rings: Record<string, string> = {
    FOCUS_RING,
    FOCUS_RING_INSET,
    'the secondary button': button(),
    'the primary button': button({ variant: 'primary' }),
    'the danger button': button({ variant: 'danger' }),
    'the link button': button({ variant: 'link' }),
    'the ghost icon button': iconButton(),
    'the active icon button': iconButton({ variant: 'secondary', active: true }),
  };

  it.each(Object.entries(rings))('%s draws a solid outline on keyboard focus', (_name, list) => {
    const element = focused(list);
    expect(computedValue(sheet, element, 'outline-style')).toBe('solid');
    expect(computedValue(sheet, element, 'outline-width')).toBe('2px');
  });

  it('keeps it in forced colours, where outlines are kept', () => {
    const element = focused(FOCUS_RING, { forcedColors: true });
    expect(computedValue(sheet, element, 'outline-style')).toBe('solid');
  });

  it('leaves a field to its accent halo, and gives it an outline in forced colours', () => {
    const box = classes(field());
    const normal: ElementModel = { classes: box, states: ['focus-within'] };
    expect(computedValue(sheet, normal, 'outline-style')).toBe('none');
    expect(computedValue(sheet, normal, '--tw-shadow')).toMatch(/^0 0 0 3px /);
    // Select's trigger is the box itself, focused from the keyboard.
    for (const states of [['focus-within'], ['focus-visible']] as const) {
      const forced: ElementModel = { classes: box, states, forcedColors: true };
      expect(computedValue(sheet, forced, 'outline-style')).toBe('solid');
      expect(computedValue(sheet, forced, 'outline-width')).toBe('2px');
      expect(computedValue(sheet, forced, 'outline-color')?.toLowerCase()).toBe('highlight');
    }
  });
});

/**
 * Custom properties whose value depends on where they are declared: on the scheme (a lowered `light-dark()` reads `--lightningcss-light` and `--lightningcss-dark`) or on the accent pair. A `var()` in a custom property is substituted on the element that declares it, and descendants inherit the result, so each such property must be declared again on every element that sets its own scheme or accent.
 */
const SCOPES = {
  '[data-scheme]': ['--lightningcss-light', '--lightningcss-dark'],
  '[data-accent]': ['--wc-accent-light', '--wc-accent-dark'],
} as const;
type Scope = keyof typeof SCOPES;
/** The properties each scope sets: they are where the dependence starts. */
const ROOTS = new Set<string>(Object.values(SCOPES).flat());

/** For each custom property the token rules declare, the scopes it depends on, followed through the properties it reads. */
function scopesOf(stylesheet: Stylesheet): Map<string, Set<Scope>> {
  const reads = new Map<string, Set<string>>();
  for (const rule of stylesheet.rules) {
    for (const [name, value] of rule.declarations) {
      if (!name.startsWith('--')) continue;
      const names = reads.get(name) ?? new Set<string>();
      for (const match of value.matchAll(/var\((--[\w-]+)/g)) names.add(match[1] ?? '');
      reads.set(name, names);
    }
  }
  const scopes = new Map<string, Set<Scope>>();
  const resolve = (name: string, path: Set<string>): Set<Scope> => {
    const known = scopes.get(name);
    if (known) return known;
    const found = new Set<Scope>();
    for (const [scope, roots] of Object.entries(SCOPES) as [Scope, readonly string[]][]) {
      if (roots.includes(name)) found.add(scope);
    }
    for (const read of reads.get(name) ?? []) {
      if (path.has(read)) continue;
      for (const scope of resolve(read, new Set([...path, read]))) found.add(scope);
    }
    scopes.set(name, found);
    return found;
  };
  for (const name of reads.keys()) resolve(name, new Set([name]));
  return scopes;
}

describe('scoped schemes and accents in the built CSS', () => {
  it('lowers light-dark(), as the production build does', () => {
    const lowered = sheet.rules.some((rule) =>
      rule.declarations.some(([, value]) => value.includes('--lightningcss-light')),
    );
    expect(lowered).toBe(true);
  });

  it('declares each scheme- or accent-dependent token again on every element that picks its own', () => {
    const scopes = scopesOf(sheet);
    // Token rules only: a utility declares its variables on the element it styles, where they resolve correctly.
    const tokenRules = sheet.rules.filter((rule) => !rule.selectors.some((s) => s.includes('.')));
    const missing = tokenRules.flatMap((rule) =>
      rule.declarations.flatMap(([name]) => {
        if (ROOTS.has(name)) return [];
        const needed = [...(scopes.get(name) ?? [])].filter((scope) => {
          // A rule written for one accent sets that accent's own values; it is the scope, not inside it.
          if (
            scope === '[data-accent]' &&
            rule.selectors.every((s) => s.includes('[data-accent='))
          ) {
            return false;
          }
          return !rule.selectors.some((selector) => selector.endsWith(scope));
        });
        return needed.map((scope) => `${rule.selectors.join(', ')} { ${name} } misses ${scope}`);
      }),
    );
    expect(missing).toEqual([]);
  });
});

/** Selectors some supported engine does not know yet (WebKitGTK before Safari 18.4's engine, older Firefox): in a list, one of these makes that engine drop the whole rule. */
const NEWER_SELECTORS = ['::details-content', '::picker(', '::scroll-marker', ':open'];

describe('selector lists in the built CSS', () => {
  it('keep newer selectors out of shared lists, so an engine that lacks one keeps the rest', () => {
    const shared = sheet.rules
      .filter((rule) => rule.selectors.length > 1)
      .filter((rule) =>
        rule.selectors.some((selector) =>
          NEWER_SELECTORS.some((newer) => selector.includes(newer)),
        ),
      )
      .map((rule) => rule.selectors.join(', '));
    expect(shared).toEqual([]);
  });
});
