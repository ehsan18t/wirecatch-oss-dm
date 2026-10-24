<script lang="ts" module>
  import CircleAlert from '~icons/lucide/circle-alert';
  import CircleCheck from '~icons/lucide/circle-check';
  import Info from '~icons/lucide/info';
  import TriangleAlert from '~icons/lucide/triangle-alert';
  import type { IconComponent } from '../primitives/icon';
  import type { ToastTone } from './toaster.svelte';

  const ICON: Record<ToastTone, IconComponent> = {
    info: Info,
    success: CircleCheck,
    warning: TriangleAlert,
    danger: CircleAlert,
  };
</script>

<script lang="ts">
  // The window's toasts, stacked at the bottom right with the newest nearest the corner, each with its actions and a close button, and one button to clear them all while there are several, always at the bottom. Screen readers hear each new toast once, through two live regions that are always on the page (a region must exist before its text arrives): failures interrupt, the rest wait their turn. While a modal layer is open the page behind it is inert, toasts included; the layer repeats new toasts inside itself, for the toaster the window's root provided (`provideToaster`).
  import X from '~icons/lucide/x';
  import { cn } from '../lib/cn';
  import { closestElement, findElement } from '../lib/dom';
  import { reflow, shift } from '../motion/motion';
  import Button from '../primitives/Button.svelte';
  import Icon from '../primitives/Icon.svelte';
  import IconButton from '../primitives/IconButton.svelte';
  import { TONE_FILL, TONE_TEXT } from '../recipes/control';
  import { FLYOUT_SURFACE } from '../recipes/surface';
  import LiveRegions from '../internal/LiveRegions.svelte';
  import type { Toaster } from './toaster.svelte';

  interface Props {
    toaster: Toaster;
    /** The region's name for screen readers. */
    label?: string;
    /** Laid out where it is placed instead of fixed to the window's corner: for a frame that is itself in the corner, and is as tall as its toasts. */
    inPlace?: boolean;
  }

  let { toaster, label = 'Notifications', inPlace = false }: Props = $props();

  let region = $state<HTMLElement | null>(null);
  let hovered = false;
  let focused = false;
  let returnFocus: HTMLElement | null = null;

  const toastElement = (id: string) => findElement(region, `[data-toast="${CSS.escape(id)}"]`);

  function release() {
    focused = false;
    toaster.hold(null);
    toaster.setPaused(hovered);
  }

  // A toast that goes away with focus in it fires no focusout, so each change to the list checks.
  $effect(() => {
    if (!focused) return;
    if (toaster.list.length === 0 || !region?.contains(document.activeElement)) release();
  });

  function onfocusin(event: FocusEvent) {
    if (
      !focused &&
      event.relatedTarget instanceof HTMLElement &&
      !region?.contains(event.relatedTarget)
    ) {
      returnFocus = event.relatedTarget;
    }
    focused = true;
    toaster.setPaused(true);
    const target = event.target;
    const toast = target instanceof Element ? closestElement(target, '[data-toast]') : null;
    toaster.hold(toast?.dataset.toast ?? null);
  }

  function onfocusout(event: FocusEvent) {
    if (!(event.relatedTarget instanceof Node) || !region?.contains(event.relatedTarget)) release();
  }

  function setHovered(next: boolean) {
    hovered = next;
    toaster.setPaused(hovered || focused);
  }

  /** Clears every toast; focus goes back to where it came from. */
  function clearAll() {
    if (region?.contains(document.activeElement) && returnFocus?.isConnected) returnFocus.focus();
    toaster.clear();
  }

  /** Before a focused toast goes, focus moves to its neighbour, or back to where it came from. */
  function dismiss(id: string) {
    if (toastElement(id)?.contains(document.activeElement)) {
      const index = toaster.list.findIndex((toast) => toast.id === id);
      const next = toaster.list[index + 1] ?? toaster.list[index - 1];
      const target = next
        ? findElement(toastElement(next.id), '[data-toast-dismiss]')
        : returnFocus;
      if (target?.isConnected) target.focus();
    }
    toaster.dismiss(id);
  }
</script>

<div
  bind:this={region}
  role="region"
  aria-label={label}
  class={cn(
    'pointer-events-none flex w-toast max-w-full flex-col gap-2',
    inPlace ? 'p-2' : 'fixed right-4 bottom-4 z-toast',
  )}
  onpointerenter={() => setHovered(true)}
  onpointerleave={() => setHovered(false)}
  {onfocusin}
  {onfocusout}
>
  {#each toaster.list as toast (toast.id)}
    <div
      data-toast={toast.id}
      class={cn(
        FLYOUT_SURFACE,
        'pointer-events-auto relative flex items-start gap-3 overflow-hidden py-3 pr-2 pl-4',
      )}
      in:shift={{ from: 'right', by: 'lg' }}
      out:shift={{ from: 'right', by: 'lg', speed: 'normal' }}
      animate:reflow
    >
      <span class={cn('absolute inset-y-0 left-0 w-1', TONE_FILL[toast.tone])} aria-hidden="true"
      ></span>
      <Icon icon={ICON[toast.tone]} size="lg" class={cn('mt-px', TONE_TEXT[toast.tone])} />
      <div class="flex min-w-0 flex-1 flex-col gap-0.5">
        {#if toast.title}
          <p class="truncate text-ui font-semibold text-ink">{toast.title}</p>
        {/if}
        <p class="text-ui wrap-break-word text-ink-muted">{toast.message}</p>
        {#if toast.actions.length > 0}
          <div class="mt-1 flex flex-wrap gap-x-3">
            {#each toast.actions as action (action.label)}
              <Button
                variant="link"
                size="sm"
                onclick={() => {
                  action.run();
                  dismiss(toast.id);
                }}
              >
                {action.label}
              </Button>
            {/each}
          </div>
        {/if}
      </div>
      <IconButton
        icon={X}
        label="Dismiss"
        tooltip={null}
        size="xs"
        data-toast-dismiss
        onclick={() => dismiss(toast.id)}
      />
    </div>
  {/each}
  {#if toaster.list.length > 1}
    <!-- Last, below the newest toast, so it stays in the same corner however many toasts there are and comes last with Tab as on screen; on the toasts' own surface, so it reads on whatever is behind the region, a website included. -->
    <div
      class={cn(FLYOUT_SURFACE, 'pointer-events-auto self-end')}
      in:shift={{ from: 'right', by: 'lg' }}
    >
      <Button variant="ghost" size="sm" onclick={clearAll}>Clear all</Button>
    </div>
  {/if}
</div>

<LiveRegions announcement={toaster.announcement} />
