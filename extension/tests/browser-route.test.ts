import { describe, expect, it } from 'vitest';
import { BrowserRoutes, SourceTabs, type SourceTab } from '../src/lib/capture/browser-route';

const PAGE = 'https://project.example/releases/v1';
const INITIAL = 'https://project.example/releases/v1/report.pdf/download';
const MIRROR = 'https://mirror.example/choose/report.pdf';
const FINAL = 'https://files.example/report.pdf?fresh';
const PROJECT_ORIGIN = 'https://project.example';

function request(requestId: string, url: string) {
  return {
    requestId,
    tabId: 1,
    frameId: 0,
    url,
    method: 'GET',
    type: 'main_frame',
    initiator: PROJECT_ORIGIN,
  };
}

describe('the source of a browser download', () => {
  it('keeps the exact browser page when a site reports its home page as referrer', async () => {
    const home = 'https://files.example/home';
    let current = PAGE;
    const routes = new BrowserRoutes(() => Promise.resolve({ url: current, incognito: false }));
    routes.seen(request('file', INITIAL));
    current = home;
    routes.redirect({ ...request('file', INITIAL), redirectUrl: FINAL });
    routes.seen(request('file', FINAL));

    await expect(
      routes.forDownload({
        id: 1,
        url: INITIAL,
        finalUrl: FINAL,
        referrer: home,
      }),
    ).resolves.toMatchObject({ pageUrl: PAGE });
  });

  it('recovers the full page and redirect chain behind an origin-only referrer', async () => {
    const routes = new BrowserRoutes(() => Promise.resolve({ url: PAGE, incognito: false }));
    routes.redirect({ ...request('file', INITIAL), redirectUrl: MIRROR });
    routes.redirect({ ...request('file', MIRROR), redirectUrl: FINAL });
    routes.seen(request('file', FINAL));

    await expect(
      routes.forDownload({ id: 1, url: INITIAL, finalUrl: FINAL, referrer: PROJECT_ORIGIN }),
    ).resolves.toEqual({ pageUrl: PAGE, redirectUrls: [INITIAL, MIRROR] });
    await expect(routes.forDownload({ id: 1, url: FINAL, referrer: PAGE })).resolves.toEqual({
      pageUrl: PAGE,
      redirectUrls: [INITIAL, MIRROR],
    });
    await expect(routes.forDownload({ id: 1, url: INITIAL, referrer: PAGE })).resolves.toEqual({
      pageUrl: PAGE,
      redirectUrls: [INITIAL, MIRROR, FINAL],
    });
  });

  it('can return to the site root when it was the actual browser page', async () => {
    const home = `${PROJECT_ORIGIN}/`;
    const routes = new BrowserRoutes(() => Promise.resolve({ url: home, incognito: false }));
    routes.seen(request('file', INITIAL));
    routes.seen(request('file', INITIAL));
    await expect(routes.forDownload({ id: 1, url: INITIAL })).resolves.toMatchObject({
      pageUrl: home,
    });
  });

  it('leaves the source unknown when more than one request could have started it', async () => {
    const routes = new BrowserRoutes(() => Promise.resolve({ url: PAGE, incognito: false }));
    routes.redirect({ ...request('one', INITIAL), redirectUrl: FINAL });
    routes.redirect({ ...request('two', INITIAL), redirectUrl: FINAL });

    await expect(
      routes.forDownload({ id: 1, url: INITIAL, finalUrl: FINAL, referrer: PROJECT_ORIGIN }),
    ).resolves.toBeNull();
  });

  it('keeps the navigation that led to a countdown page separately from the file request', async () => {
    const entry = 'https://project.example/get/report.pdf';
    const countdown = 'https://project.example/wait/report.pdf?session=old';
    const routes = new BrowserRoutes(() => Promise.resolve({ url: countdown, incognito: false }));
    routes.redirect({ ...request('page', entry), redirectUrl: countdown });
    routes.seen(request('page', countdown));
    routes.redirect({ ...request('file', INITIAL), redirectUrl: FINAL });
    routes.seen(request('file', FINAL));

    await expect(
      routes.forDownload({ id: 1, url: INITIAL, finalUrl: FINAL, referrer: countdown }),
    ).resolves.toEqual({
      pageUrl: countdown,
      pageRedirectUrls: [entry],
      redirectUrls: [INITIAL],
    });
  });
});

describe('the tab a request began in', () => {
  function counted(tab: () => SourceTab | null) {
    const reads: number[] = [];
    const tabs = new SourceTabs((id) => {
      reads.push(id);
      return Promise.resolve(tab());
    });
    return { reads, tabs };
  }

  it('is read once for the requests of one page', async () => {
    const { reads, tabs } = counted(() => ({ url: PAGE, incognito: false }));
    await expect(tabs.of(1)).resolves.toMatchObject({ url: PAGE });
    await expect(tabs.of(1)).resolves.toMatchObject({ url: PAGE });
    await tabs.of(2);
    expect(reads).toEqual([1, 2]);
  });

  it('is read again after the tab navigates', async () => {
    let current = PAGE;
    const { reads, tabs } = counted(() => ({ url: current, incognito: false }));
    await tabs.of(1);
    current = FINAL;
    tabs.forget(1);
    await expect(tabs.of(1)).resolves.toMatchObject({ url: FINAL });
    expect(reads).toEqual([1, 1]);
  });

  it("is read each time while the page is the opener's or unknown", async () => {
    const lent = counted(() => ({ url: PAGE, incognito: false, lent: true }));
    await lent.tabs.of(1);
    await lent.tabs.of(1);
    expect(lent.reads).toEqual([1, 1]);
    const unknown = counted(() => null);
    await unknown.tabs.of(1);
    await unknown.tabs.of(1);
    expect(unknown.reads).toEqual([1, 1]);
  });
});
