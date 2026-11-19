import type { CookieRequest } from '@wirecatch/protocol';
import { log } from '../diag/log';
import type { MediaRequest, MediaSendResult } from '../media/center';
import { MEDIA_UNREACHABLE } from '../media/messages';
import { refererFor } from '../referrer';
import { MEDIA_NO_SITE_ACCESS, MEDIA_PRIVATE_WINDOW, mediaAnswer } from './alerts';
import type { AppSide } from './app';
import type { HubDeps } from './deps';

export type MediaSenderDeps = Pick<
  HubDeps,
  'cookies' | 'hasSiteAccess' | 'newHandoverId' | 'privateSession'
>;

/** True when the extension may read cookies for `url`; a failed check counts as not granted. */
export async function hasAccess(
  deps: Pick<HubDeps, 'hasSiteAccess'>,
  url: string,
): Promise<boolean> {
  try {
    return await deps.hasSiteAccess(url);
  } catch (error) {
    log.warn('hub', 'Could not check access to the site; taking it as not granted', error);
    return false;
  }
}

export function newHandoverId(deps: Pick<HubDeps, 'newHandoverId'>): string {
  return deps.newHandoverId?.() ?? crypto.randomUUID();
}

/** Sends the media the person picked (the button, the popup, the shortcut) to the app. */
export class MediaSender {
  readonly #side: AppSide;
  readonly #deps: MediaSenderDeps;

  constructor(side: AppSide, deps: MediaSenderDeps) {
    this.#side = side;
    this.#deps = deps;
  }

  async send(request: MediaRequest): Promise<MediaSendResult> {
    const { target } = request;
    const { app, settings } = this.#side;
    if (!(await hasAccess(this.#deps, target.url))) {
      return { ok: false, message: MEDIA_NO_SITE_ACCESS };
    }
    // Nothing from a private tab reaches the app while private windows are off or unknown (D279).
    let privateSession: string | null = null;
    if (request.cookieStore.private) {
      privateSession = (await app.privateWindowsOn())
        ? ((await this.#deps.privateSession?.current()) ?? null)
        : null;
      if (privateSession === null) return { ok: false, message: MEDIA_PRIVATE_WINDOW };
    }
    await settings.ready;
    if (!app.connected || settings.live === null) {
      // Connect first, so "could not be reached" is certain: nothing has been sent yet when this fails.
      try {
        await app.fetchConfig();
      } catch (error) {
        log.warn('hub', 'Could not reach the app to send media', error);
        return { ok: false, message: MEDIA_UNREACHABLE };
      }
    }
    // Media is loaded by the page itself (a top-level navigation only for the page address).
    const context: CookieRequest = {
      initiator: request.referrer,
      top_level: request.pageUrl,
      navigation: target.kind === 'page',
    };
    const result = await app.submitChecked(
      newHandoverId(this.#deps),
      {
        url: target.url,
        page_url: request.pageUrl,
        frame_url: target.requestedBy,
        file_name: null,
        mime: target.mime,
        size: target.size,
        source: 'media_button',
        private_session: privateSession,
        cookie_request: context,
      },
      {
        url: target.url,
        referrer: refererFor(target.url, request.referrer),
        page_url: request.pageUrl,
        browser_route: {
          observed_page: request.pageUrl ?? target.requestedBy,
          private_window: request.cookieStore.private,
          private_session: privateSession,
          redirect_urls: [],
        },
        // The document that requested the media chose its address; left out when unknown.
        ...(target.requestedBy === null ? {} : { frame_url: target.requestedBy }),
        suggested_name: null,
        mime: target.mime,
        size_hint: target.size,
        source: 'media_button',
        media: { kind: target.kind, title: request.title },
      },
      // Media from another site goes with the cookies its own request carried, when it was seen: no cookie read can tell whether the browser sends its unpartitioned cookies to another site (D59). The app says whether it is from another site (D279).
      async (plan) =>
        target.sentCookies !== undefined && plan.cross_site
          ? { cookies: target.sentCookies }
          : this.#deps.cookies(target.url, request.cookieStore, context, plan),
    );
    return mediaAnswer(result);
  }
}
