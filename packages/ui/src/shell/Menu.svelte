<script lang="ts" module>
  import type { Placement } from '../lib/flyout';

  const PLACEMENT: Placement = { side: 'below', align: 'start', gap: 4, inset: 8 };
</script>

<script lang="ts">
  // A menu opened from a button (a "More" button, a split button's arrow). The trigger is a snippet that spreads the props it is given onto its button: `{#snippet trigger(props)}<IconButton {...props} ... />{/snippet}`.
  import { tick, type Snippet } from 'svelte';
  import { createAttachmentKey } from 'svelte/attachments';
  import { Anchored, dismissOnOutside } from '../lib/anchored.svelte';
  import type { MenuCloseReason, MenuEntry, MenuTriggerProps } from './menu';
  import MenuList from './MenuList.svelte';

  interface Props {
    entries: readonly MenuEntry[];
    label: string;
    align?: Placement['align'];
    trigger: Snippet<[MenuTriggerProps]>;
  }

  let { entries, label, align = 'start', trigger }: Props = $props();

  const id = $props.id();
  let open = $state(false);
  let start = $state<'first' | 'last'>('first');
  let anchor: HTMLElement | null = null;
  let list = $state<HTMLElement | null>(null);
  const anchored = new Anchored();

  async function show(from: 'first' | 'last') {
    start = from;
    open = true;
    await tick();
    if (anchor && list)
      anchored.place(anchor.getBoundingClientRect(), list, { ...PLACEMENT, align });
  }

  function close(reason: MenuCloseReason) {
    open = false;
    // On Tab too: the browser's Tab then moves on from the trigger, the menu's place in the order.
    if (reason !== 'away') anchor?.focus();
  }

  const triggerProps: MenuTriggerProps = {
    'aria-haspopup': 'menu',
    get 'aria-expanded'() {
      return open;
    },
    'aria-controls': id,
    onclick: () => {
      if (open) close('escape');
      else void show('first');
    },
    onkeydown: (event: KeyboardEvent) => {
      if (event.key !== 'ArrowDown' && event.key !== 'ArrowUp') return;
      event.preventDefault();
      void show(event.key === 'ArrowDown' ? 'first' : 'last');
    },
    [createAttachmentKey()]: (node: HTMLElement) => {
      anchor = node;
    },
  };

  $effect(() => {
    if (!open) return;
    return dismissOnOutside(
      () => [anchor, list],
      anchor,
      () => {
        close('away');
      },
    );
  });
</script>

{@render trigger(triggerProps)}

{#if open}
  <MenuList
    bind:element={list}
    {id}
    {entries}
    {label}
    {start}
    style={anchored.style}
    origin={anchored.above ? 'bottom' : 'top'}
    onclose={close}
  />
{/if}
