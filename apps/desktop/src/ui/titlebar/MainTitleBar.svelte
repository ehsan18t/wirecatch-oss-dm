<script lang="ts">
  // The main window's title bar: the navigation toggle, the app's name, the one search (through the view on screen, whose name the field shows, or through settings while they show), how fast everything moves, and the caption buttons. The bar drags the window; Windows 11's snap layouts open over its maximize button.
  import Bell from '~icons/lucide/bell';
  import PanelLeft from '~icons/lucide/panel-left';
  import Search from '~icons/lucide/search';
  import X from '~icons/lucide/x';
  import { AppMark, Badge, IconButton, Input, TitleBar, type SidebarState } from '@wirecatch/ui';
  import { CATEGORY_LABEL } from '$lib/format';
  import { runKey } from '$lib/commands';
  import { SEARCH_ID } from '$lib/dom';
  import { ariaShortcut } from '$lib/keymap';
  import { getApp } from '$lib/state/app.svelte';
  import { STATUS_GROUP_LABEL } from '$lib/state/view';
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

  /** Settings show: the search finds settings, in place of downloads. */
  const inSettings = $derived(app.screen === 'settings');
  const query = $derived(inSettings ? app.settingsQuery : app.search);

  /** The view the search is limited to, by name; null when it searches everything. */
  const scope = $derived.by(() => {
    const v = app.view;
    switch (v.kind) {
      case 'all':
        return null;
      case 'status':
        return STATUS_GROUP_LABEL[v.group];
      case 'queue':
        return app.queues.byId(v.id)?.settings.name ?? 'Queue';
      case 'category':
        return CATEGORY_LABEL[v.category];
    }
  });

  /** What the field says it searches: settings, the view on screen, or every download. */
  const placeholder = $derived(
    inSettings ? 'Search settings' : scope ? `Search ${scope}` : 'Search downloads',
  );

  function searchFor(text: string) {
    if (inSettings) app.settingsQuery = text;
    else app.search = text;
  }

  function onkeydown(e: KeyboardEvent) {
    if (e.key === 'Escape') {
      // The search's own Escape clears what was typed; in an empty search it does what Escape does anywhere in the window, which the field would otherwise keep from it.
      e.preventDefault();
      e.stopPropagation();
      if (query) searchFor('');
      else runKey(app, 'escape');
    } else if (e.key === 'ArrowDown' && !inSettings) {
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
      id={SEARCH_ID}
      type="search"
      size="sm"
      icon={Search}
      label={placeholder}
      {placeholder}
      aria-keyshortcuts={ariaShortcut('search')}
      value={query}
      onchange={searchFor}
      {onkeydown}
    >
      {#snippet trailing()}
        {#if query}
          <IconButton
            icon={X}
            label="Clear the search"
            size="xs"
            onclick={() => {
              searchFor('');
              // The button goes with the text; the focus stays in the search rather than falling to the page.
              document.getElementById(SEARCH_ID)?.focus();
            }}
          />
        {/if}
      {/snippet}
    </Input>
  {/snippet}
  {#snippet trailing()}
    <!-- Each waiting download has its own window (D23); this brings them all back. Its slot is always there, so nothing moves when it comes and goes. -->
    <span class="relative flex size-control-sm shrink-0 items-center justify-center">
      {#if app.questions.waitingCount > 0}
        <IconButton
          icon={Bell}
          size="sm"
          label="{app.questions.waitingCount} waiting for you"
          tooltip="Downloads waiting for your answer"
          onclick={() => app.showQuestionWindows()}
        />
        <span
          class="pointer-events-none absolute -top-0.5 -right-0.5 flex h-4 min-w-4 items-center justify-center rounded-full bg-accent px-1 text-caption font-semibold text-accent-fg tabular-nums"
          aria-hidden="true">{app.questions.waitingCount}</span
        >
      {/if}
    </span>
    <SpeedReadout />
    {#if app.sampleData}
      <Badge tone="warning" size="sm">Sample data</Badge>
    {/if}
  {/snippet}
</TitleBar>
