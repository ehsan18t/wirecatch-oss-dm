<script lang="ts">
  // A group's heading in the list: its name and how many it holds (and, for what downloads, how fast), and a click folds it away or opens it. Folding is for the pointer here; the command palette folds and opens each group from the keyboard.
  import ChevronDown from '~icons/lucide/chevron-down';
  import { formatSpeed } from '@wirecatch/units';
  import { cn, Icon } from '@wirecatch/ui';
  import { getApp } from '$lib/state/app.svelte';
  import {
    sectionIds,
    STATUS_GROUP_LABEL,
    type ListSection,
    type StatusGroup,
  } from '$lib/state/view';

  let {
    section,
    group,
    rowIndex,
  }: {
    section: ListSection;
    group: StatusGroup;
    /** The heading's place in the grid, as a row. */
    rowIndex: number;
  } = $props();

  const app = getApp();
  /** How fast the group's own downloads go: the view's, not every queue's. */
  const speed = $derived.by(() => {
    if (group !== 'active') return '';
    let total = 0;
    for (const id of sectionIds(app.layout, section)) total += app.downloads.get(id)?.speed ?? 0;
    return formatSpeed(total);
  });
  const note = $derived(
    group === 'waiting' && app.sort.column !== 'position' ? 'in queue order' : '',
  );
</script>

<div role="row" aria-rowindex={rowIndex} class="flex h-group items-end px-2 pb-1">
  <span role="gridcell" class="flex min-w-0">
    <button
      type="button"
      tabindex={-1}
      aria-expanded={!section.folded}
      class="flex h-control-sm min-w-0 items-center gap-2 rounded-sm px-2 text-small font-semibold text-ink-muted transition-colors duration-fast ease-out hover:bg-wash hover:text-ink"
      onclick={() => app.toggleGroup(group)}
    >
      <Icon
        icon={ChevronDown}
        size="xs"
        class={cn(
          'shrink-0 text-ink-subtle transition-transform duration-normal ease-out',
          section.folded && '-rotate-90',
        )}
      />
      <span class="truncate">{STATUS_GROUP_LABEL[group]}</span>
      <span class="font-normal text-ink-subtle tabular-nums"
        >{[String(section.count), speed, note].filter(Boolean).join(' · ')}</span
      >
    </button>
  </span>
</div>
