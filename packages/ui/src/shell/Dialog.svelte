<script lang="ts" module>
  import type { DialogSize } from './dialog';

  const WIDTH: Record<DialogSize, string> = {
    sm: 'max-w-dialog-sm',
    md: 'max-w-dialog-md',
    lg: 'max-w-dialog-lg',
  };
</script>

<script lang="ts">
  // A modal dialog on the native <dialog>: shown in the top layer, with the page behind it inert. On top of that it starts focus on its first control, keeps Tab inside, closes on Escape (only the topmost dialog does) and on a click on the veil, and gives focus back to what opened it. It is controlled: `onclose` asks to close, and the owner sets `open`.
  import X from '~icons/lucide/x';
  import type { Snippet } from 'svelte';
  import { cn } from '../lib/cn';
  import { containTab, focusedElement, initialFocus, restoreFocus } from '../lib/focus';
  import { fade, settle } from '../motion/motion';
  import IconButton from '../primitives/IconButton.svelte';
  import { DIALOG_SURFACE, SCRIM } from '../recipes/surface';
  import { ownsKey } from './layer-keys';
  import ModalAnnouncer from './ModalAnnouncer.svelte';

  interface Props {
    open: boolean;
    title: string;
    /** A line under the title, which also describes the dialog to screen readers. */
    description?: string | undefined;
    size?: DialogSize;
    /** `alertdialog` for a question about something that cannot be undone. */
    role?: 'dialog' | 'alertdialog';
    /** False keeps the dialog open on Escape and on a click outside, for one that must be answered. */
    dismissible?: boolean;
    onclose?: () => void;
    /** The buttons at the bottom, the main action last. */
    footer?: Snippet;
    class?: string | undefined;
    children?: Snippet;
  }

  let {
    open,
    title,
    description,
    size = 'md',
    role = 'dialog',
    dismissible = true,
    onclose,
    footer,
    class: className,
    children,
  }: Props = $props();

  const id = $props.id();
  const titleId = `${id}-title`;
  const descriptionId = `${id}-description`;
  let panel = $state<HTMLElement | null>(null);
  // A drag that starts inside (selecting text) and ends on the veil is not a click outside.
  let pressedVeil = false;

  function show(dialog: HTMLDialogElement) {
    const opener = focusedElement();
    if (!dialog.open) dialog.showModal();
    initialFocus(dialog).focus();
    return () => {
      restoreFocus(opener, dialog);
    };
  }

  /** The browser closed the dialog itself (a form with method="dialog", any other native close): tell the owner, so `open` never stays true over a closed dialog. */
  function onNativeClose() {
    if (open) onclose?.();
  }

  function onkeydown(event: KeyboardEvent & { currentTarget: HTMLDialogElement }) {
    if (!ownsKey(event, event.currentTarget)) return;
    if (event.key === 'Escape') {
      // Handled here, so the browser's own Escape never closes the dialog behind the owner's back.
      event.preventDefault();
      if (dismissible) onclose?.();
    } else if (panel) {
      containTab(event, panel);
    }
  }
</script>

{#if open}
  <dialog
    {role}
    aria-modal="true"
    aria-labelledby={titleId}
    aria-describedby={description ? descriptionId : undefined}
    class="fixed inset-0 m-0 flex size-full max-h-none max-w-none items-center justify-center overflow-hidden border-0 bg-transparent p-4 text-ink backdrop:bg-transparent"
    {@attach show}
    {onkeydown}
    oncancel={(event) => {
      event.preventDefault();
    }}
    onclose={onNativeClose}
  >
    <div
      class={cn('absolute inset-0', SCRIM)}
      aria-hidden="true"
      transition:fade
      onpointerdown={(event) => (pressedVeil = event.target === event.currentTarget)}
      onclick={() => {
        if (pressedVeil && dismissible) onclose?.();
        pressedVeil = false;
      }}
    ></div>
    <section
      bind:this={panel}
      tabindex="-1"
      class={cn(
        DIALOG_SURFACE,
        'relative flex max-h-full w-full flex-col overflow-hidden outline-none',
        WIDTH[size],
        className,
      )}
      transition:settle
    >
      <header class={cn('flex items-start gap-3 px-6 pt-5 pb-2', dismissible && 'pr-14')}>
        <div class="flex min-w-0 flex-1 flex-col gap-1">
          <h2 id={titleId} class="font-display text-headline font-semibold text-ink">{title}</h2>
          {#if description}
            <p id={descriptionId} class="text-body text-ink-muted">{description}</p>
          {/if}
        </div>
      </header>
      {#if children}
        <div class="min-h-0 flex-1 overflow-y-auto px-6 py-3 text-body">{@render children()}</div>
      {/if}
      {#if footer}
        <footer
          class="flex flex-wrap items-center justify-end gap-2 border-t border-line bg-wash px-6 py-4"
        >
          {@render footer()}
        </footer>
      {/if}
      {#if dismissible}
        <!-- Last in the page order, so focus starts on the dialog's own controls and Tab reaches this after them. -->
        <IconButton
          icon={X}
          label="Close"
          size="sm"
          class="absolute top-4 right-4"
          onclick={() => onclose?.()}
        />
      {/if}
      <ModalAnnouncer />
    </section>
  </dialog>
{/if}
