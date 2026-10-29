/**
 * The toolbar popup, rendered in jsdom and wired to the real background hub and native client,
 * which talk to a fake app through a fake native port.
 */
import { queueId } from '@wirecatch/protocol';
import { PRODUCT_NAME } from '@wirecatch/product';
import { FIRST_TOTAL_SPEED_LIMIT } from '@wirecatch/units';
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
  close: ReturnType<typeof vi.fn>;
  openShortcuts: ReturnType<typeof vi.fn>;
}

async function openPopup(
  options: {
    configure?: (app: FakeApp, cache: MemoryCache) => void;
    currentUrl?: string | null;
    siteAccess?: boolean;
    requestSiteAccess?: () => Promise<boolean>;
    mediaShortcut?: string | null;
  } = {},
): Promise<Opened> {
  const wired = await wirePage(options.configure);
  const close = vi.fn();
  const openShortcuts = vi.fn();
  const { container } = render(Popup, {
    props: {
      model: wired.model,
      currentUrl:
        options.currentUrl === undefined ? 'https://www.example.com/page' : options.currentUrl,
      siteAccess: options.siteAccess ?? true,
      requestSiteAccess: options.requestSiteAccess,
      mediaShortcut: options.mediaShortcut ?? null,
      openShortcuts,
      close,
    },
  });
  return { ...wired, container, close, openShortcuts };
}

/** Unfolds one of the popup's sections, by the label it starts with. */
async function unfold(label: 'Downloads' | 'Video and audio'): Promise<void> {
  const button = screen.getByRole('button', { name: new RegExp(`^${label}`) });
  if (button.getAttribute('aria-expanded') === 'false') await userEvent.setup().click(button);
}

