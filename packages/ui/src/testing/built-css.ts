// The package's styles as a production build makes them, and enough of the cascade to read a property off them. jsdom computes no styles, so what only the built CSS shows (a focus ring that resolves to `outline-style: none`, a colour resolved on the wrong element) is checked here, over the real output of Tailwind and Lightning CSS.
import { join } from 'node:path';
import { compile, optimize } from '@tailwindcss/node';
import { Scanner } from '@tailwindcss/oxide';
import { Features, transform } from 'lightningcss';

const SOURCE = join(import.meta.dirname, '..');
const ENTRY = "@import './tokens.css';\n@import './base.css';";

/**
 * tokens.css and base.css compiled with every utility the package's sources use, plus `extra`, the way the apps' builds do: Tailwind, its optimizer, then Lightning CSS with `light-dark()` lowered to variables, as Vite's minifier does for its default browser targets (Chrome 111, Safari 16.4).
 */
export async function buildCss(extra: readonly string[] = []): Promise<string> {
  const compiler = await compile(ENTRY, { base: SOURCE, onDependency: () => undefined });
  const candidates = new Scanner({ sources: compiler.sources }).scan();
  const optimized = optimize(compiler.build([...candidates, ...extra])).code;
  return transform({
    filename: 'built.css',
    code: Buffer.from(optimized),
    include: Features.LightDark,
  }).code.toString();
}

/** One style rule of a stylesheet, with where it sits. */
export interface CssRule {
  selectors: string[];
  declarations: (readonly [name: string, value: string])[];
  /** The cascade layer, or null outside any. */
  layer: string | null;
  /** The `@media` and `@supports` conditions around it. */
  conditions: string[];
}

export interface Stylesheet {
  rules: CssRule[];
  /** Layer names, in the order they were first declared. */
  layers: string[];
  /** The initial value of each registered custom property. */
  initial: Map<string, string>;
}

/** Splits `text` on `separator` where it is not inside brackets or quotes. */
function splitTop(text: string, separator: string): string[] {
  const parts: string[] = [];
  let depth = 0;
  let quote: string | null = null;
  let start = 0;
  for (let i = 0; i < text.length; i++) {
    const char = text.charAt(i);
    if (quote !== null) {
      if (char === quote) quote = null;
    } else if (char === '"' || char === "'") quote = char;
    else if ('([{'.includes(char)) depth++;
    else if (')]}'.includes(char)) depth--;
    else if (char === separator && depth === 0) {
      parts.push(text.slice(start, i));
      start = i + 1;
    }
  }
  parts.push(text.slice(start));
  return parts.map((part) => part.trim()).filter((part) => part !== '');
}

function declarations(body: string): CssRule['declarations'] {
  return splitTop(body, ';').flatMap((part) => {
    const colon = part.indexOf(':');
    return colon === -1
      ? []
      : [[part.slice(0, colon).trim(), part.slice(colon + 1).trim()] as const];
  });
}

/** The index just past the block that opens at `open`. */
function blockEnd(css: string, open: number): number {
  let depth = 0;
  for (let i = open; i < css.length; i++) {
    if (css[i] === '{') depth++;
    else if (css[i] === '}') {
      depth--;
      if (depth === 0) return i + 1;
    }
  }
  return css.length;
}

/** Reads a flat stylesheet (no nesting, no comments), as Lightning CSS writes one. */
export function parseCss(css: string): Stylesheet {
  const sheet: Stylesheet = { rules: [], layers: [], initial: new Map() };
  const addLayers = (names: string) => {
    for (const name of splitTop(names, ',')) {
      if (!sheet.layers.includes(name)) sheet.layers.push(name);
    }
  };
  const block = (prelude: string, body: string, layer: string | null, conditions: string[]) => {
    if (prelude.startsWith('@layer')) {
      const name = prelude.slice('@layer'.length).trim();
      addLayers(name);
      walk(body, name, conditions);
    } else if (prelude.startsWith('@media') || prelude.startsWith('@supports')) {
      walk(body, layer, [...conditions, prelude]);
    } else if (prelude.startsWith('@property')) {
      const initial = declarations(body).find(([name]) => name === 'initial-value');
      if (initial) sheet.initial.set(prelude.slice('@property'.length).trim(), initial[1]);
    } else if (!prelude.startsWith('@')) {
      const selectors = splitTop(prelude, ',');
      sheet.rules.push({ selectors, declarations: declarations(body), layer, conditions });
    }
  };
  const walk = (text: string, layer: string | null, conditions: string[]) => {
    let i = 0;
    for (;;) {
      const open = text.indexOf('{', i);
      if (open === -1) return;
      const statement = text.indexOf(';', i);
      if (statement !== -1 && statement < open) {
        const prelude = text.slice(i, statement).trim();
        if (prelude.startsWith('@layer')) addLayers(prelude.slice('@layer'.length));
        i = statement + 1;
        continue;
      }
      const end = blockEnd(text, open);
      block(text.slice(i, open).trim(), text.slice(open + 1, end - 1), layer, conditions);
      i = end;
    }
  };
  walk(css, null, []);
  return sheet;
}

