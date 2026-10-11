/**
 * The popup, rendered in jsdom and wired to the real background hub and native
 * client, which talk to a fake app through a fake native port.
 */
import { cleanup, render, screen, waitFor, within } from '@testing-library/svelte';
import userEvent from '@testing-library/user-event';
import axe from 'axe-core';
import { afterEach, describe, expect, it } from 'vitest';
import App from '../src/components/App.svelte';
import { Hub } from '../src/lib/hub';
import type { BackgroundMessage, PopupRequest } from '../src/lib/messages';
import { NativeClient } from '../src/lib/native/client';
import { PopupModel } from '../src/lib/popup/model.svelte';
import { patchOf } from '../src/lib/protocol';
import { FakeApp, FakeDownloads, MemoryCache, channelPair, downloadItem, flush } from './fakes';

interface Opened {
  app: FakeApp;
  hub: Hub;
  client: NativeClient;
  model: PopupModel;
  container: HTMLElement;
}

const opened: Opened[] = [];

async function openPopup(
  options: {
    configure?: (app: FakeApp, cache: MemoryCache) => void;
    currentUrl?: string | null;
    siteAccess?: boolean;
    requestSiteAccess?: () => Promise<boolean>;
  } = {},
): Promise<Opened> {
  const app = new FakeApp();
  const cache = new MemoryCache();
  options.configure?.(app, cache);
  const client = new NativeClient({
    transport: app,
    hello: () =>
      Promise.resolve({
        client: 'extension',
        client_version: '0.1.0',
        protocol_version: 1,
        browser: null,
      }),
    backoffMs: [60_000],
  });
  const hub = new Hub({
    client,
    cache,
    downloads: new FakeDownloads(),
    cookieHeader: () => Promise.resolve(null),
    hasSiteAccess: () => Promise.resolve(true),
    userAgent: null,
  });
  await hub.start();
  await flush();
  const [popupEnd, backgroundEnd] = channelPair<PopupRequest, BackgroundMessage>();
  hub.addPopup(backgroundEnd);
  const model = new PopupModel(popupEnd);
  await flush();
  const { container } = render(App, {
    props: {
      model,
      currentUrl:
        options.currentUrl === undefined ? 'https://www.example.com/page' : options.currentUrl,
      siteAccess: options.siteAccess ?? true,
      requestSiteAccess: options.requestSiteAccess,
    },
  });
  const result = { app, hub, client, model, container };
  opened.push(result);
  return result;
}

async function axeViolations(container: HTMLElement): Promise<string[]> {
  const results = await axe.run(container, {
    // jsdom does no layout, so colour contrast is checked by hand in the report instead.
    rules: { 'color-contrast': { enabled: false } },
  });
  return results.violations.map((v) => `${v.id}: ${v.nodes.map((n) => n.html).join(' | ')}`);
}

afterEach(() => {
  cleanup();
  for (const o of opened.splice(0)) o.client.stop();
});