const captureSwitch = (): HTMLElement => screen.getByRole('switch', { name: 'Capture downloads' });
const siteSwitch = (): HTMLElement => screen.getByRole('switch', { name: /^Ignore / });

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
    const site = siteSwitch();
    expect(site.getAttribute('aria-checked')).toBe('false');
    expect(
      screen.getByRole('switch', {
        name: 'Ignore example.com',
        description: 'Downloads from this site stay in the browser.',
      }),
    ).toBe(site);
    expect(isUnavailable(screen.getByRole('button', { name: 'Pause all' }))).toBe(false);
    // D4: whether a window asks first, for files and for videos, shows at once; the rest is folded (D61).
    expect(screen.getAllByRole('switch').map((s) => s.id)).toEqual([
      'capture-enabled',
      'ask-before',
      'ask-each-video',
      'ignore-site',
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
    expect(isUnavailable(siteSwitch())).toBe(false);
  });

  it('updates in place when the setting changes in the app', async () => {
    const { app } = await openPopup();
    const capture = captureSwitch();
    app.changeSettings({ capture_enabled: false, ignored_sites: ['example.com'] });
    await waitFor(() => expect(capture.getAttribute('aria-checked')).toBe('false'));
    expect(siteSwitch().getAttribute('aria-checked')).toBe('true');
    // Same element: the popup was updated in place, not reopened.
    expect(captureSwitch()).toBe(capture);
  });

  it('ignores the current site in one click, and can undo it', async () => {
    const { app } = await openPopup({ currentUrl: 'https://www.example.com/downloads' });
    const user = userEvent.setup();
    await user.click(siteSwitch());
    await waitFor(() => expect(app.settings.ignored_sites).toEqual(['example.com']));
    await waitFor(() => expect(siteSwitch().getAttribute('aria-checked')).toBe('true'));
    await user.click(siteSwitch());
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
    await userEvent.setup().click(siteSwitch());
    await waitFor(() =>
      expect(app.settings.ignored_sites).toEqual(['other.example', 'example.com']),
    );
  });

  it('cannot ignore a browser page', async () => {
    await openPopup({ currentUrl: 'chrome://newtab/' });
    const site = screen.getByRole('switch', { name: 'Ignore this site' });
    expect(isUnavailable(site)).toBe(true);
    expect(site.getAttribute('aria-checked')).toBe('false');
  });

  it('names the entry that ignores a page in its label, and turning it off removes that entry', async () => {
    const { app } = await openPopup({
      currentUrl: 'https://video.example.com/watch',
      configure: (fake) => {
        fake.settings = { ...fake.settings, ignored_sites: ['example.com'] };
      },
    });
    const site = screen.getByRole('switch', { name: 'Ignore example.com' });
    expect(site.getAttribute('aria-checked')).toBe('true');
    await userEvent.setup().click(site);
    await waitFor(() => expect(app.settings.ignored_sites).toEqual([]));
  });

  it("opens the app's browser settings from the header, and closes once they open (D61)", async () => {
    const { app, close } = await openPopup();
    await userEvent.setup().click(screen.getByRole('button', { name: 'Open Wirecatch settings' }));
    await waitFor(() => expect(close).toHaveBeenCalledTimes(1));
    expect(app.calls('show_browser_settings')).toHaveLength(1);
  });

  it('opens the window of an app from before the browser settings could be shown', async () => {
    const { app, close } = await openPopup({
      configure: (fake) => fake.unsupported.add('show_browser_settings'),
    });
    await userEvent.setup().click(screen.getByRole('button', { name: 'Open Wirecatch settings' }));
    await waitFor(() => expect(close).toHaveBeenCalledTimes(1));
    expect(app.calls('show_window')).toHaveLength(1);
  });

  it("shows the keyboard shortcut under Video and audio, and opens the browser's shortcuts page to set it", async () => {
    const { openShortcuts } = await openPopup();
    await unfold('Video and audio');
    expect(screen.getByText('Not set')).toBeTruthy();
    await userEvent
      .setup()
      .click(screen.getByRole('button', { name: 'Set the keyboard shortcut' }));
    expect(openShortcuts).toHaveBeenCalledTimes(1);
    cleanup();
    await openPopup({ mediaShortcut: 'Alt+Shift+D' });
    await unfold('Video and audio');
    expect([...document.querySelectorAll('kbd kbd')].map((key) => key.textContent)).toEqual([
      'Alt',
      'Shift',
      'D',
    ]);
    expect(screen.getByRole('button', { name: 'Change the keyboard shortcut' })).toBeTruthy();
  });

  it('asks about each video, or stops asking, in the app (D61)', async () => {
    const { app } = await openPopup();
    const ask = screen.getByRole('switch', { name: 'Ask about each video' });
    expect(ask.getAttribute('aria-checked')).toBe('true');
    await userEvent.setup().click(ask);
    await waitFor(() => expect(app.controls.media.ask_each_time).toBe(false));
    expect(app.calls('update_browser_controls')).toEqual([
      { method: 'update_browser_controls', params: { media: { ask_each_time: false } } },
    ]);
    expect(ask.getAttribute('aria-checked')).toBe('false');
  });

  it('shows a change made in the app at once, without reopening (D61)', async () => {
    const { app } = await openPopup();
    await unfold('Downloads');
    app.changeControls({ media: { ask_each_time: false }, max_concurrent_downloads: 5 });
    app.changeSettings({ show_media_button: false });
    await waitFor(() =>
      expect(
        screen.getByRole('switch', { name: 'Ask about each video' }).getAttribute('aria-checked'),
      ).toBe('false'),
    );
    expect(screen.getByRole('spinbutton', { name: 'Downloads at the same time' })).toHaveProperty(
      'value',
      '5',
    );
    await unfold('Video and audio');
    expect(
      screen
        .getByRole('switch', { name: 'Show a download button on videos' })
        .getAttribute('aria-checked'),
    ).toBe('false');
  });

  it('changes the queue, the speed limit and downloads at once in the app', async () => {
    const { app } = await openPopup();
    const user = userEvent.setup();
    await unfold('Downloads');
    await user.click(screen.getByRole('combobox', { name: 'Queue for browser downloads' }));
    await user.click(screen.getByRole('option', { name: 'Night' }));
    await waitFor(() => expect(app.settings.default_queue).toBe(queueId(2)));

    await user.click(screen.getByRole('switch', { name: 'Global speed limit' }));
    await waitFor(() => expect(app.controls.global_speed_limit).toBe(FIRST_TOTAL_SPEED_LIMIT));
    expect(screen.getByRole('spinbutton', { name: 'Global speed limit' })).toHaveProperty(
      'value',
      '5',
    );

    const concurrent = screen.getByRole('spinbutton', { name: 'Downloads at the same time' });
    await user.clear(concurrent);
    await user.type(concurrent, '2{Enter}');
    await waitFor(() => expect(app.controls.max_concurrent_downloads).toBe(2));
  });

  it('changes how videos download in the app, keeping the audio quality nearest a new format', async () => {
    const { app } = await openPopup({
      configure: (fake) => {
        fake.controls.media.audio = {
          ...fake.controls.media.audio,
          format: 'mp3',
          audio_kbps: 250,
        };
      },
    });
    const user = userEvent.setup();
    await unfold('Video and audio');
    await user.click(screen.getByRole('switch', { name: 'Show a download button on videos' }));
    await waitFor(() => expect(app.settings.show_media_button).toBe(false));

    await user.click(screen.getByRole('combobox', { name: 'Resolution' }));
    await user.click(screen.getByRole('option', { name: '720p' }));
    await waitFor(() => expect(app.controls.media.quality).toEqual({ height: 720 }));
    // A height has a fallback; Best available does not need one.
    expect(
      screen.getByRole('combobox', { name: 'If a video doesn’t offer that resolution' }),
    ).toBeTruthy();

    await user.click(screen.getByRole('combobox', { name: 'Save audio as' }));
    await user.click(screen.getByRole('option', { name: 'M4A' }));
    await waitFor(() => expect(app.controls.media.audio.format).toBe('m4a'));
    expect(app.controls.media.audio.audio_kbps).toBe(256);

    await user.click(screen.getByRole('switch', { name: 'Add chapters to videos' }));
    await waitFor(() => expect(app.controls.media.video.chapters).toBe(true));
  });

  it('shows only what an app from before the popup controls can change', async () => {
    const { app } = await openPopup({
      configure: (fake) => {
        fake.versions = { oldest: 1, newest: 1 };
      },
    });
    expect(screen.queryByRole('switch', { name: 'Ask about each video' })).toBeNull();
    await unfold('Video and audio');
    expect(
      screen.getByText('Update Wirecatch to choose how videos download from here.'),
    ).toBeTruthy();
    expect(screen.getByRole('switch', { name: 'Show a download button on videos' })).toBeTruthy();
    expect(app.calls('get_browser_controls')).toEqual([]);
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

  it('connects a closed Wirecatch in the tray, without its window or a status line', async () => {
    const { app } = await openPopup({
      configure: (fake) => {
        fake.running = false;
      },
    });
    expect(screen.getByText('Closed')).toBeTruthy();
    expect(screen.getByRole('heading', { name: 'Wirecatch is not running.' })).toBeTruthy();
    await userEvent.setup().click(await screen.findByRole('button', { name: 'Connect Wirecatch' }));
    await screen.findByText('Connected');
    expect(app.starts).toBe(1);
    expect(app.calls('show_window')).toHaveLength(0);
    const status = within(screen.getByRole('contentinfo')).getByRole('status');
    expect(status.textContent.trim()).toBe('');
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

  it('shows no settings while the app is unreachable, only the reason and the fix (D61)', async () => {
    const { app } = await openPopup({
      configure: (app, cache) => {
        // The session copy judges downloads meanwhile, but the popup never shows it.
        cache.value = {
          settings: { ...app.settings, capture_enabled: false },
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
    expect(
      screen.getByText(
        'Your settings are stored in Wirecatch and appear here once it is connected.',
      ),
    ).toBeTruthy();
    expect(screen.queryByRole('switch', { name: 'Capture downloads' })).toBeNull();
    expect(screen.queryByRole('button', { name: /^Downloads/ })).toBeNull();
    // Disabled with the reason, which a screen reader hears and the pointer sees.
    for (const name of ['Pause all', 'Resume all']) {
      expect(
        screen.getByRole('button', { name, description: 'Wirecatch is not connected.' }),
      ).toSatisfy(isUnavailable);
    }

    // "Try again" once the problem is fixed: the app's own settings show, ready to change.
    app.hostMissing = null;
    await userEvent.setup().click(screen.getByRole('button', { name: 'Try again' }));
    await waitFor(() => expect(screen.getByText('Connected')).toBeTruthy());
    await waitFor(() => expect(isUnavailable(captureSwitch())).toBe(false));
    expect(captureSwitch().getAttribute('aria-checked')).toBe('true');
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
        name: 'Wirecatch does not recognize this copy of the extension.',
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
    expect(
      screen.getByText(
        'Your settings are stored in Wirecatch and appear here once it is connected.',
      ),
    ).toBeTruthy();
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
    const list = await screen.findByRole('list', { name: 'Alerts' });
    await waitFor(() => expect(within(list).getAllByRole('listitem')).toHaveLength(2));
    const dismiss = (message: string): HTMLElement =>
      screen.getByRole('button', { name: 'Dismiss', description: message });
    const user = userEvent.setup();

    await user.click(dismiss('Second message.'));
    await waitFor(() => expect(document.activeElement).toBe(dismiss('First message.')));

    await user.click(dismiss('First message.'));
    await waitFor(() => expect(screen.queryByRole('list', { name: 'Alerts' })).toBeNull());
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
    // Clicked without awaiting, so focus moves before the list without the alert arrives.
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

  it('has no axe violations with the access alert and the app closed', async () => {
    const { container } = await openPopup({
      siteAccess: false,
      configure: (app) => {
        app.running = false;
      },
    });
    await screen.findByRole('button', { name: 'Connect Wirecatch' });
    expect(await axeViolations(container)).toEqual([]);
  });

  it('has no axe violations when the app is unreachable', async () => {
    const { container } = await openPopup({
      configure: (app, cache) => {
        cache.value = { settings: app.settings, appClosed: false };
        app.hostMissing = 'Specified native messaging host not found.';
      },
    });
    expect(await axeViolations(container)).toEqual([]);
  });

  it('reaches every control with Tab, in reading order', async () => {
    await openPopup();
    const user = userEvent.setup();
    const expected = [
      'button:Open Wirecatch settings',
      'button:About Capture downloads',
      'switch:Capture downloads',
      'button:About Show a window before each download',
      'switch:Show a window before each download',
      'button:About Ask about each video',
      'switch:Ask about each video',
      'button:About Ignore example.com',
      'switch:Ignore example.com',
      'button:Downloads No speed limit · 3 at once · Main',
      'button:Video and audio Best available · Original',
      'button:Pause all',
      'button:Resume all',
    ];
    for (const control of expected) {
      await user.tab();
      expect(describeFocus(document.activeElement)).toBe(control);
    }
    // Nothing else takes focus: the next Tab leaves the popup content.
    await user.tab();
    expect(document.activeElement).toBe(document.body);
  });

  it('works with the keyboard alone: Space toggles capture, Enter ignores the site', async () => {
    const { app } = await openPopup();
    const user = userEvent.setup();
    await user.tab();
    await user.tab();
    await user.tab();
    expect(document.activeElement).toBe(captureSwitch());
    await user.keyboard(' ');
    await waitFor(() => expect(app.settings.capture_enabled).toBe(false));
    for (let i = 0; i < 6; i += 1) await user.tab();
    expect(document.activeElement).toBe(siteSwitch());
    await user.keyboard('{Enter}');
    await waitFor(() => expect(app.settings.ignored_sites).toEqual(['example.com']));
  });

  it('turns private windows off and on, and hides the switch from an app that does not know it (D82)', async () => {
    const { app } = await openPopup({
      configure: (fake) => (fake.settings = { ...fake.settings, private_windows: true }),
    });
    const privateSwitch = () => screen.getByRole('switch', { name: 'Work in private windows' });
    expect(privateSwitch().getAttribute('aria-checked')).toBe('true');
    await userEvent.click(privateSwitch());
    await waitFor(() => expect(app.settings.private_windows).toBe(false));
    expect(privateSwitch().getAttribute('aria-checked')).toBe('false');
    cleanup();

    const old = await openPopup();
    const noSwitch = () => screen.queryByRole('switch', { name: 'Work in private windows' });
    expect(noSwitch()).toBeNull();
    await userEvent.click(captureSwitch());
    expect(noSwitch()).toBeNull();
    await waitFor(() => expect(old.app.settings.capture_enabled).toBe(false));
    expect(noSwitch()).toBeNull();
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
        controls: null,
        queues: [],
        alerts: [],
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
