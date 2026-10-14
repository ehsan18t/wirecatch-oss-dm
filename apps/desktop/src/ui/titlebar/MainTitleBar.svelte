<script lang="ts">
  // The main window's title bar: the navigation toggle, the app's name, the search through the list, how fast everything moves, the way to every action, and the caption buttons. The bar drags the window; Windows 11's snap layouts open over its maximize button.
  import SquareTerminal from '~icons/lucide/square-terminal';
  import PanelLeft from '~icons/lucide/panel-left';
  import Search from '~icons/lucide/search';
  import X from '~icons/lucide/x';
  import { AppMark, Badge, IconButton, Input, TitleBar, type SidebarState } from '@wirecatch/ui';
  import { runKey } from '$lib/commands';
  import { ariaShortcut, shortcutLabel } from '$lib/keymap';
  import { getApp } from '$lib/state/app.svelte';
  import SpeedReadout from './SpeedReadout.svelte';

  let { sidebar, ontoggle }: { sidebar: SidebarState; ontoggle: () => void } = $props();

  const app = getApp();

  const toggleLabel = $derived(
    sidebar.wide
      ? sidebar.collapsed
        ? 'Expand the navigation'
        : 'Collapse the navigation'
      : 'Open the navigation',
  );

  /** Typing a search shows the list it searches. */
  function searchFor(text: string) {
    app.search = text;
    if (text !== '' && app.screen !== 'downloads') app.screen = 'downloads';
  }

  function onkeydown(e: KeyboardEvent) {
    if (e.key === 'Escape') {
      // The search's own Escape clears what was typed; in an empty search it does what Escape does anywhere in the window, which the field would otherwise keep from it.
      e.preventDefault();
      e.stopPropagation();
      if (app.search) app.search = '';
      else runKey(app, 'escape');
    } else if (e.key === 'ArrowDown') {
      e.preventDefault();
      // From a search, the list takes the keyboard at its first match.
      const first = app.visible[0];
      if (app.search && first !== undefined) app.selection.active = first;
      document.getElementById('download-list')?.focus();
    }
  }
</script>

<TitleBar
  title={app.name}
  controls={app.windowControls}
  onerror={(_action, error) => app.windowFailed(error)}
>
  {#snippet leading()}
    <IconButton
      icon={PanelLeft}
      label={toggleLabel}
      aria-expanded={sidebar.wide ? !sidebar.collapsed : sidebar.drawerOpen}
      onclick={ontoggle}
    />
  {/snippet}
  {#snippet icon()}
    <AppMark />
  {/snippet}
  {#snippet center()}
    <Input
      id="list-search"
      type="search"
      size="sm"
      icon={Search}
      label="Search downloads"
      placeholder="Search names and links ({shortcutLabel('search')})"
      aria-keyshortcuts={ariaShortcut('search')}
      value={app.search}
      onchange={searchFor}
      {onkeydown}
    >
      {#snippet trailing()}
        {#if app.search}
          <IconButton
            icon={X}
            label="Clear the search"
            size="xs"
            onclick={() => {
              app.search = '';
              // The button goes with the text; the focus stays in the search rather than falling to the page.
              document.getElementById('list-search')?.focus();
            }}
          />
        {/if}
      {/snippet}
    </Input>
  {/snippet}
  {#snippet trailing()}
    <SpeedReadout />
    {#if app.sampleData}
      <Badge tone="warning" size="sm">Sample data</Badge>
    {/if}
    <IconButton
      icon={SquareTerminal}
      label="Find any action"
      tooltip="Find any action or setting ({shortcutLabel('palette')})"
      aria-keyshortcuts={ariaShortcut('palette')}
      onclick={() => (app.paletteOpen = true)}
    />
  {/snippet}
</TitleBar>
