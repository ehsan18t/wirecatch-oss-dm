<script lang="ts">
  // What the list says when it has nothing to show, by why: nothing downloaded yet, a search that matches nothing, an empty queue, or a view nothing is in.
  import CircleCheck from '~icons/lucide/circle-check';
  import Download from '~icons/lucide/download';
  import Inbox from '~icons/lucide/inbox';
  import ListPlus from '~icons/lucide/list-plus';
  import Plus from '~icons/lucide/plus';
  import SearchX from '~icons/lucide/search-x';
  import { PRODUCT_NAME } from '@wirecatch/product';
  import { Button, EmptyState } from '@wirecatch/ui';
  import { CATEGORY_LABEL } from '$lib/format';
  import { shortcutLabel } from '$lib/keymap';
  import { getApp } from '$lib/state/app.svelte';
  import { STATUS_GROUP_LABEL } from '$lib/state/view';
  import { CATEGORY_ICON } from '../category-icons';

  const app = getApp();

  const firstRun = $derived(app.counts.all === 0);
  const search = $derived(app.search.trim());
  const queue = $derived(app.view.kind === 'queue' ? app.queues.byId(app.view.id) : undefined);
  const otherQueue = $derived(
    app.queues.list.find((q) => q.id !== queue?.id)?.settings.name ?? 'another queue',
  );

  /** A view with nothing in it: what it would hold, and its picture. */
  const nothing = $derived.by(() => {
    const v = app.view;
    if (v.kind === 'status' && v.group === 'failed')
      return { title: 'Nothing needs your attention', icon: CircleCheck, tone: 'success' as const };
    if (v.kind === 'status')
      return {
        title: `Nothing ${STATUS_GROUP_LABEL[v.group].toLowerCase()}`,
        icon: Inbox,
        tone: 'neutral' as const,
      };
    if (v.kind === 'category')
      return {
        title: `No ${CATEGORY_LABEL[v.category].toLowerCase()} yet`,
        icon: CATEGORY_ICON[v.category],
        tone: 'neutral' as const,
      };
    return { title: 'No downloads here', icon: Inbox, tone: 'neutral' as const };
  });
</script>

<div
  class="flex min-h-0 flex-1 items-center justify-center overflow-y-auto px-6 pb-12"
  role="status"
>
  {#if firstRun}
    <EmptyState
      icon={Download}
      title="Copy a link, then press {shortcutLabel('paste')} here"
      description="{app.name} downloads it right away, using several connections at once. Downloads you start in your browser show up here too once the {PRODUCT_NAME} extension is installed."
    >
      {#snippet actions()}
        <Button
          variant="primary"
          icon={Plus}
          onclick={() => app.openDialog({ kind: 'add', urls: [] })}>New download</Button
        >
        <Button onclick={() => (app.paletteOpen = true)}>See everything {app.name} can do</Button>
      {/snippet}
    </EmptyState>
  {:else if search}
    <EmptyState
      icon={SearchX}
      tone="neutral"
      title={`Nothing matches “${search}”`}
      description="The search looks at file names and links in this view."
    >
      {#snippet actions()}
        <Button onclick={() => (app.search = '')}>Clear the search</Button>
        {#if app.view.kind !== 'all'}
          <Button variant="ghost" onclick={() => app.show({ kind: 'all' })}
            >Search all downloads</Button
          >
        {/if}
      {/snippet}
    </EmptyState>
  {:else if queue}
    <EmptyState
      icon={ListPlus}
      tone="neutral"
      title="{queue.settings.name} is empty"
      description="Add a download straight into this queue, or drag downloads onto it in the sidebar from {otherQueue} or any other list."
    >
      {#snippet actions()}
        <Button
          variant="primary"
          icon={Plus}
          onclick={() => app.openDialog({ kind: 'add', urls: [], queue: queue.id })}
          >Add to {queue.settings.name}</Button
        >
      {/snippet}
    </EmptyState>
  {:else}
    <EmptyState
      icon={nothing.icon}
      tone={nothing.tone}
      title={nothing.title}
      description="Downloads appear here as soon as they match this view."
    >
      {#snippet actions()}
        <Button onclick={() => app.show({ kind: 'all' })}>Show all downloads</Button>
      {/snippet}
    </EmptyState>
  {/if}
</div>