/** The interactive states an element can be in for `computedValue`. */
export type ElementState = 'focus' | 'focus-visible' | 'focus-within';

/** A bare element with classes and states, in a window that may be in forced-colours mode. */
export interface ElementModel {
  classes: readonly string[];
  states?: readonly ElementState[];
  forcedColors?: boolean;
}

/** A compound selector's parts: `.a\:b:focus-visible` is `.a\:b` and `:focus-visible`. */
function compoundParts(selector: string): string[] {
  const parts: string[] = [];
  let current = '';
  for (let i = 0; i < selector.length; i++) {
    const char = selector.charAt(i);
    if (char === '\\') {
      current += char + selector.charAt(i + 1);
      i++;
      continue;
    }
    const starts = char === '.' || (char === ':' && current !== ':');
    if (starts && current !== '') {
      parts.push(current);
      current = '';
    }
    current += char;
  }
  if (current !== '') parts.push(current);
  return parts;
}

/** `\:` and the other escapes in a class selector, undone. */
const unescape = (name: string) => name.replaceAll(/\\(.)/g, '$1');

/**
 * The specificity of `selector` if it matches the element, else null. Only what utilities and base.css use is understood: `*`, classes and the focus pseudo-classes. Anything else (`:root`, attributes, combinators, pseudo-elements) is taken as not matching, so the result errs towards fewer rules, never more.
 */
function matches(selector: string, element: ElementModel): number | null {
  const states = new Set<string>(element.states ?? []);
  if (states.has('focus-visible')) states.add('focus');
  if (states.has('focus')) states.add('focus-within');
  let specificity = 0;
  for (const part of compoundParts(selector)) {
    if (part === '*') continue;
    const known =
      (part.startsWith('.') && element.classes.includes(unescape(part.slice(1)))) ||
      (part.startsWith(':') && states.has(part.slice(1)));
    if (!known) return null;
    specificity++;
  }
  return specificity;
}

/** Forced colours as the element's window has it; feature queries as the supported browsers answer them; any other media query (reduced motion, hover) as off. */
function conditionHolds(condition: string, element: ElementModel): boolean {
  if (condition.includes('forced-colors')) return element.forcedColors === true;
  return condition.startsWith('@supports') && !condition.includes('not (');
}

function bestMatch(rule: CssRule, element: ElementModel): number | null {
  let best: number | null = null;
  for (const selector of rule.selectors) {
    const found = matches(selector, element);
    if (found !== null && (best === null || found > best)) best = found;
  }
  return best;
}

function outranks(a: readonly number[], b: readonly number[]): boolean {
  for (let i = 0; i < a.length; i++) {
    const difference = (a[i] ?? 0) - (b[i] ?? 0);
    if (difference !== 0) return difference > 0;
  }
  return true;
}

/**
 * The value `property` computes to on `element`, with `var()` resolved against the element's own custom properties (it has no parent to inherit from). Layers, then specificity, then order decide between declarations, as in a browser; rules outside a layer beat every layer.
 */
export function computedValue(
  sheet: Stylesheet,
  element: ElementModel,
  property: string,
): string | null {
  let winner: string | null = null;
  let winnerRank: number[] = [];
  for (const [order, rule] of sheet.rules.entries()) {
    if (rule.conditions.some((condition) => !conditionHolds(condition, element))) continue;
    const specificity = bestMatch(rule, element);
    if (specificity === null) continue;
    const layer = rule.layer === null ? sheet.layers.length : sheet.layers.indexOf(rule.layer);
    const rank = [layer, specificity, order];
    for (const [name, value] of rule.declarations) {
      if (name === property && (winner === null || outranks(rank, winnerRank))) {
        winner = value;
        winnerRank = rank;
      }
    }
  }
  const value = winner ?? sheet.initial.get(property) ?? null;
  return value === null ? null : resolveVars(sheet, element, value);
}

function resolveVars(sheet: Stylesheet, element: ElementModel, value: string): string {
  return value.replaceAll(
    /var\((--[\w-]+)(?:,\s*([^\s()][^()]*)?)?\)/g,
    (_match, name: string, fallback?: string) =>
      (computedValue(sheet, element, name) ?? fallback ?? '').trim(),
  );
}
