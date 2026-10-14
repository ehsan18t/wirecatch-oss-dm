/**
 * The toolbar popup, rendered in jsdom and wired to the real background hub and native client,
 * which talk to a fake app through a fake native port.
 */
import { queueId } from '@wirecatch/protocol';
import { PRODUCT_NAME } from '@wirecatch/product';
import { cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/svelte';
import userEvent from '@testing-library/user-event';
import { afterEach, describe, expect, it, vi } from 'vitest';
import type { BackgroundMessage, PopupRequest } from '../src/lib/messages';
import { PopupModel } from '../src/lib/popup/model.svelte';
import { patchOf } from '../src/lib/protocol';
import Popup from '../src/ui/popup/Popup.svelte';
import { channelPair, downloadItem, flush, type FakeApp, type MemoryCache } from './fakes';
import {
  axeViolations,
  describeFocus,
  isUnavailable,
  unwireAll,
  wirePage,
  type Wired,
} from './pages';

interface Opened extends Wired {
  container: HTMLElement;
  openSettings: ReturnType<typeof vi.fn>;
}

async function openPopup(
  options: {
    configure?: (app: FakeApp, cache: MemoryCache) => void;
    currentUrl?: string | null;
    siteAccess?: boolean;
    requestSiteAccess?: () => Promise<boolean>;
  } = {},
): Promise<Opened> {
  const wired = await wirePage(options.configure);
  const openSettings = vi.fn();
  const { container } = render(Popup, {
    props: {
      model: wired.model,
      currentUrl:
        options.currentUrl === undefined ? 'https://www.example.com/page' : options.currentUrl,
      siteAccess: options.siteAccess ?? true,
      requestSiteAccess: options.requestSiteAccess,
      openSettings,
    },
  });
  return { ...wired, container, openSettings };
}

const captureSwitch = (): HTMLElement => screen.getByRole('switch', { name: 'Capture downloads' });

afterEach(() => {
  cleanup();
  unwireAll();
});

describe('popup', () => {
  it('shows the connection, capture and this site at a glance', async () => {
    await openPopup();
    expect(screen.getByRole('heading', { level: 1, name: PRODUCT_NAME })).toBeTruthy();
    expect(screen.getByText('Connected')).toBeTruthy();
    const capture = screen.getByRole('switch', {
      name: 'Capture downloads',
      description: 'Send downloads from this browser to Wirecatch.',
    });
    expect(capture.getAttribute('aria-checked')).toBe('true');
    expect(screen.getByText('example.com')).toBeTruthy();
    expect(
      screen.getByRole('button', {
        name: 'Ignore this site',
        description: 'Downloads from example.com go to Wirecatch.',
      }),
    ).toBeTruthy();
    expect(isUnavailable(screen.getByRole('button', { name: 'Pause all' }))).toBe(false);
    // D4: whether a window asks first stays in the popup; every other setting is on the options page (D5).
    expect(screen.getAllByRole('switch').map((s) => s.id)).toEqual([
      'capture-enabled',
      'ask-before',
    ]);
    expect(screen.queryByRole('combobox')).toBeNull();
    expect(screen.queryByRole('textbox')).toBeNull();
  });

  it('turns the window before each download on and off in the app', async () => {
    const { app } = await openPopup();
    const ask = screen.getByRole('switch', {
      name: 'Show a window before each download',
      description: 'Choose where each file goes before it starts.',
    });
    expect(ask.getAttribute('aria-checked')).toBe('false');
    await userEvent.setup().click(ask);
    await waitFor(() => expect(app.settings.ask_before_download).toBe(true));
    expect(app.calls('update_extension_settings')).toEqual([
      { method: 'update_extension_settings', params: patchOf({ ask_before_download: true }) },
    ]);
    expect(ask.getAttribute('aria-checked')).toBe('true');
  });

  it('is titled with the product name', async () => {
    document.title = '';
    await openPopup();
    expect(document.title).toBe(PRODUCT_NAME);
  });

  it('turns capture off and on in the app', async () => {
    const { app } = await openPopup();
    const user = userEvent.setup();
    await user.click(captureSwitch());
    await waitFor(() => expect(app.settings.capture_enabled).toBe(false));
    expect(captureSwitch().getAttribute('aria-checked')).toBe('false');
    expect(app.calls('update_extension_settings')).toEqual([
      { method: 'update_extension_settings', params: patchOf({ capture_enabled: false }) },
    ]);
    await user.click(captureSwitch());
    await waitFor(() => expect(app.settings.capture_enabled).toBe(true));
  });

  it('shows capture off, with this site still at hand', async () => {
    await openPopup({
      configure: (app) => {
        app.settings = { ...app.settings, capture_enabled: false };
      },
    });
    expect(captureSwitch().getAttribute('aria-checked')).toBe('false');
    expect(isUnavailable(screen.getByRole('button', { name: 'Ignore this site' }))).toBe(false);
  });

  it('updates in place when the setting changes in the app', async () => {
    const { app } = await openPopup();
    const capture = captureSwitch();
    app.changeSettings({ capture_enabled: false, ignored_sites: ['example.com'] });
    await waitFor(() => expect(capture.getAttribute('aria-checked')).toBe('false'));
    expect(screen.getByRole('button', { name: 'Stop ignoring' })).toBeTruthy();
    // Same element: the popup was updated in place, not reopened.
    expect(captureSwitch()).toBe(capture);
  });

  it('ignores the current site in one click, and can undo it', async () => {
    const { app } = await openPopup({ currentUrl: 'https://www.example.com/downloads' });
    const user = userEvent.setup();
    await user.click(screen.getByRole('button', { name: 'Ignore this site' }));
    await waitFor(() => expect(app.settings.ignored_sites).toEqual(['example.com']));
    const undo = await screen.findByRole('button', { name: 'Stop ignoring' });
    expect(screen.getByText('Downloads from example.com stay in the browser.')).toBeTruthy();
    await user.click(undo);
    await waitFor(() => expect(app.settings.ignored_sites).toEqual([]));
    // One site added and removed, never the whole list.
    expect(app.calls('update_extension_settings')).toEqual([
      {
        method: 'update_extension_settings',
        params: patchOf({ add_ignored_sites: ['example.com'] }),
      },
      {
        method: 'update_extension_settings',
        params: patchOf({ remove_ignored_sites: ['example.com'] }),
      },
    ]);
  });

  it('does not overwrite a site another browser added at the same time', async () => {
    const { app } = await openPopup({ currentUrl: 'https://www.example.com/downloads' });
    // Another browser adds a site; this popup has not heard about it yet.
    app.settings = { ...app.settings, ignored_sites: ['other.example'] };
    await userEvent.setup().click(screen.getByRole('button', { name: 'Ignore this site' }));
    await waitFor(() =>
      expect(app.settings.ignored_sites).toEqual(['other.example', 'example.com']),
    );
  });

  it('cannot ignore a browser page', async () => {
    await openPopup({ currentUrl: 'chrome://newtab/' });
    expect(isUnavailable(screen.getByRole('button', { name: 'Ignore this site' }))).toBe(true);
    expect(screen.getByText('Open a web page to ignore its site.')).toBeTruthy();
  });

  it('opens the options page from the header and from the media hint', async () => {
    const { openSettings } = await openPopup();
    const user = userEvent.setup();
    await user.click(screen.getByRole('button', { name: 'Settings' }));
    expect(openSettings).toHaveBeenCalledTimes(1);
    await user.click(screen.getByRole('button', { name: 'Turn it off in settings' }));
    expect(openSettings).toHaveBeenCalledTimes(2);
  });

  it('offers to allow website access again when it was revoked', async () => {
    let asked = 0;
    await openPopup({
      siteAccess: false,
      requestSiteAccess: () => {
        asked += 1;
        return Promise.resolve(true);
      },
    });
    expect(
      screen.getByRole('heading', { name: 'Wirecatch cannot read website cookies.' }),
    ).toBeTruthy();
    await userEvent.setup().click(screen.getByRole('button', { name: 'Allow access' }));
    expect(asked).toBe(1);
    await waitFor(() => expect(screen.queryByRole('button', { name: 'Allow access' })).toBeNull());
  });

  it('pauses and resumes all downloads and says so', async () => {
    const { app } = await openPopup();
    const user = userEvent.setup();
    const status = within(screen.getByRole('contentinfo')).getByRole('status');
    await user.click(screen.getByRole('button', { name: 'Pause all' }));
    await waitFor(() => expect(status.textContent.trim()).toBe('All downloads are paused.'));
    await user.click(screen.getByRole('button', { name: 'Resume all' }));
    await waitFor(() => expect(status.textContent.trim()).toBe('All downloads are resuming.'));
    expect(app.calls('pause_all')).toHaveLength(1);
    expect(app.calls('resume_all')).toHaveLength(1);
  });

  // The button used to only reconnect, and the bridge starts the app in the tray, so nothing opened.
  it('opens a closed Wirecatch and shows its window', async () => {
    const { app } = await openPopup({
      configure: (fake) => {
        fake.running = false;
      },
    });
    expect(screen.getByText('Closed')).toBeTruthy();
    expect(screen.getByRole('heading', { name: 'Wirecatch is closed.' })).toBeTruthy();
    await userEvent.setup().click(await screen.findByRole('button', { name: 'Open Wirecatch' }));
    const status = within(screen.getByRole('contentinfo')).getByRole('status');
    await waitFor(() => expect(status.textContent.trim()).toBe('Wirecatch is open.'));
    expect(app.starts).toBe(1);
    expect(app.calls('show_window')).toHaveLength(1);
  });

  it('rolls a change back and says why when the app refuses it', async () => {
    const { app } = await openPopup();
    app.updateError = { code: 'internal', message: 'Settings could not be saved.' };
    await userEvent.setup().click(captureSwitch());
    const status = within(screen.getByRole('contentinfo')).getByRole('status');
    await waitFor(() => expect(status.textContent).toContain('Settings could not be saved.'));
    expect(status.className).toContain('text-danger');
    expect(captureSwitch().getAttribute('aria-checked')).toBe('true');
  });

  it('shows the cached settings read-only, with the reason and the fix, when the app is unreachable', async () => {
    const { app } = await openPopup({
      configure: (app, cache) => {
        cache.value = {
          settings: { ...app.settings, capture_enabled: false },
          queues: [{ id: queueId(1), name: 'Main', is_default: true }],
          savedAt: Date.UTC(2026, 9, 1),
          appClosed: false,
        };
        app.hostMissing = 'Specified native messaging host not found.';
      },
    });
    expect(screen.getByText('Not connected')).toBeTruthy();
    expect(
      screen.getByRole('heading', {
        name: 'Wirecatch is not installed, or not set up for this browser.',
      }),
    ).toBeTruthy();
    expect(
      screen.getByText('Install Wirecatch, or open it once so it can connect to this browser.'),
    ).toBeTruthy();
    expect(screen.getByText('Details: Specified native messaging host not found.')).toBeTruthy();
    // The saved copy is said inside the connection notice, not in a notice of its own.
    expect(
      screen.getByRole('heading', {
        name: 'Wirecatch is not installed, or not set up for this browser.',
      }).parentElement?.textContent,
    ).toMatch(/Showing your last saved settings/);
    expect(screen.getAllByText(/Showing your last saved settings/)).toHaveLength(1);
    expect(captureSwitch().getAttribute('aria-checked')).toBe('false');
    expect(isUnavailable(captureSwitch())).toBe(true);
    expect(screen.getByRole('switch', { name: 'Show a window before each download' })).toSatisfy(
      isUnavailable,
    );
    // Disabled with the reason, which a screen reader hears and the pointer sees; the site's own
    // description stays, ahead of the reason.
    for (const [name, description] of [
      [
        'Ignore this site',
        'Downloads from example.com go to Wirecatch. Wirecatch is not connected.',
      ],
      ['Pause all', 'Wirecatch is not connected.'],
      ['Resume all', 'Wirecatch is not connected.'],
    ] as const) {
      expect(screen.getByRole('button', { name, description })).toSatisfy(isUnavailable);
    }

    // "Try again" once the problem is fixed: the popup becomes editable in place.
    app.hostMissing = null;
    await userEvent.setup().click(screen.getByRole('button', { name: 'Try again' }));
    await waitFor(() => expect(screen.getByText('Connected')).toBeTruthy());
    await waitFor(() => expect(isUnavailable(captureSwitch())).toBe(false));
    expect(screen.queryByText(/Showing your last saved settings/)).toBeNull();
    expect(isUnavailable(screen.getByRole('button', { name: 'Pause all' }))).toBe(false);
  });

  it('says plainly when Wirecatch does not trust this copy of the extension, without asking for a reinstall', async () => {
    await openPopup({
      configure: (app) => {
        app.hostMissing = 'Access to the specified native messaging host is forbidden.';
      },
    });
    expect(
      screen.getByRole('heading', {
        name: 'This copy of the extension is not the one Wirecatch trusts.',
      }),
    ).toBeTruthy();
    expect(
      screen.getByText(
        'Install the extension from the Wirecatch website or store, or update Wirecatch.',
      ),
    ).toBeTruthy();
    expect(document.body.textContent).not.toMatch(/reinstall|not installed/i);
  });

  it('does not show made-up settings before it has ever been connected', async () => {
    await openPopup({
      configure: (app) => {
        app.hostMissing = 'Specified native messaging host not found.';
      },
    });
    expect(screen.getByText('Your settings show here once Wirecatch is connected.')).toBeTruthy();
    expect(screen.queryByRole('switch')).toBeNull();
  });

  it('does not show made-up settings while connecting for the first time', async () => {
    await openPopup({
      configure: (app) => {
        app.answers = false; // hello never answered: still connecting
      },
    });
    expect(screen.getByText('Connecting')).toBeTruthy();
    expect(
      screen.getByText('Connecting to Wirecatch. Your settings show here once it is connected.'),
    ).toBeTruthy();
    expect(screen.queryByRole('switch')).toBeNull();
  });

  it('explains when the bridge cannot start the app', async () => {
    await openPopup({
      configure: (app) => {
        app.helloError = { code: 'app_unavailable', message: 'Could not start Wirecatch.' };
      },
    });
    expect(
      screen.getByRole('heading', { name: 'The Wirecatch app could not be started.' }),
    ).toBeTruthy();
    expect(screen.getByText('Open the Wirecatch app yourself, then click Try again.')).toBeTruthy();
  });

  it('shows background messages and lets them be dismissed', async () => {
    const { app, hub } = await openPopup();
    app.addBehavior = { error: { code: 'internal', message: 'Disk full.' } };
    await hub.onDownload(downloadItem());
    expect(await screen.findByText('big.zip stayed in the browser: Disk full.')).toBeTruthy();
    await userEvent.setup().click(
      screen.getByRole('button', {
        name: 'Dismiss',
        description: 'big.zip stayed in the browser: Disk full.',
      }),
    );
    await waitFor(() => expect(screen.queryByText(/stayed in the browser/)).toBeNull());
  });

  it('lists the messages and moves focus to the next one after a dismiss', async () => {
    const { hub } = await openPopup();
    hub.notify('warning', 'First message.');
    hub.notify('error', 'Second message.');
    const list = await screen.findByRole('list', { name: 'Messages' });
    await waitFor(() => expect(within(list).getAllByRole('listitem')).toHaveLength(2));
    const dismiss = (message: string): HTMLElement =>
      screen.getByRole('button', { name: 'Dismiss', description: message });
    const user = userEvent.setup();

    await user.click(dismiss('Second message.'));
    await waitFor(() => expect(document.activeElement).toBe(dismiss('First message.')));

    await user.click(dismiss('First message.'));
    await waitFor(() => expect(screen.queryByRole('list', { name: 'Messages' })).toBeNull());
    expect(document.activeElement).toBe(screen.getByRole('main'));
  });

  it('leaves focus where the person moved it before the dismissed message went', async () => {
    const { hub } = await openPopup();
    hub.notify('warning', 'First message.');
    hub.notify('error', 'Second message.');
    const button = await screen.findByRole('button', {
      name: 'Dismiss',
      description: 'Second message.',
    });
    const elsewhere = screen.getByRole('main');
    button.focus();
    // Clicked without awaiting, so focus moves before the list without the notice arrives.
    void fireEvent.click(button);
    elsewhere.focus();
    await waitFor(() => expect(screen.queryByText('Second message.')).toBeNull());
    expect(document.activeElement).toBe(elsewhere);
  });
});

describe('popup accessibility', () => {
  it('has no axe violations when connected, with messages', async () => {
    const { container, hub } = await openPopup();
    hub.notify('warning', 'A message.');
    await screen.findByText('A message.');
    expect(await axeViolations(container)).toEqual([]);
  });

  it('has no axe violations with the access notice and the app closed', async () => {
    const { container } = await openPopup({
      siteAccess: false,
      configure: (app) => {
        app.running = false;
      },
    });
    await screen.findByRole('button', { name: 'Open Wirecatch' });
    expect(await axeViolations(container)).toEqual([]);
  });

  it('has no axe violations when the app is unreachable', async () => {
    const { container } = await openPopup({
      configure: (app, cache) => {
        cache.value = { settings: app.settings, queues: [], savedAt: 1, appClosed: false };
        app.hostMissing = 'Specified native messaging host not found.';
      },
    });
    expect(await axeViolations(container)).toEqual([]);
  });

  it('reaches every control with Tab, in reading order', async () => {
    await openPopup();
    const user = userEvent.setup();
    const order: string[] = [];
    for (let i = 0; i < 7; i += 1) {
      await user.tab();
      order.push(describeFocus(document.activeElement));
    }
    expect(order).toEqual([
      'button:Settings',
      'switch:Capture downloads',
      'switch:Show a window before each download',
      'button:Ignore this site',
      'button:Turn it off in settings',
      'button:Pause all',
      'button:Resume all',
    ]);
    // Nothing else takes focus: the next Tab leaves the popup content.
    await user.tab();
    expect(document.activeElement).toBe(document.body);
  });

  it('works with the keyboard alone: Space toggles capture, Enter ignores the site', async () => {
    const { app } = await openPopup();
    const user = userEvent.setup();
    await user.tab();
    await user.tab();
    expect(document.activeElement).toBe(captureSwitch());
    await user.keyboard(' ');
    await waitFor(() => expect(app.settings.capture_enabled).toBe(false));
    await user.tab();
    await user.tab();
    await user.keyboard('{Enter}');
    await waitFor(() => expect(app.settings.ignored_sites).toEqual(['example.com']));
  });

  it('announces results in the footer live region', async () => {
    await openPopup();
    const footer = screen.getByRole('contentinfo');
    expect(within(footer).getByRole('status').textContent.trim()).toBe('');
  });
});

describe('popup model', () => {
  it('ignores a message from the background that does not have the shape its type claims', async () => {
    const [popupEnd, backgroundEnd] = channelPair<PopupRequest, BackgroundMessage>();
    const model = new PopupModel(popupEnd);
    backgroundEnd.postMessage({ type: 'snapshot', snapshot: { status: 'fine' } } as never);
    backgroundEnd.postMessage({ type: 'media', view: { tabId: 1 } } as never);
    await flush();
    expect(model.snapshot).toBeNull();
    expect(model.media).toBeNull();
  });

  it('says when what a page shows first is in: the snapshot, and the media list when asked', async () => {
    const [pageEnd, backgroundEnd] = channelPair<PopupRequest, BackgroundMessage>();
    const model = new PopupModel(pageEnd);
    const order: string[] = [];
    void model.loaded(false).then(() => order.push('snapshot'));
    void model.loaded(true).then(() => order.push('media'));
    await flush();
    expect(order).toEqual([]);
    backgroundEnd.postMessage({
      type: 'snapshot',
      snapshot: {
        status: { state: 'connecting' },
        settings: null,
        settingsFromCache: false,
        cachedAt: null,
        queues: [],
        notices: [],
      },
    });
    await flush();
    expect(order).toEqual(['snapshot']);
    backgroundEnd.postMessage({ type: 'media', view: { tabId: 1, page: 'web', entries: [] } });
    await flush();
    expect(order).toEqual(['snapshot', 'media']);
    // Asked once everything is in, it answers at once.
    await expect(model.loaded(true)).resolves.toBeUndefined();
  });
});
