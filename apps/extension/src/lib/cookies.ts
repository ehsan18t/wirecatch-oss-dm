/** The fields of a `cookies.Cookie` needed to build a `Cookie` header. */
export interface CookieLike {
  name: string;
  value: string;
}

/** `[{name: "a", value: "1"}, {name: "b", value: "2"}]` becomes `"a=1; b=2"`; no cookies gives null. */
export function cookieHeader(cookies: readonly CookieLike[]): string | null {
  if (cookies.length === 0) return null;
  return cookies.map((c) => (c.name === '' ? c.value : `${c.name}=${c.value}`)).join('; ');
}
