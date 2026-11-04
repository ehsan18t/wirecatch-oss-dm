// Whether a host is covered by a site: a port of `wirecatch_protocol::hosts`, tested against the same cases.

const IPV4 = /^\d{1,3}(?:\.\d{1,3}){3}$/u;

function withoutRootDot(host: string): string {
  return host.replace(/\.+$/u, '').toLowerCase();
}

function isAddress(host: string): boolean {
  return host.includes(':') || IPV4.test(host);
}

/** Whether `host` is `site` or a name under it. Case and trailing dots never matter, and an address has no names under it. */
export function hostOnSite(host: string, site: string): boolean {
  const [name, on] = [withoutRootDot(host), withoutRootDot(site)];
  if (on === '') return false;
  if (name === on) return true;
  if (isAddress(name) || isAddress(on)) return false;
  return name.endsWith(`.${on}`) && name.length > on.length + 1;
}
