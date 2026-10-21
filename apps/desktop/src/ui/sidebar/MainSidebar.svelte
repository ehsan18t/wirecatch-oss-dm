<script lang="ts">
  // The main window's navigation. On the downloads screen: the downloads that wait for an answer, the list by state, and the queues (the first three, the rest folded away), each with how many it holds, and at its foot settings, every action and the shortcuts as icons. On the settings screen the same navigation becomes the list of settings sections, with the way back at its top (D47). Docked with labels in a wide window, as an icon rail when collapsed, and as a drawer in a narrow one.
  import Ban from '~icons/lucide/ban';
  import Bell from '~icons/lucide/bell';
  import ChevronDown from '~icons/lucide/chevron-down';
  import ChevronLeft from '~icons/lucide/chevron-left';
  import CircleAlert from '~icons/lucide/circle-alert';
  import CircleCheck from '~icons/lucide/circle-check';
  import CirclePause from '~icons/lucide/circle-pause';
  import Clock from '~icons/lucide/clock';
  import Download from '~icons/lucide/download';
  import Keyboard from '~icons/lucide/keyboard';
  import Layers from '~icons/lucide/layers';
  import Plus from '~icons/lucide/plus';
  import SlidersHorizontal from '~icons/lucide/sliders-horizontal';
  import SquareTerminal from '~icons/lucide/square-terminal';
  import {
    cn,
    expand,
    Icon,
    IconButton,
    Sidebar,
    SidebarItem,
    SidebarSection,
    type IconComponent,
    type SidebarState,
  } from '@wirecatch/ui';
  import { ariaShortcut, shortcutLabel } from '$lib/keymap';
  import { searchSettings, SECTIONS, type SectionId } from '$lib/settings-schema';
  import { getApp } from '$lib/state/app.svelte';
  import {
    sameView,
    STATUS_GROUP_LABEL,
    STATUS_GROUPS,
    type StatusGroup,
    type View,
  } from '$lib/state/view';
  import { SECTION_ICON } from '../settings/section-icons';
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

  /** Queues shown before the rest fold away under "more queues". */
  const QUEUES_SHOWN = 3;

  function current(view: View): boolean {
    return app.screen === 'downloads' && sameView(app.view, view);
  }

  let moreQueues = $state(false);
  const queues = $derived(app.queues.list);
  // The queue on screen always shows, folded or not.
  const shownQueues = $derived(
    moreQueues
      ? queues
      : queues.filter((q, i) => i < QUEUES_SHOWN || current({ kind: 'queue', id: q.id })),
  );
  const hiddenQueues = $derived(queues.length - shownQueues.length);
  const waiting = $derived(app.questions.waitingCount);

  /** While settings are searched, how many settings of each section match. */
  const matches = $derived.by(() => {
    const query = app.settingsQuery.trim();
    if (!query) return null;
    const counts: Partial<Record<SectionId, number>> = {};
    for (const def of searchSettings(query)) counts[def.section] = (counts[def.section] ?? 0) + 1;
    return counts;
  });
</script>

<Sidebar {sidebar} label={app.screen === 'settings' ? 'Settings sections' : 'Views and queues'}>
  {#snippet header()}
    {#if app.screen === 'settings'}
      <SidebarItem
        label="Back to downloads"
        icon={ChevronLeft}
        onclick={() => (app.screen = 'downloads')}
      />
    {:else if waiting > 0}
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

  {#if app.screen === 'settings'}
    <SidebarSection title="Settings">
      {#each SECTIONS as section (section.id)}
        {@const found = matches?.[section.id] ?? 0}
        <!-- While searching, each section counts its matches; one with none says nothing. -->
        <SidebarItem
          label={section.title}
          icon={SECTION_ICON[section.id]}
          active={!matches && app.settingsSection === section.id}
          count={matches ? found : undefined}
          onclick={() => app.openSettingsSection(section.id)}
        />
      {/each}
    </SidebarSection>
  {:else}
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
      {#each shownQueues as queue (queue.id)}
        <div transition:expand>
          <QueueItem {queue} />
        </div>
      {/each}
      {#if (moreQueues || hiddenQueues > 0) && sidebar.showLabels}
        <button
          type="button"
          data-nav-stop
          aria-expanded={moreQueues}
          class="flex h-control-sm shrink-0 items-center gap-2 rounded-sm px-3 text-small text-ink-subtle transition-colors duration-fast hover:bg-wash hover:text-ink"
          onclick={() => (moreQueues = !moreQueues)}
        >
          <Icon
            icon={ChevronDown}
            size="xs"
            class={cn('transition-transform duration-normal ease-out', !moreQueues && '-rotate-90')}
          />
          {moreQueues
            ? 'Fewer queues'
            : `${String(hiddenQueues)} more queue${hiddenQueues === 1 ? '' : 's'}`}
        </button>
      {/if}
    </SidebarSection>
  {/if}

  {#snippet footer()}
    <div
      class={cn('flex', sidebar.showLabels ? 'justify-around' : 'flex-col items-center gap-0.5')}
    >
      <IconButton
        icon={SlidersHorizontal}
        data-nav-stop
        label="Settings"
        tooltip="Settings ({shortcutLabel('settings')})"
        active={app.screen === 'settings'}
        aria-pressed={app.screen === 'settings'}
        aria-keyshortcuts={ariaShortcut('settings')}
        onclick={() =>
          app.screen === 'settings' ? (app.screen = 'downloads') : app.openSettings()}
      />
      <IconButton
        icon={SquareTerminal}
        data-nav-stop
        label="Find any action"
        tooltip="Find any action or setting ({shortcutLabel('palette')})"
        aria-keyshortcuts={ariaShortcut('palette')}
        onclick={() => (app.paletteOpen = true)}
      />
      <IconButton
        icon={Keyboard}
        data-nav-stop
        label="Keyboard shortcuts"
        tooltip="Keyboard shortcuts ({shortcutLabel('help')})"
        onclick={() => app.openDialog({ kind: 'shortcuts' })}
      />
    </div>
  {/snippet}
</Sidebar>
