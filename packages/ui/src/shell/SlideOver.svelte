<script lang="ts">
  // A panel that slides in from the right over the content: a download's details. Placed inside a positioned container (the content area), it covers that area's right edge. It is non-modal by default: the list beside it stays usable, focus moves into the panel when it opens and back when it closes, and Escape anywhere closes it unless something nearer used the key. `docked` makes it a column beside the content instead of a layer over its edge, for a wide window: the parent lays the two out in a row, and the column's width grows in, so the content makes room rather than being covered. `modal` puts it over a veil instead, for a narrow window.
  import type { Snippet } from 'svelte';
  import { cn } from '../lib/cn';
  import { findElement } from '../lib/dom';
  import { containTab, focusedElement, modalLayerOpen, restoreFocus } from '../lib/focus';
  import ModalAnnouncer from './ModalAnnouncer.svelte';
  import { expand, fade } from '../motion/motion';
  import { SCRIM } from '../recipes/surface';
  import { ownsKey } from './layer-keys';
  import SlideOverPanel from './SlideOverPanel.svelte';

  interface Props {
    open: boolean;
    title: string;
    /** A line under the title, which also describes the panel to screen readers (as Dialog's does). */
    description?: string | undefined;
    modal?: boolean;
    /** Beside the content, as a column of the parent's row, rather than over it. Non-modal; `modal` wins over it. */
    docked?: boolean;
    onclose?: () => void;
    /** Actions pinned under the content. */
    footer?: Snippet;
    /** Beside the title: a status badge. */
    meta?: Snippet;
    class?: string | undefined;
    children: Snippet;
  }

  let {
    open,
    title,
    description,
    modal = false,
    docked = false,
    onclose,
    footer,
    meta,
    class: className,
    children,
  }: Props = $props();

  const titleId = $props.id();

  /** Over a veil: shows the dialog, then moves focus to the panel (only once the dialog is open can anything in it take focus), and gives it back as it closes. */
  function showModal(dialog: HTMLDialogElement) {
    const opener = focusedElement();
    if (!dialog.open) dialog.showModal();
    findElement(dialog, ':scope > section')?.focus({ preventScroll: true });
    return () => {
      restoreFocus(opener, dialog);
    };
  }

  /** The browser closed the dialog itself (it may, after a cancelled Escape): tell the owner, so `open` never stays true over a closed dialog. */
  function onNativeClose() {
    if (open) onclose?.();
  }

  /** Keys inside the layer: the panel when it sits beside the content, the whole dialog over a veil. */
  function onkeydown(event: KeyboardEvent) {
    const layer = event.currentTarget;
    if (!(layer instanceof HTMLElement) || !ownsKey(event, layer)) return;
    if (event.key === 'Escape') {
      event.preventDefault();
      onclose?.();
    } else if (modal) {
      containTab(event, layer);
    }
  }

  /** Escape from outside the panel (the list beside it) closes it too, unless a layer or control there used the key, or a modal layer above the page (a dialog, the drawer) owns the keys. */
  function onWindowKeydown(event: KeyboardEvent) {
    if (!open || modal || event.key !== 'Escape' || event.defaultPrevented) return;
    if (modalLayerOpen()) return;
    const target = event.target;
    if (target instanceof Element && target.closest('dialog, [data-layer]')) return;
    onclose?.();
  }
</script>

<svelte:window onkeydown={onWindowKeydown} />

{#if open}
  {#if modal}
    <dialog
      aria-modal="true"
      aria-labelledby={titleId}
      aria-describedby={description ? `${titleId}-description` : undefined}
      class="fixed inset-0 m-0 size-full max-h-none max-w-none overflow-hidden border-0 bg-transparent p-0 text-ink backdrop:bg-transparent"
      {@attach showModal}
      {onkeydown}
      oncancel={(event) => {
        event.preventDefault();
      }}
      onclose={onNativeClose}
    >
      <div
        class={cn('absolute inset-0', SCRIM)}
        aria-hidden="true"
        transition:fade|global
        onclick={() => {
          onclose?.();
        }}
      ></div>
      <SlideOverPanel
        {titleId}
        {title}
        {description}
        {modal}
        {onclose}
        {footer}
        {meta}
        class={className}
      >
        {@render children()}
      </SlideOverPanel>
      <ModalAnnouncer />
    </dialog>
  {:else if docked}
    <!-- The column's width grows from nothing while the panel inside keeps its own, so its text never reflows as it opens. -->
    <div
      class="h-full shrink-0 overflow-hidden"
      transition:expand|global={{ axis: 'x', speed: 'slow' }}
    >
      <SlideOverPanel
        {titleId}
        {title}
        {description}
        {modal}
        docked
        {onclose}
        {footer}
        {meta}
        class={className}
        {onkeydown}
      >
        {@render children()}
      </SlideOverPanel>
    </div>
  {:else}
    <SlideOverPanel
      {titleId}
      {title}
      {description}
      {modal}
      {onclose}
      {footer}
      {meta}
      class={className}
      {onkeydown}
    >
      {@render children()}
    </SlideOverPanel>
  {/if}
{/if}
