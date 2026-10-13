import { parseUrl } from './url';

/** Host permissions the manifest asks for; the popup offers to grant them again if they were revoked. */
export const SITE_ORIGINS = ['http://*/*', 'https://*/*'];

/** The permission pattern that covers one address, for `permissions.contains`; null for non-web addresses. */
export function originPattern(url: string): string | null {
  const parsed = parseUrl(url);
  if (parsed === null) return null;
  const { protocol, hostname } = parsed;
  if ((protocol !== 'http:' && protocol !== 'https:') || hostname === '') return null;
  // Match patterns do not carry a port; the host pattern covers every port.
  return `${protocol}//${hostname}/*`;
}