describe('popup', () => {
  it('shows the connection and every setting from the app, with labels and explanations', async () => {
    await openPopup({
      configure: (app) => {
        app.settings = {
          ...app.settings,
          ask_before_download: true,
          ignored_sites: ['ads.example.org'],
          capture_extensions: ['zip', 'iso'],
          min_capture_size: 5 * 1024 ** 2,
          default_queue: 2,
        };
      },
    });
    expect(screen.getByText('Connected')).toBeTruthy();
    expect(screen.getByRole('switch', { name: 'Capture downloads' })).toHaveProperty(
      'checked',
      true,
    );
    const ask = screen.getByRole('switch', { name: 'Show a window before each download' });
    expect(ask).toHaveProperty('checked', true);
    expect(ask.getAttribute('aria-describedby')).toBe('ask-before-help');
    expect(screen.getByRole('switch', { name: 'Show download button on videos' })).toHaveProperty(
      'checked',
      true,
    );
    expect(screen.getByRole('combobox', { name: 'Default queue' })).toHaveProperty('value', '2');
    expect(screen.getByRole('textbox', { name: 'File types to capture' })).toHaveProperty(
      'value',
      'zip, iso',
    );
    expect(screen.getByRole('spinbutton', { name: 'Minimum file size' })).toHaveProperty(
      'value',
      '5',
    );
    expect(screen.getByRole('combobox', { name: 'Size unit' })).toHaveProperty('value', 'MB');
    expect(screen.getByText('ads.example.org')).toBeTruthy();
    expect(screen.getByRole('button', { name: 'Ignore this site' })).toBeTruthy();
    expect(isDisabled(screen.getByRole('button', { name: 'Pause all' }))).toBe(false);
  });

  it('sends a changed setting to the app', async () => {
    const { app } = await openPopup();
    const user = userEvent.setup();
    await user.click(screen.getByRole('switch', { name: 'Show a window before each download' }));
    await waitFor(() => expect(app.settings.ask_before_download).toBe(true));
    expect(app.calls('update_extension_settings')).toEqual([
      { method: 'update_extension_settings', params: patchOf({ ask_before_download: true }) },
    ]);
    expect(
      screen.getByRole('switch', { name: 'Show a window before each download' }),
    ).toHaveProperty('checked', true);
  });

  it('updates an open popup when the setting changes in the app', async () => {
    const { app } = await openPopup();
    const capture = screen.getByRole('switch', { name: 'Capture downloads' });
    expect(capture).toHaveProperty('checked', true);
    app.changeSettings({
      capture_enabled: false,
      capture_extensions: ['mp4'],
      ignored_sites: ['new.example'],
    });
    await waitFor(() => expect(capture).toHaveProperty('checked', false));
    expect(screen.getByRole('textbox', { name: 'File types to capture' })).toHaveProperty(
      'value',
      'mp4',
    );
    expect(screen.getByText('new.example')).toBeTruthy();
    // Same element: the popup was updated in place, not reopened.
    expect(screen.getByRole('switch', { name: 'Capture downloads' })).toBe(capture);
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
    expect(isDisabled(screen.getByRole('button', { name: 'Ignore this site' }))).toBe(true);
    expect(screen.getByText('Open a web page to ignore its site.')).toBeTruthy();
  });

  it('edits file types, minimum size, default queue and the ignored sites list', async () => {
    const { app } = await openPopup();
    const user = userEvent.setup();

    const types = screen.getByRole('textbox', { name: 'File types to capture' });
    await user.type(types, 'zip, .EXE{Enter}');
    await waitFor(() => expect(app.settings.capture_extensions).toEqual(['zip', 'exe']));

    const size = screen.getByRole('spinbutton', { name: 'Minimum file size' });
    await user.clear(size);
    await user.type(size, '2');
    await user.tab();
    await waitFor(() => expect(app.settings.min_capture_size).toBe(2 * 1024 ** 2));
    await user.selectOptions(screen.getByRole('combobox', { name: 'Size unit' }), 'GB');
    await waitFor(() => expect(app.settings.min_capture_size).toBe(2 * 1024 ** 3));

    await user.selectOptions(screen.getByRole('combobox', { name: 'Default queue' }), 'Night');
    await waitFor(() => expect(app.settings.default_queue).toBe(2));

    const site = screen.getByRole('textbox', { name: 'Site to ignore' });
    await user.type(site, 'https://Tracker.example.net/x{Enter}');
    await waitFor(() => expect(app.settings.ignored_sites).toEqual(['tracker.example.net']));
    await user.click(
      await screen.findByRole('button', { name: 'Stop ignoring tracker.example.net' }),
    );
    await waitFor(() => expect(app.settings.ignored_sites).toEqual([]));
    expect(app.calls('update_extension_settings').slice(-2)).toEqual([
      {
        method: 'update_extension_settings',
        params: patchOf({ add_ignored_sites: ['tracker.example.net'] }),
      },
      {
        method: 'update_extension_settings',
        params: patchOf({ remove_ignored_sites: ['tracker.example.net'] }),
      },
    ]);
  });

  it('keeps what the person is typing in file types when a change arrives from the app', async () => {
    const { app } = await openPopup();
    const user = userEvent.setup();
    const types = screen.getByRole('textbox', { name: 'File types to capture' });
    await user.click(types);
    await user.keyboard('zip, ex');
    app.changeSettings({ capture_enabled: false });
    await waitFor(() =>
      expect(screen.getByRole('switch', { name: 'Capture downloads' })).toHaveProperty(
        'checked',
        false,
      ),
    );
    expect(types).toHaveProperty('value', 'zip, ex');
    await user.keyboard('e{Enter}');
    await waitFor(() => expect(app.settings.capture_extensions).toEqual(['zip', 'exe']));
    await user.tab();
    expect(types).toHaveProperty('value', 'zip, exe');
  });

  it('marks the queues and shows clearly when the default queue no longer exists', async () => {
    const { app } = await openPopup({
      configure: (app) => {
        app.settings = { ...app.settings, default_queue: 9 };
      },
    });
    const select = screen.getByRole('combobox', { name: 'Default queue' });
    expect(select).toHaveProperty('value', '9');
    expect(screen.getByRole('option', { name: 'Missing queue (choose another)' })).toBeTruthy();
    expect(screen.getByRole('option', { name: 'Main (default)' })).toBeTruthy();
    expect(
      screen.getByText('This queue no longer exists in Wirecatch. Choose another one.'),
    ).toBeTruthy();
    expect(select.getAttribute('aria-invalid')).toBe('true');
    await userEvent.setup().selectOptions(select, 'Main (default)');
    await waitFor(() => expect(app.settings.default_queue).toBe(1));
    await waitFor(() => expect(screen.queryByText(/no longer exists/)).toBeNull());
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

  it('explains invalid input next to the field', async () => {
    const { app } = await openPopup();
    const user = userEvent.setup();
    await user.click(screen.getByRole('button', { name: 'Add' }));
    expect(screen.getByRole('alert').textContent).toContain('Type a site such as example.com.');
    expect(
      screen.getByRole('textbox', { name: 'Site to ignore' }).getAttribute('aria-invalid'),
    ).toBe('true');
    expect(app.calls('update_extension_settings')).toHaveLength(0);
  });

  it('pauses and resumes all downloads and says so', async () => {
    const { app } = await openPopup();
    const user = userEvent.setup();
    await user.click(screen.getByRole('button', { name: 'Pause all' }));
    await waitFor(() =>
      expect(screen.getByRole('status').textContent).toBe('All downloads are paused.'),
    );
    await user.click(screen.getByRole('button', { name: 'Resume all' }));
    await waitFor(() =>
      expect(screen.getByRole('status').textContent).toBe('All downloads are resuming.'),
    );
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
    await userEvent.setup().click(await screen.findByRole('button', { name: 'Open Wirecatch' }));
    await waitFor(() => expect(screen.getByRole('status').textContent).toBe('Wirecatch is open.'));
    expect(app.starts).toBe(1);
    expect(app.calls('show_window')).toHaveLength(1);
  });

  it('rolls a change back and says why when the app refuses it', async () => {
    const { app } = await openPopup();
    app.updateError = { code: 'internal', message: 'Settings could not be saved.' };
    const user = userEvent.setup();
    const media = screen.getByRole('switch', { name: 'Show download button on videos' });
    await user.click(media);
    await waitFor(() =>
      expect(screen.getByRole('status').textContent).toContain('Settings could not be saved.'),
    );
    expect(media).toHaveProperty('checked', true);
  });

  it('shows the cached settings read-only, with the reason and the fix, when the app is unreachable', async () => {
    const { app } = await openPopup({
      configure: (app, cache) => {
        cache.value = {
          settings: { ...app.settings, capture_extensions: ['iso'] },
          queues: [{ id: 1, name: 'Main', is_default: true }],
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
    expect(screen.getByText(/Showing your last saved settings/)).toBeTruthy();
    expect(screen.getByRole('textbox', { name: 'File types to capture' })).toHaveProperty(
      'value',
      'iso',
    );
    expect(isDisabled(screen.getByRole('switch', { name: 'Capture downloads' }))).toBe(true);
    expect(isDisabled(screen.getByRole('textbox', { name: 'File types to capture' }))).toBe(true);
    expect(isDisabled(screen.getByRole('button', { name: 'Ignore this site' }))).toBe(true);
    expect(isDisabled(screen.getByRole('button', { name: 'Pause all' }))).toBe(true);

    // "Try again" once the problem is fixed: the popup becomes editable in place.
    app.hostMissing = null;
    await userEvent.setup().click(screen.getByRole('button', { name: 'Try again' }));
    await waitFor(() => expect(screen.getByText('Connected')).toBeTruthy());
    await waitFor(() =>
      expect(isDisabled(screen.getByRole('switch', { name: 'Capture downloads' }))).toBe(false),
    );
    expect(screen.queryByText(/Showing your last saved settings/)).toBeNull();
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
    expect(
      screen.getByText('Details: Access to the specified native messaging host is forbidden.'),
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
    const message = await screen.findByText('big.zip stayed in the browser: Disk full.');
    expect(message).toBeTruthy();
    await userEvent.setup().click(screen.getByRole('button', { name: 'Dismiss' }));
    await waitFor(() => expect(screen.queryByText(/stayed in the browser/)).toBeNull());
  });
});

describe('popup accessibility', () => {
  it('has no axe violations when connected', async () => {
    const { container } = await openPopup({
      configure: (app) => {
        app.settings = { ...app.settings, ignored_sites: ['a.example', 'b.example'] };
      },
    });
    expect(await axeViolations(container)).toEqual([]);
  });

  it('has no axe violations with the access panel and a missing default queue', async () => {
    const { container } = await openPopup({
      siteAccess: false,
      configure: (app) => {
        app.settings = { ...app.settings, default_queue: 9 };
      },
    });
    expect(await axeViolations(container)).toEqual([]);
  });

  it('has no axe violations when the app is unreachable', async () => {
    const { container } = await openPopup({
      configure: (app, cache) => {
        cache.value = {
          settings: { ...app.settings, ignored_sites: ['a.example'] },
          queues: [],
          savedAt: 1,
          appClosed: false,
        };
        app.hostMissing = 'Specified native messaging host not found.';
      },
    });
    expect(await axeViolations(container)).toEqual([]);
  });

  it('reaches every control with Tab, in reading order', async () => {
    await openPopup({
      configure: (app) => {
        app.settings = { ...app.settings, ignored_sites: ['a.example'] };
      },
    });
    const user = userEvent.setup();
    const order: string[] = [];
    for (let i = 0; i < 13; i += 1) {
      await user.tab();
      const el = document.activeElement as HTMLElement;
      order.push(`${el.getAttribute('role') ?? el.tagName.toLowerCase()}:${accessibleName(el)}`);
    }
    expect(order).toEqual([
      'switch:Capture downloads',
      'button:Ignore this site',
      'switch:Show a window before each download',
      'switch:Show download button on videos',
      'select:Default queue',
      'input:File types to capture',
      'input:Minimum file size',
      'select:Size unit',
      'button:Stop ignoring a.example',
      'input:Site to ignore',
      'button:Add',
      'button:Pause all',
      'button:Resume all',
    ]);
    // Nothing else takes focus: the next Tab leaves the popup content.
    await user.tab();
    expect(document.activeElement).toBe(document.body);
  });

  it('works with the keyboard alone: Space toggles a switch, Enter adds a site', async () => {
    const { app } = await openPopup();
    const user = userEvent.setup();
    await user.tab();
    expect(document.activeElement).toBe(screen.getByRole('switch', { name: 'Capture downloads' }));
    await user.keyboard(' ');
    await waitFor(() => expect(app.settings.capture_enabled).toBe(false));
    screen.getByRole('textbox', { name: 'Site to ignore' }).focus();
    await user.keyboard('keyboard.example{Enter}');
    await waitFor(() => expect(app.settings.ignored_sites).toEqual(['keyboard.example']));
  });

  it('announces results in a live region and marks the problem panel with a heading', async () => {
    await openPopup();
    const status = screen.getByRole('status');
    expect(status.getAttribute('role')).toBe('status');
    const footer = screen.getByRole('contentinfo');
    expect(within(footer).getByRole('status')).toBe(status);
  });
});

/** True for a control that cannot be used, including one inside a disabled fieldset. */
function isDisabled(el: HTMLElement): boolean {
  return el.matches(':disabled');
}

function accessibleName(el: HTMLElement): string {
  const labelledBy = el.getAttribute('aria-labelledby');
  if (labelledBy !== null) return document.getElementById(labelledBy)?.textContent.trim() ?? '';
  const aria = el.getAttribute('aria-label');
  if (aria !== null) return aria;
  if (el.id !== '') {
    const label = document.querySelector(`label[for="${el.id}"]`);
    if (label !== null) return label.textContent.trim();
  }
  return el.textContent.trim();
}
