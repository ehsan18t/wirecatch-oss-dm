// Commands for settings: opening any one of them, the switches people flip most, the theme,
// and the media add-on, so it can be installed or removed without visiting Settings.

import type { AddonInfo, Theme } from '@wirecatch/protocol';
import { addonAction, isInstalled } from '../addons';
import { SETTINGS, type SettingDef } from '../settings-schema';
import { SECTIONS } from '../settings-sections';
import type { AppState } from '../state/app.svelte';
import { commandsFrom, type Command, type CommandSpec } from './table';

const GROUP = 'Settings';

const OPEN: CommandSpec<AppState> = {
  id: 'settings.open',
  title: 'Open settings',
  key: 'settings',
  keywords: 'preferences options',
  run: (app) => app.openSettings(),
};

/** Export and import are dialogs of their own, named by themselves rather than by section. */
function isTransfer(id: SettingDef['id']): id is 'export' | 'import' {
  return id === 'export' || id === 'import';
}

const SETTING: CommandSpec<{ app: AppState; s: SettingDef }> = {
  id: ({ s }) => `setting.${s.id}`,
  title: ({ s }) =>
    isTransfer(s.id)
      ? s.label
      : `${SECTIONS.find((x) => x.id === s.section)?.title ?? ''}: ${s.label}`,
  keywords: ({ s }) => `${s.help} ${s.keywords}`,
  run: ({ app, s }) => (isTransfer(s.id) ? app.openDialog({ kind: s.id }) : app.openSettings(s.id)),
};

/** The switches as they stand when the palette opens; each command flips one. */
interface Switches {
  app: AppState;
  ask: boolean;
  progress: boolean;
  capture: boolean;
  /** Videos ask for a quality (D50). */
  videos: boolean;
}

/** Each switch, titled by what flipping it does. */
const TOGGLES: readonly CommandSpec<Switches>[] = [
  {
    id: 'toggle.ask',
    title: ({ ask }) =>
      ask ? 'Start browser downloads without asking' : 'Show a window before each browser download',
    keywords: 'confirm prompt ask window question',
    run: ({ app, ask }) => app.setAskBeforeDownload(!ask),
  },
  {
    id: 'toggle.progress',
    title: ({ progress }) =>
      progress
        ? 'Close download windows when downloads start'
        : 'Keep download windows open to show progress',
    keywords: 'progress window close keep',
    run: ({ app, progress }) => app.setShowProgressWindows(!progress),
  },
  {
    id: 'toggle.videos',
    title: ({ videos }) =>
      videos ? 'Download videos with my saved choices' : 'Ask for a quality for each video',
    keywords: 'video quality resolution ask window automatic remember',
    run: ({ app, videos }) => app.updateSettings((d) => void (d.media.ask_each_time = !videos)),
  },
  {
    id: 'toggle.capture',
    title: ({ capture }) =>
      capture ? 'Stop catching browser downloads' : 'Catch browser downloads',
    keywords: 'extension capture off on',
    run: ({ app, capture }) =>
      app.updateSettings((d) => void (d.extension.capture_enabled = !capture)),
  },
];

function useTheme(app: AppState, theme: Theme): Promise<void> {
  return app.updateSettings((d) => void (d.theme = theme));
}

const THEMES: readonly CommandSpec<AppState>[] = [
  {
    id: 'theme.system',
    title: 'Use the system theme',
    keywords: 'appearance auto',
    run: (app) => useTheme(app, 'system'),
  },
  {
    id: 'theme.light',
    title: 'Use the light theme',
    keywords: 'appearance',
    run: (app) => useTheme(app, 'light'),
  },
  {
    id: 'theme.dark',
    title: 'Use the dark theme',
    keywords: 'appearance night',
    run: (app) => useTheme(app, 'dark'),
  },
];

const ADDON_KEYWORDS = 'addon add-on yt-dlp ffmpeg deno youtube video media';

/** One add-on as the palette offers it. */
interface Addon {
  app: AppState;
  addon: AddonInfo;
  /** Its name inside a sentence. */
  name: string;
  /** What its state offers to do. */
  action: ReturnType<typeof addonAction>;
}

/** What the palette offers for one add-on, by its state. */
const ADDON: readonly CommandSpec<Addon>[] = [
  {
    // A failed install is offered again as an install.
    id: ({ addon, action }) => `addon.${action ?? 'install'}.${addon.id}`,
    title: ({ name }) => `Install the ${name}`,
    keywords: ADDON_KEYWORDS,
    when: ({ action }) => action === 'install' || action === 'retry',
    run: ({ app, addon }) => {
      app.openSettings('media_addon');
      return app.addons.install(addon.id);
    },
  },
  {
    id: ({ addon }) => `addon.update.${addon.id}`,
    title: ({ name }) => `Update the ${name}`,
    keywords: ADDON_KEYWORDS,
    when: ({ action }) => action === 'update',
    run: ({ app, addon }) => {
      app.openSettings('media_addon');
      return app.addons.update(addon.id);
    },
  },
  {
    id: ({ addon }) => `addon.check.${addon.id}`,
    title: ({ name }) => `Check for ${name} updates`,
    keywords: `${ADDON_KEYWORDS} update newer version check`,
    when: ({ addon }) => isInstalled(addon),
    run: ({ app }) => app.addons.checkUpdates(),
  },
  {
    id: ({ addon }) => `addon.remove.${addon.id}`,
    title: ({ name }) => `Remove the ${name}`,
    danger: true,
    keywords: `${ADDON_KEYWORDS} uninstall delete`,
    when: ({ addon }) => isInstalled(addon),
    // Removing stops video downloads, so the palette asks first.
    run: ({ app, addon }) => app.openDialog({ kind: 'remove-addon', id: addon.id }),
  },
];

export function settingsCommands(app: AppState): Command[] {
  const switches: Switches = {
    app,
    ask: app.settings?.extension.ask_before_download ?? true,
    progress: app.settings?.show_progress_windows ?? true,
    capture: app.settings?.extension.capture_enabled ?? true,
    videos: app.settings?.media.ask_each_time ?? true,
  };
  return [
    ...commandsFrom(GROUP, [OPEN], app),
    ...SETTINGS.flatMap((s) => commandsFrom(GROUP, [SETTING], { app, s })),
    ...commandsFrom(GROUP, TOGGLES, switches),
    ...commandsFrom(GROUP, THEMES, app),
  ];
}

export function addonCommands(app: AppState): Command[] {
  return (app.addons.list ?? []).flatMap((addon) =>
    commandsFrom(GROUP, ADDON, {
      app,
      addon,
      name: addon.name.toLowerCase(),
      action: addonAction(addon),
    }),
  );
}
