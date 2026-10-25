// The app's proxy setting (D71) as the settings show it: its parts in words, and what a test found.

import type {
  ProxyKind,
  ProxyMode,
  ProxyRole,
  ProxyServer,
  ProxySettings,
  ProxyTestResult,
} from '@wirecatch/protocol';

/** A proxy with nothing entered. */
export function emptyServer(kind: ProxyKind = 'http'): ProxyServer {
  return { kind, host: '', port: 0, user: '', has_password: false };
}

/** Whether `a` and `b` name the same proxy and login, so a password saved for one is the other's, as the core decides. */
export function sameProxy(a: ProxyServer, b: ProxyServer): boolean {
  return (
    a.kind === b.kind &&
    a.host.trim().toLowerCase() === b.host.trim().toLowerCase() &&
    a.port === b.port &&
    a.user === b.user
  );
}

/** The setting of a fresh install: the computer's proxy. */
export function defaultProxySettings(): ProxySettings {
  return {
    mode: 'system',
    per_protocol: false,
    all: emptyServer(),
    http: emptyServer(),
    https: emptyServer(),
    ftp: emptyServer(),
    socks: emptyServer('socks5'),
    exceptions: [],
    ftp_direct: false,
  };
}

export const MODE_OPTIONS: readonly { value: ProxyMode; label: string; description: string }[] = [
  {
    value: 'system',
    label: 'Use the computer’s proxy',
    description: 'The proxy set in your network settings, or in the environment.',
  },
  { value: 'direct', label: 'No proxy', description: 'Connect to every server directly.' },
  { value: 'manual', label: 'Use this proxy', description: 'A proxy server you enter below.' },
];

export const KIND_OPTIONS: readonly { value: ProxyKind; label: string }[] = [
  { value: 'http', label: 'HTTP' },
  { value: 'https', label: 'HTTPS' },
  { value: 'socks5', label: 'SOCKS5' },
];

/** The proxies set one per protocol, in the order the settings show them, each with its name. */
export const PER_PROTOCOL: readonly { role: ProxyRole; label: string }[] = [
  { role: 'http', label: 'HTTP links' },
  { role: 'https', label: 'HTTPS links' },
  { role: 'ftp', label: 'FTP links' },
  { role: 'socks', label: 'SOCKS5, for all other links' },
];

/** The highest port a server can listen on. */
export const MAX_PORT = 65_535;
/** The port SOCKS proxies listen on (RFC 1928). */
const SOCKS_PORT = 1080;
/** The port HTTP proxies most often listen on. */
const HTTP_PROXY_PORT = 8080;

/** The port a proxy of `kind` most often listens on, for a port not entered yet. */
export function usualPort(kind: ProxyKind): number {
  return kind === 'socks5' ? SOCKS_PORT : HTTP_PROXY_PORT;
}

/** What a test found, in words, and how it reads; `loginEntered` says whether a login was given to refuse. */
export function testResultText(
  result: ProxyTestResult,
  loginEntered: boolean,
): {
  tone: 'success' | 'danger' | 'warning';
  text: string;
} {
  switch (result.outcome) {
    case 'works':
      return { tone: 'success', text: 'The proxy works. A test request went through it.' };
    case 'login_refused':
      return {
        tone: 'danger',
        text: loginEntered
          ? 'The proxy refused the login. Check the user name and password.'
          : 'The proxy needs a login. Enter its user name and password.',
      };
    case 'unreachable':
      return {
        tone: 'danger',
        text: `The proxy could not be reached: ${result.reason.replace(/\.$/, '')}.`,
      };
    case 'timed_out':
      return {
        tone: 'warning',
        text: 'The proxy did not answer in time. Check its address and port.',
      };
  }
}
