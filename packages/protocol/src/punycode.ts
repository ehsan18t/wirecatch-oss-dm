// Punycode (RFC 3492), the encoding of a Unicode label after `xn--` in a host name. Only decoding is here: what the URL parser of the runtime does with an `xn--` label differs between runtimes (Node 24.22 and current Chromium keep any such label as it is), so the site normalizer checks the label itself (D68).

const BASE = 36;
const T_MIN = 1;
const T_MAX = 26;
const SKEW = 38;
const DAMP = 700;
const INITIAL_BIAS = 72;
const INITIAL_N = 0x80;
const DELIMITER = '-';
/** The largest value a decoder must handle, as RFC 3492 section 6.4 sizes it. */
const MAX_INT = 0x7f_ff_ff_ff;
/** The largest code point. */
const MAX_CODE_POINT = 0x10_ff_ff;

/** The Punycode digits in the order of their values, 0 to 35. */
const DIGITS = 'abcdefghijklmnopqrstuvwxyz0123456789';
const UPPER_DIGITS = DIGITS.toUpperCase();
/** Any character beyond ASCII: never among the basic code points before the delimiter, and always in what a label after `xn--` stands for. */
export const NOT_ASCII = /\P{ASCII}/u;

/** The value of a Punycode digit (either case), or null for any other character. */
function digitValue(char: string): number | null {
  const value = Math.max(DIGITS.indexOf(char), UPPER_DIGITS.indexOf(char));
  return value === -1 ? null : value;
}

function adapt(delta: number, points: number, first: boolean): number {
  let d = first ? Math.floor(delta / DAMP) : Math.floor(delta / 2);
  d += Math.floor(d / points);
  let k = 0;
  while (d > ((BASE - T_MIN) * T_MAX) / 2) {
    d = Math.floor(d / (BASE - T_MIN));
    k += BASE;
  }
  return k + Math.floor(((BASE - T_MIN + 1) * d) / (d + SKEW));
}

/** One generalized variable-length integer from `input` at `at`: the value and where it ends, or null when the input ends early, holds a non-digit or overflows. */
function readInteger(
  input: string,
  at: number,
  bias: number,
): { value: number; end: number } | null {
  let value = 0;
  let weight = 1;
  let i = at;
  for (let k = BASE; ; k += BASE) {
    const char = input[i];
    if (char === undefined) return null;
    i += 1;
    const digit = digitValue(char);
    if (digit === null || digit > (MAX_INT - value) / weight) return null;
    value += digit * weight;
    const t = k <= bias ? T_MIN : Math.min(k - bias, T_MAX);
    if (digit < t) return { value, end: i };
    if (weight > MAX_INT / (BASE - t)) return null;
    weight *= BASE - t;
  }
}

/** The Unicode text `input` encodes (the part of a label after `xn--`), or null when it is not valid Punycode. */
export function decodePunycode(input: string): string | null {
  const split = input.lastIndexOf(DELIMITER);
  const basic = split === -1 ? '' : input.slice(0, split);
  if (NOT_ASCII.test(basic)) return null;
  const output = Array.from(basic, (char) => char.charCodeAt(0));
  let n = INITIAL_N;
  let bias = INITIAL_BIAS;
  let i = 0;
  let at = split === -1 ? 0 : split + 1;
  while (at < input.length) {
    const read = readInteger(input, at, bias);
    if (read === null || read.value > MAX_INT - i) return null;
    at = read.end;
    i += read.value;
    const points = output.length + 1;
    bias = adapt(read.value, points, output.length === basic.length);
    if (Math.floor(i / points) > MAX_INT - n) return null;
    n += Math.floor(i / points);
    i %= points;
    if (n > MAX_CODE_POINT) return null;
    output.splice(i, 0, n);
    i += 1;
  }
  return String.fromCodePoint(...output);
}
