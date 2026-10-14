/**
 * The options page, rendered in jsdom and wired to the real background hub and native client,
 * which talk to a fake app through a fake native port.
 */
import { queueId } from '@wirecatch/protocol';
import { PRODUCT_NAME } from '@wirecatch/product';
import { cleanup, render, screen, waitFor, within } from '@testing-library/svelte';
import userEvent from '@testing-library/user-event';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { patchOf } from '../src/lib/protocol';
import Options from '../src/ui/options/Options.svelte';
import type { FakeApp, MemoryCache } from './fakes';
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
  openShortcuts: ReturnType<typeof vi.fn>;
}

async function openOptions(
  options: {
    configure?: (app: FakeApp, cache: MemoryCache) => void;
    siteAccess?: boolean;
    shortcut?: string | null;
    /** False for Firefox, where no page can open the shortcuts page. */
    canOpenShortcuts?: boolean;
  } = {},
): Promise<Opened> {
  const wired = await wirePage(options.configure);
  const openShortcuts = vi.fn();
  const { container } = render(Options, {
    props: {
      model: wired.model,
      siteAccess: options.siteAccess ?? true,
      requestSiteAccess: () => Promise.resolve(true),
      mediaShortcut: options.shortcut === undefined ? 'Alt+Shift+D' : options.shortcut,
      openShortcuts: options.canOpenShortcuts === false ? null : openShortcuts,
    },
  });
  return { ...wired, container, openShortcuts };
}

const region = (name: string): HTMLElement => screen.getByRole('region', { name });
const search = (): HTMLElement => screen.getByRole('searchbox', { name: 'Search settings' });
/** The settings sections on show; the toasts' region is outside the page's main content. */
const regionNames = (): string[] =>
  within(screen.getByRole('main'))
    .queryAllByRole('region')
    .map((r) => r.querySelector('h2')?.textContent.trim() ?? '');
/** An error the page announces, as opposed to a toast's live region. */
const pageAlert = (): HTMLElement => within(screen.getByRole('main')).getByRole('alert');
/** The toasts on show, by their text. */
const toasts = (): string[] =>
  [...screen.getByRole('region', { name: 'Notifications' }).querySelectorAll('[data-toast]')].map(
    (t) => t.textContent,
  );

/** Opens a Select and picks one of its options. */
async function choose(
  user: ReturnType<typeof userEvent.setup>,
  name: string,
  option: string,
): Promise<void> {
  await user.click(screen.getByRole('combobox', { name }));
  await user.click(await screen.findByRole('option', { name: option }));
}

afterEach(() => {
  cleanup();
  unwireAll();
});

