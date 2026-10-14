<script lang="ts" module>
  import type { Placement } from '../lib/flyout';

  const PLACEMENT: Placement = { side: 'below', align: 'start', gap: 6, inset: 8 };
</script>

<script lang="ts">
  // A small non-modal panel anchored to a button: a speed limit, a filter. Focus moves into it as it opens; Escape closes it and returns focus to the button; a click elsewhere closes it and lets focus go where it was clicked.
  import { tick, type Snippet } from 'svelte';
  import { createAttachmentKey } from 'svelte/attachments';
  import { Anchored, dismissOnOutside } from '../lib/anchored.svelte';
  import { cn } from '../lib/cn';
  import { initialFocus } from '../lib/focus';
  import { topLayer } from '../lib/layer';
  import { pop } from '../motion/motion';
  import { FLYOUT_SURFACE, TOP_LAYER } from '../recipes/surface';
  import { ownsKey } from './layer-keys';
  import type { PopoverTriggerProps } from './popover';

  interface Props {
    /** The panel's name for screen readers. */
    label: string;
    align?: Placement['align'];
    trigger: Snippet<[PopoverTriggerProps]>;
    class?: string | undefined;
    /** The content; `close` closes the popover and returns focus to the trigger. */
    children: Snippet<[close: () => void]>;
  }

  let { label, align = 'start', trigger, class: className, children }: Props = $props();

  const id = $props.id();
  let open = $state(false);
  let anchor: HTMLElement | null = null;
  let panel = $state<HTMLElement | null>(null);
  const anchored = new Anchored();

  async function show() {
    open = true;
    await tick();
    if (!anchor || !panel) return;
    anchored.place(anchor.getBoundingClientRect(), panel, { ...PLACEMENT, align });
    initialFocus(panel).focus();
  }

  function close(returnFocus = true) {
    open = false;
    if (returnFocus) anchor?.focus();
  }

  const triggerProps: PopoverTriggerProps = {
    'aria-haspopup': 'dialog',
    get 'aria-expanded'() {
      return open;
    },
    'aria-controls': id,
    onclick: () => {
      if (open) close();
      else void show();
    },
    [createAttachmentKey()]: (node: HTMLElement) => {
      anchor = node;
    },
  };

  function onkeydown(event: KeyboardEvent & { currentTarget: HTMLElement }) {
    if (event.key !== 'Escape' || !ownsKey(event, event.currentTarget)) return;
    event.preventDefault();
    close();
  }

  $effect(() => {
    if (!open) return;
    return dismissOnOutside(
      () => [anchor, panel],
      anchor,
      () => {
        close(false);
      },
    );
  });
</script>

{@render trigger(triggerProps)}

{#if open}
  <div
    bind:this={panel}
    {id}
    role="dialog"
    aria-label={label}
    popover="manual"
    tabindex="-1"
    data-layer
    class={cn(
      TOP_LAYER,
      FLYOUT_SURFACE,
      'z-popover max-w-prose p-4 outline-none',
      anchored.above ? 'origin-bottom' : 'origin-top',
      className,
    )}
    style={anchored.style}
    {@attach topLayer}
    transition:pop
    {onkeydown}
  >
    {@render children(() => {
      close();
    })}
  </div>
{/if}
