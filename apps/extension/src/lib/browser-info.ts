import type { BrowserInfo } from '@wirecatch/protocol';

/** One entry of `navigator.userAgentData.brands` (Chromium only). */
export interface Brand {
  brand: string;
  version: string;
}

export interface BrowserClues {
  /** From Firefox's `runtime.getBrowserInfo()`. */
  firefox?: { name: string; version: string } | null;
  /** From `navigator.userAgentData.brands` in Chromium browsers. */
  brands?: readonly Brand[] | null;
  /** True when `navigator.brave.isBrave` exists (Brave hides itself from the brand list). */
  isBrave?: boolean;
  userAgent?: string | null;
}

const BRAND_NAMES: readonly [RegExp, string][] = [
  [/edge/i, 'edge'],
  [/opera/i, 'opera'],
  [/brave/i, 'brave'],
  [/vivaldi/i, 'vivaldi'],
  [/yandex/i, 'yandex'],
  [/google chrome/i, 'chrome'],
  [/^chromium$/i, 'chromium'],
];

/** Works out which browser this is, for the `hello` call. Pure so every branch is testable. */
export function detectBrowser(clues: BrowserClues): BrowserInfo {
  if (clues.firefox) {
    return { name: clues.firefox.name.toLowerCase(), version: clues.firefox.version };
  }
  const brands = clues.brands ?? [];
  const chromium = brands.find((b) => /^chromium$/i.test(b.brand));
  if (clues.isBrave === true) {
    return { name: 'brave', version: chromium?.version ?? null };
  }
  for (const [pattern, name] of BRAND_NAMES) {
    const found = brands.find((b) => pattern.test(b.brand));
    if (found) return { name, version: found.version };
  }
  const ua = clues.userAgent ?? '';
  const fromUa: readonly [RegExp, string][] = [
    [/Edg\/([\d.]+)/, 'edge'],
    [/OPR\/([\d.]+)/, 'opera'],
    [/Vivaldi\/([\d.]+)/, 'vivaldi'],
    [/Firefox\/([\d.]+)/, 'firefox'],
    [/Chrome\/([\d.]+)/, 'chrome'],
  ];
  for (const [pattern, name] of fromUa) {
    const match = pattern.exec(ua);
    if (match) return { name, version: match[1] ?? null };
  }
  return { name: 'unknown', version: null };
}