describe('options page', () => {
  it('shows every setting from the app, in sections, with labels and explanations', async () => {
    await openOptions({
      configure: (app) => {
        app.settings = {
          ...app.settings,
          ask_before_download: true,
          ignored_sites: ['ads.example.org'],
          capture_extensions: ['zip', 'iso'],
          min_capture_size: 5 * 1024 ** 2,
          default_queue: queueId(2),
        };
      },
    });
    expect(
      screen.getByRole('heading', { level: 1, name: `${PRODUCT_NAME} settings` }),
    ).toBeTruthy();
    expect(document.title).toBe(`${PRODUCT_NAME} settings`);
    const capture = region('Capture');
    expect(
      within(capture)
        .getByRole('switch', { name: 'Capture downloads' })
        .getAttribute('aria-checked'),
    ).toBe('true');
    const ask = within(capture).getByRole('switch', {
      name: 'Show a window before each download',
      description: 'Choose where each file goes before it starts.',
    });
    expect(ask.getAttribute('aria-checked')).toBe('true');
    expect(within(capture).getByRole('combobox', { name: 'Default queue' }).textContent).toContain(
      'Night',
    );
    const filters = region('Which downloads');
    expect(
      within(filters).getByRole('textbox', {
        name: 'File types to capture',
        description: 'For example: zip, exe, mp4. Leave empty for every type.',
      }),
    ).toHaveProperty('value', 'zip, iso');
    expect(within(filters).getByRole('spinbutton', { name: 'Minimum file size' })).toHaveProperty(
      'value',
      '5',
    );
    expect(within(filters).getByRole('combobox', { name: 'Size unit' }).textContent).toContain(
      'MB',
    );
    expect(
      within(within(region('Sites')).getByRole('list', { name: 'Sites to ignore' })).getByText(
        'ads.example.org',
      ),
    ).toBeTruthy();
    const media = region('Video and audio');
    expect(
      within(media).getByRole('switch', { name: 'Show download button on videos' }),
    ).toBeTruthy();
    expect(within(region('Connection')).getByText('Connected, version 0.1.0.')).toBeTruthy();
  });

  it('sends a changed setting to the app', async () => {
    const { app } = await openOptions();
    const user = userEvent.setup();
    await user.click(screen.getByRole('switch', { name: 'Show a window before each download' }));
    await waitFor(() => expect(app.settings.ask_before_download).toBe(true));
    expect(app.calls('update_extension_settings')).toEqual([
      { method: 'update_extension_settings', params: patchOf({ ask_before_download: true }) },
    ]);
  });

  it('updates in place when a setting changes in the app', async () => {
    const { app } = await openOptions();
    const capture = screen.getByRole('switch', { name: 'Capture downloads' });
    app.changeSettings({ capture_enabled: false, capture_extensions: ['mp4'] });
    await waitFor(() => expect(capture.getAttribute('aria-checked')).toBe('false'));
    expect(screen.getByRole('textbox', { name: 'File types to capture' })).toHaveProperty(
      'value',
      'mp4',
    );
    expect(screen.getByRole('switch', { name: 'Capture downloads' })).toBe(capture);
  });

  it('edits file types, minimum size, default queue and the ignored sites list', async () => {
    const { app } = await openOptions();
    const user = userEvent.setup();

    await user.type(
      screen.getByRole('textbox', { name: 'File types to capture' }),
      'zip, .EXE{Enter}',
    );
    await waitFor(() => expect(app.settings.capture_extensions).toEqual(['zip', 'exe']));

    const size = screen.getByRole('spinbutton', { name: 'Minimum file size' });
    await user.clear(size);
    await user.type(size, '2');
    await user.tab();
    await waitFor(() => expect(app.settings.min_capture_size).toBe(2 * 1024 ** 2));
    await choose(user, 'Size unit', 'GB');
    await waitFor(() => expect(app.settings.min_capture_size).toBe(2 * 1024 ** 3));
    // Arrow keys step the number, as in any spin button.
    size.focus();
    await user.keyboard('{ArrowUp}');
    await waitFor(() => expect(app.settings.min_capture_size).toBe(3 * 1024 ** 3));

    await choose(user, 'Default queue', 'Night');
    await waitFor(() => expect(app.settings.default_queue).toBe(2));

    await user.type(
      screen.getByRole('textbox', { name: 'Site to ignore' }),
      'https://Tracker.example.net/x{Enter}',
    );
    await waitFor(() => expect(app.settings.ignored_sites).toEqual(['tracker.example.net']));
    await user.click(
      await screen.findByRole('button', { name: 'Stop ignoring tracker.example.net' }),
    );
    await waitFor(() => expect(app.settings.ignored_sites).toEqual([]));
    expect(screen.getByText('No sites ignored.')).toBeTruthy();
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
    const { app } = await openOptions();
    const user = userEvent.setup();
    const types = screen.getByRole('textbox', { name: 'File types to capture' });
    await user.click(types);
    await user.keyboard('zip, ex');
    app.changeSettings({ capture_enabled: false });
    await waitFor(() =>
      expect(
        screen.getByRole('switch', { name: 'Capture downloads' }).getAttribute('aria-checked'),
      ).toBe('false'),
    );
    expect(types).toHaveProperty('value', 'zip, ex');
    await user.keyboard('e{Enter}');
    await waitFor(() => expect(app.settings.capture_extensions).toEqual(['zip', 'exe']));
    await user.tab();
    expect(types).toHaveProperty('value', 'zip, exe');
  });

  it('shows clearly when the default queue no longer exists, and lets another be chosen', async () => {
    const { app } = await openOptions({
      configure: (app) => {
        app.settings = { ...app.settings, default_queue: queueId(9) };
      },
    });
    const select = screen.getByRole('combobox', { name: 'Default queue' });
    expect(select.textContent).toContain('Missing queue (choose another)');
    expect(pageAlert().textContent).toBe(
      'This queue no longer exists in Wirecatch. Choose another one.',
    );
    expect(select.getAttribute('aria-invalid')).toBe('true');
    await choose(userEvent.setup(), 'Default queue', 'Main (default)');
    await waitFor(() => expect(app.settings.default_queue).toBe(1));
    await waitFor(() => expect(screen.queryByText(/no longer exists/)).toBeNull());
  });

  it('explains a site it cannot read next to the field, and sends nothing', async () => {
    const { app } = await openOptions();
    const user = userEvent.setup();
    await user.click(screen.getByRole('button', { name: 'Add' }));
    expect(pageAlert().textContent).toBe('Type a site such as example.com.');
    const field = screen.getByRole('textbox', { name: 'Site to ignore' });
    expect(field.getAttribute('aria-invalid')).toBe('true');
    expect(app.calls('update_extension_settings')).toHaveLength(0);
    // Typing again clears it.
    await user.type(field, 'a');
    expect(within(screen.getByRole('main')).queryByRole('alert')).toBeNull();
  });

  it('says why a change was not saved, and shows the setting as it was', async () => {
    const { app } = await openOptions();
    app.updateError = { code: 'internal', message: 'Settings could not be saved.' };
    const media = screen.getByRole('switch', { name: 'Show download button on videos' });
    await userEvent.setup().click(media);
    await waitFor(() =>
      expect(toasts().join()).toContain('That change was not saved. Settings could not be saved.'),
    );
    expect(media.getAttribute('aria-checked')).toBe('true');
  });

  it('shows the media shortcut and opens the browser page that changes it', async () => {
    const { openShortcuts } = await openOptions();
    const media = region('Video and audio');
    expect([...media.querySelectorAll('kbd kbd')].map((key) => key.textContent)).toEqual([
      'Alt',
      'Shift',
      'D',
    ]);
    await userEvent.setup().click(
      within(media).getByRole('button', {
        name: 'Change shortcut',
        description: "Sends a page's video or audio to Wirecatch.",
      }),
    );
    expect(openShortcuts).toHaveBeenCalledTimes(1);
  });

  it('says where to set the shortcut where no page can open it (Firefox)', async () => {
    await openOptions({ shortcut: null, canOpenShortcuts: false });
    const media = region('Video and audio');
    expect(within(media).getByText('Not set')).toBeTruthy();
    expect(
      within(media).getByText(/Change it in your browser's extension shortcuts\./),
    ).toBeTruthy();
    expect(within(media).queryByRole('button')).toBeNull();
  });

  it('opens Wirecatch from the connection section', async () => {
    const { app } = await openOptions();
    await userEvent.setup().click(
      within(region('Connection')).getByRole('button', {
        name: 'Open Wirecatch',
        description: 'Connected, version 0.1.0.',
      }),
    );
    await waitFor(() => expect(app.calls('show_window')).toHaveLength(1));
    await waitFor(() => expect(toasts().join()).toContain('Wirecatch is open.'));
  });

  it('shows the cached settings read-only, with the reason and the fix, when the app is unreachable', async () => {
    const { app } = await openOptions({
      configure: (app, cache) => {
        cache.value = {
          settings: { ...app.settings, capture_extensions: ['iso'] },
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
    // The saved copy is said inside the connection notice, not in a notice of its own.
    const notice = screen.getByRole('heading', {
      name: 'Wirecatch is not installed, or not set up for this browser.',
    }).parentElement;
    expect(notice?.textContent).toMatch(/Showing your last saved settings/);
    expect(screen.getAllByText(/Showing your last saved settings/)).toHaveLength(1);
    expect(screen.getByRole('textbox', { name: 'File types to capture' })).toHaveProperty(
      'value',
      'iso',
    );
    for (const control of [
      screen.getByRole('switch', { name: 'Capture downloads' }),
      screen.getByRole('textbox', { name: 'File types to capture' }),
      screen.getByRole('spinbutton', { name: 'Minimum file size' }),
      screen.getByRole('combobox', { name: 'Default queue' }),
      screen.getByRole('textbox', { name: 'Site to ignore' }),
      screen.getByRole('button', { name: 'Add' }),
    ]) {
      expect(control).toSatisfy(isUnavailable);
    }

    app.hostMissing = null;
    // One way to try again: the notice's; the Connection row only says how things stand.
    expect(screen.getAllByRole('button', { name: 'Try again' })).toHaveLength(1);
    await userEvent.setup().click(screen.getByRole('button', { name: 'Try again' }));
    await waitFor(() => expect(screen.getByText('Connected')).toBeTruthy());
    await waitFor(() =>
      expect(isUnavailable(screen.getByRole('switch', { name: 'Capture downloads' }))).toBe(false),
    );
    expect(screen.queryByText(/Showing your last saved settings/)).toBeNull();
  });

  it('does not show made-up settings before it has ever been connected', async () => {
    await openOptions({
      configure: (app) => {
        app.hostMissing = 'Specified native messaging host not found.';
      },
    });
    expect(
      screen.getByRole('heading', { name: 'Your settings show here once Wirecatch is connected.' }),
    ).toBeTruthy();
    expect(screen.queryByRole('switch')).toBeNull();
    // The connection is still there; what to do about it is in the notice above.
    expect(within(region('Connection')).getByText(/not installed/)).toBeTruthy();
    expect(screen.getAllByRole('button', { name: 'Try again' })).toHaveLength(1);
  });

  it('offers to open a closed Wirecatch once, in the notice', async () => {
    await openOptions({
      configure: (app) => {
        app.running = false;
      },
    });
    await screen.findByRole('heading', { name: 'Wirecatch is closed.' });
    expect(screen.getAllByRole('button', { name: 'Open Wirecatch' })).toHaveLength(1);
    expect(within(region('Connection')).queryByRole('button')).toBeNull();
  });

  it('keeps focus in the sites list after removing a site, then moves it to the field', async () => {
    await openOptions({
      configure: (app) => {
        app.settings = { ...app.settings, ignored_sites: ['a.example', 'b.example'] };
      },
    });
    const user = userEvent.setup();
    screen.getByRole('button', { name: 'Stop ignoring a.example' }).focus();
    await user.keyboard('{Enter}');
    await waitFor(() =>
      expect(document.activeElement).toBe(
        screen.getByRole('button', { name: 'Stop ignoring b.example' }),
      ),
    );
    await user.keyboard('{Enter}');
    await waitFor(() =>
      expect(document.activeElement).toBe(screen.getByRole('textbox', { name: 'Site to ignore' })),
    );
  });

  it('offers to allow website access again when it was revoked', async () => {
    await openOptions({ siteAccess: false });
    await userEvent.setup().click(screen.getByRole('button', { name: 'Allow access' }));
    await waitFor(() => expect(screen.queryByRole('button', { name: 'Allow access' })).toBeNull());
  });
});

describe('searching the options page', () => {
  it('shows only the settings that match, in their sections', async () => {
    await openOptions();
    const user = userEvent.setup();
    await user.type(search(), 'queue');
    expect(screen.getByRole('combobox', { name: 'Default queue' })).toBeTruthy();
    expect(screen.queryByRole('switch')).toBeNull();
    expect(regionNames()).toEqual(['Capture']);
  });

  it('finds a setting by its description and by words that are not on screen', async () => {
    await openOptions();
    const user = userEvent.setup();
    await user.type(search(), 'intercept');
    expect(screen.getAllByRole('switch')).toHaveLength(1);
    expect(screen.getByRole('switch', { name: 'Capture downloads' })).toBeTruthy();
    await user.clear(search());
    await user.type(search(), 'subdomains');
    expect(regionNames()).toEqual(['Sites']);
    expect(screen.getByRole('textbox', { name: 'Site to ignore' })).toBeTruthy();
  });

  it('finds settings by the title of their section', async () => {
    await openOptions();
    await userEvent.setup().type(search(), 'which downloads');
    expect(regionNames()).toEqual(['Which downloads']);
    expect(screen.getByRole('textbox', { name: 'File types to capture' })).toBeTruthy();
    expect(screen.getByRole('spinbutton', { name: 'Minimum file size' })).toBeTruthy();
  });

  it('says it is waiting, not that nothing matches, before the settings have ever arrived', async () => {
    await openOptions({
      configure: (app) => {
        app.hostMissing = 'Specified native messaging host not found.';
      },
    });
    const user = userEvent.setup();
    await user.type(search(), 'queue');
    expect(
      screen.getByRole('heading', { name: 'Your settings show here once Wirecatch is connected.' }),
    ).toBeTruthy();
    expect(screen.queryByText(/No settings match/)).toBeNull();
    await user.clear(search());
    await user.type(search(), 'zzz');
    expect(screen.getByRole('heading', { name: 'No settings match “zzz”' })).toBeTruthy();
    expect(screen.queryByText('Your settings show here once Wirecatch is connected.')).toBeNull();
  });

  it('says when nothing matches, and clears the search with a button or Escape', async () => {
    await openOptions();
    const user = userEvent.setup();
    await user.type(search(), 'zzz');
    expect(screen.getByRole('heading', { name: 'No settings match “zzz”' })).toBeTruthy();
    expect(regionNames()).toEqual([]);
    await user.click(screen.getByRole('button', { name: 'Clear search' }));
    expect(search()).toHaveProperty('value', '');
    // The button goes with the empty state; focus goes back to the search box, not the page.
    expect(document.activeElement).toBe(search());
    const every = ['Capture', 'Which downloads', 'Sites', 'Video and audio', 'Connection'];
    expect(regionNames()).toEqual(every);

    await user.type(search(), 'size');
    expect(regionNames()).toEqual(['Which downloads']);
    await user.keyboard('{Escape}');
    expect(search()).toHaveProperty('value', '');
    expect(document.activeElement).toBe(search());
    expect(regionNames()).toEqual(every);
  });
});

describe('options page accessibility', () => {
  it('has no axe violations when connected', async () => {
    const { container } = await openOptions({
      configure: (app) => {
        app.settings = { ...app.settings, ignored_sites: ['a.example', 'b.example'] };
      },
    });
    expect(await axeViolations(container)).toEqual([]);
  });

  it('has no axe violations with the access notice and a missing default queue', async () => {
    const { container } = await openOptions({
      siteAccess: false,
      configure: (app) => {
        app.settings = { ...app.settings, default_queue: queueId(9) };
      },
    });
    expect(await axeViolations(container)).toEqual([]);
  });

  it('has no axe violations when the app is unreachable, or when a search finds nothing', async () => {
    const { container } = await openOptions({
      configure: (app, cache) => {
        cache.value = { settings: app.settings, queues: [], savedAt: 1, appClosed: false };
        app.hostMissing = 'Specified native messaging host not found.';
      },
    });
    expect(await axeViolations(container)).toEqual([]);
    await userEvent.setup().type(search(), 'zzz');
    expect(await axeViolations(container)).toEqual([]);
  });

  it('reaches every control with Tab, in reading order', async () => {
    await openOptions({
      configure: (app) => {
        app.settings = { ...app.settings, ignored_sites: ['a.example'] };
      },
    });
    const user = userEvent.setup();
    const order: string[] = [];
    for (let i = 0; i < 13; i += 1) {
      await user.tab();
      order.push(describeFocus(document.activeElement));
    }
    expect(order).toEqual([
      'input:Search settings',
      'switch:Capture downloads',
      'switch:Show a window before each download',
      'combobox:Default queue',
      'input:File types to capture',
      'spinbutton:Minimum file size',
      'combobox:Size unit',
      'button:Stop ignoring a.example',
      'input:Site to ignore',
      'button:Add',
      'switch:Show download button on videos',
      'button:Change shortcut',
      'button:Open Wirecatch',
    ]);
    await user.tab();
    expect(document.activeElement).toBe(document.body);
  });

  it('works with the keyboard alone: Space toggles a switch, Enter adds a site', async () => {
    const { app } = await openOptions();
    const user = userEvent.setup();
    await user.tab();
    await user.tab();
    expect(document.activeElement).toBe(screen.getByRole('switch', { name: 'Capture downloads' }));
    await user.keyboard(' ');
    await waitFor(() => expect(app.settings.capture_enabled).toBe(false));
    screen.getByRole('textbox', { name: 'Site to ignore' }).focus();
    await user.keyboard('keyboard.example{Enter}');
    await waitFor(() => expect(app.settings.ignored_sites).toEqual(['keyboard.example']));
  });
});
