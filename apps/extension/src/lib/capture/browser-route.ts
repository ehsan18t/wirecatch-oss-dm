import { MAX_DOWNLOAD_REDIRECTS } from '@wirecatch/protocol';
import type { DownloadItemLike } from './candidate';
import { nonEmpty } from '../text';
import { parseWebUrl } from '../url';

const MAX_RECENT_REQUESTS = 128;
const REQUEST_LIFETIME_MS = 60_000;

interface FileRequest {
  requestId: string;
  tabId: number;
  url: string;
  method: string;
  type: string;
  initiator?: string;
  originUrl?: string;
  documentUrl?: string;
}

export interface DownloadRoute {
  pageUrl: string | null;
  pageRedirectUrls?: string[];
  redirectUrls: string[];
}

interface ObservedRoute {
  url: string;
  earlier: string[];
  tab: Promise<SourceTab | null>;
  from: string | undefined;
  tabId: number;
  navigation: boolean;
  at: number;
}

function fullPage(url: URL | null): URL | null {
  return url === null || url.href === `${url.origin}/` ? null : url;
}

/** Keep a full referrer. When only its origin is reported, use matching tab context. */
function sourcePage(referrer: URL | null, tab: SourceTab, route: ObservedRoute): string | null {
  const fullReferrer = fullPage(referrer);
  if (fullReferrer !== null) return fullReferrer.href;
  const from = parseWebUrl(route.from ?? '');
  const origin = referrer?.origin ?? from?.origin;
  const page = [fullPage(from), parseWebUrl(tab.url ?? '')].find(
    (candidate) => candidate?.origin === origin,
  );
  return page?.href ?? null;
}

export interface SourceTab {
  url?: string;
  incognito: boolean;
}

/**
 * The download API has no tab or request id. Correlate its exact request
 * addresses with recent browser events, and fall back to its referrer when the
 * correlation is ambiguous. Only a download handed to the app leaves this cache.
 */
export class BrowserRoutes {
  readonly #requests = new Map<string, ObservedRoute>();
  readonly #tab: (id: number) => Promise<SourceTab | null>;
  readonly #now: () => number;

  constructor(tab: (id: number) => Promise<SourceTab | null>, now = Date.now) {
    this.#tab = tab;
    this.#now = now;
  }

  response(request: FileRequest): void {
    if (request.method !== 'GET' || request.tabId < 0) return;
    this.#remember(request, request.url);
  }

  redirect(request: FileRequest & { redirectUrl: string }): void {
    if (request.method !== 'GET' || request.tabId < 0) return;
    const route = this.#remember(request, request.redirectUrl);
    if (route.earlier.length < MAX_DOWNLOAD_REDIRECTS) route.earlier.push(request.url);
  }

  #remember(request: FileRequest, url: string): ObservedRoute {
    const known = this.#requests.get(request.requestId);
    const at = this.#now();
    const route: ObservedRoute = {
      url,
      earlier: known?.earlier ?? [],
      from: known?.from ?? request.originUrl ?? request.documentUrl ?? request.initiator,
      tab: known?.tab ?? this.#tab(request.tabId),
      tabId: request.tabId,
      navigation: request.type === 'main_frame',
      at,
    };
    this.#requests.delete(request.requestId);
    this.#requests.set(request.requestId, route);
    for (const [id, recent] of this.#requests) {
      if (at - recent.at <= REQUEST_LIFETIME_MS && this.#requests.size <= MAX_RECENT_REQUESTS)
        break;
      this.#requests.delete(id);
    }
    return route;
  }

  #matchingRequest(url: string, finalUrl: string | null): ObservedRoute | undefined {
    const recent = [...this.#requests.values()].filter((route) => {
      if (this.#now() - route.at > REQUEST_LIFETIME_MS) return false;
      const first = route.earlier[0] ?? route.url;
      return finalUrl === null
        ? first === url || route.url === url
        : first === url && finalUrl === route.url;
    });
    return recent.length === 1 ? recent[0] : undefined;
  }

  #pageRedirects(page: string | null, download: ObservedRoute): string[] | undefined {
    const address = parseWebUrl(page ?? '');
    if (address === null) return undefined;
    address.hash = '';
    const navigation = [...this.#requests.values()].findLast(
      (route) =>
        route !== download &&
        route.tabId === download.tabId &&
        route.navigation &&
        route.url === address.href &&
        download.at >= route.at &&
        download.at - route.at <= REQUEST_LIFETIME_MS,
    );
    return navigation?.earlier.slice();
  }

  #downloadRedirects(route: ObservedRoute, url: string, finalUrl: string | null): string[] {
    const redirects = route.earlier.slice();
    if (finalUrl === null && route.url !== url && redirects.length < MAX_DOWNLOAD_REDIRECTS) {
      redirects.push(route.url);
    }
    return redirects;
  }

  async forDownload(item: DownloadItemLike): Promise<DownloadRoute> {
    const referrer = parseWebUrl(item.referrer ?? '');
    const finalUrl = nonEmpty(item.finalUrl);
    const original = finalUrl !== null && finalUrl !== item.url ? [item.url] : [];
    const fallback = { pageUrl: fullPage(referrer)?.href ?? null, redirectUrls: original };
    const route = this.#matchingRequest(item.url, finalUrl);
    if (route === undefined) return fallback;
    const tab = await route.tab;
    if (tab?.incognito !== (item.incognito === true)) return fallback;
    const pageUrl = sourcePage(referrer, tab, route);
    return {
      pageUrl,
      pageRedirectUrls: this.#pageRedirects(pageUrl, route),
      redirectUrls: this.#downloadRedirects(route, item.url, finalUrl),
    };
  }
}
