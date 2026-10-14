<script lang="ts">
  // The main window's navigation: adding a download, the downloads that wait for an answer, the list by status, by queue and by kind of file, each with how many it holds, and settings. Docked with labels in a wide window, as an icon rail when collapsed, and as a drawer in a narrow one. File types folds away, and starts folded in a window too short for every section open.
  import Ban from '~icons/lucide/ban';
  import Bell from '~icons/lucide/bell';
  import CircleAlert from '~icons/lucide/circle-alert';
  import CircleCheck from '~icons/lucide/circle-check';
  import CirclePause from '~icons/lucide/circle-pause';
  import Clock from '~icons/lucide/clock';
  import Download from '~icons/lucide/download';
  import Keyboard from '~icons/lucide/keyboard';
  import Layers from '~icons/lucide/layers';
  import Plus from '~icons/lucide/plus';
  import SettingsIcon from '~icons/lucide/settings';
  import { MediaQuery } from 'svelte/reactivity';
  import {
    Button,
    IconButton,
    Sidebar,
    SidebarItem,
    SidebarSection,
    type IconComponent,
    type SidebarState,
    token,
  } from '@wirecatch/ui';
  import { CATEGORIES, CATEGORY_LABEL } from '$lib/format';
  import { ariaShortcut } from '$lib/keymap';
  import { getApp } from '$lib/state/app.svelte';
  import { fileTypesShown, keepFileTypesShown } from '$lib/state/sidebar-preference';
  import {
    sameView,
    STATUS_GROUP_LABEL,
    STATUS_GROUPS,
    type StatusGroup,
    type View,
  } from '$lib/state/view';
  import { CATEGORY_ICON } from '../category-icons';
  import QueueItem from './QueueItem.svelte';

  let { sidebar }: { sidebar: SidebarState } = $props();

  const app = getApp();

  const STATUS_ICON: Record<StatusGroup, IconComponent> = {
    active: Download,
    waiting: Clock,
    paused: CirclePause,
    finished: CircleCheck,
    failed: CircleAlert,
    cancelled: Ban,
  };

  function current(view: View): boolean {
    return app.screen === 'downloads' && sameView(app.view, view);
  }

  const addDownload = () => app.openDialog({ kind: 'add', urls: [] });

  /** Where the stylesheet is not there to say (unit tests), the window counts as tall. */
  const openHeight = token('--height-nav-open');
  const tall = openHeight === '' ? true : new MediaQuery(`(min-height: ${openHeight})`).current;
  let fileTypesOpen = $state(fileTypesShown(tall));
  const waiting = $derived(app.questions.waitingCount);
</script>

<Sidebar {sidebar} label="Views and queues">
  {#snippet header()}
    {#if sidebar.showLabels}
      <Button
        variant="primary"
        icon={Plus}
        block
        aria-keyshortcuts={ariaShortcut('add')}
        onclick={addDownload}>Add download</Button
      >
    {:else}
      <IconButton
        icon={Plus}
        label="Add download"
        variant="primary"
        size="lg"
        class="mx-auto"
        aria-keyshortcuts={ariaShortcut('add')}
        onclick={addDownload}
      />
    {/if}
    {#if waiting > 0}
      <!-- Each waiting download has its own window (D23); this brings them all back. -->
      <SidebarItem
        label="Waiting for you"
        icon={Bell}
        count={waiting}
        countTone="warning"
        onclick={() => app.showQuestionWindows()}
      />
    {/if}
  {/snippet}

  <SidebarSection title="Downloads">
    <SidebarItem
      label="All downloads"
      icon={Layers}
      active={current({ kind: 'all' })}
      count={app.counts.all}
      onclick={() => app.show({ kind: 'all' })}
    />
    {#each STATUS_GROUPS as group (group)}
      {@const count = app.counts.status[group]}
      {#if group !== 'cancelled' || count > 0 || current({ kind: 'status', group })}
        <SidebarItem
          label={STATUS_GROUP_LABEL[group]}
          icon={STATUS_ICON[group]}
          active={current({ kind: 'status', group })}
          {count}
          countTone={group === 'failed' ? 'danger' : 'neutral'}
          onclick={() => app.show({ kind: 'status', group })}
        />
      {/if}
    {/each}
  </SidebarSection>

  <SidebarSection title="Queues">
    {#snippet action()}
      <IconButton
        icon={Plus}
        label="New queue"
        size="xs"
        onclick={() => app.openDialog({ kind: 'queue', id: null })}
      />
    {/snippet}
    {#each app.queues.list as queue (queue.id)}
      <QueueItem {queue} />
    {/each}
  </SidebarSection>

  <SidebarSection
    title="File types"
    collapsible
    bind:expanded={fileTypesOpen}
    ontoggle={keepFileTypesShown}
  >
    {#each CATEGORIES as category (category)}
      <SidebarItem
        label={CATEGORY_LABEL[category]}
        icon={CATEGORY_ICON[category]}
        active={current({ kind: 'category', category })}
        count={app.counts.category[category]}
        onclick={() => app.show({ kind: 'category', category })}
      />
    {/each}
  </SidebarSection>

  {#snippet footer()}
    <SidebarItem
      label="Settings"
      icon={SettingsIcon}
      active={app.screen === 'settings'}
      onclick={() => app.openSettings()}
    />
    <SidebarItem
      label="Keyboard shortcuts"
      icon={Keyboard}
      onclick={() => app.openDialog({ kind: 'shortcuts' })}
    />
  {/snippet}
</Sidebar>
