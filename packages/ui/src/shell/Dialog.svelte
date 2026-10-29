<script lang="ts" module>
  import type { DialogSize } from './dialog';

  /** The longest a double click takes, near enough: a press this soon after opening finishes the click that opened it. */
  const DOUBLE_CLICK = 500;

  /** Dialogs open now: one opened over another sits lower, so it never lines up with the one below. */
  let openDialogs = 0;

  const WIDTH: Record<DialogSize, string> = {
    sm: 'max-w-dialog-sm',
    md: 'max-w-dialog-md',
    lg: 'max-w-dialog-lg',
  };
</script>

<script lang="ts">
  // A modal dialog on the native <dialog>: shown in the top layer, with the page behind it inert. On top of that it starts focus on its first control, keeps Tab inside, closes on Escape (only the topmost dialog does) and on a click on the veil, and gives focus back to what opened it. It is controlled: `onclose` asks to close, and the owner sets `open`. It hangs from a fixed height rather than centring, so a line appearing in it never moves its title.
  import X from '~icons/lucide/x';
  import type { Snippet } from 'svelte';
  import { cn } from '../lib/cn';
  import { containTab, focusedElement, initialFocus, restoreFocus } from '../lib/focus';
  import { fade, settle } from '../motion/motion';
  import IconButton from '../primitives/IconButton.svelte';
  import { DIALOG_SURFACE, SCRIM } from '../recipes/surface';
  import { ownsKey } from './layer-keys';
  import ModalToasts from './ModalToasts.svelte';

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
    /** Before the title: a picture of what the dialog is about. */
    leading?: Snippet;
    /** After the title, before the close button: its state. */
    meta?: Snippet;
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
    leading,
    meta,
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
  /** When it opened: the second click of the double click that opened it is not a click outside. */
  let shownAt = 0;
  let stacked = $state(false);
  /** More of the content lies below what shows, which the footer's edge says. */
  let more = $state(false);

  function measure(body: HTMLElement) {
    const update = () => {
      more = body.scrollHeight - body.scrollTop - body.clientHeight > 1;
    };
    update();
    const resized = new ResizeObserver(update);
    resized.observe(body);
    for (const child of body.children) resized.observe(child);
    body.addEventListener('scroll', update, { passive: true });
    return () => {
      resized.disconnect();
      body.removeEventListener('scroll', update);
    };
  }

  function show(dialog: HTMLDialogElement) {
    shownAt = performance.now();
    stacked = openDialogs > 0;
    openDialogs += 1;
    const opener = focusedElement();
    if (!dialog.open) dialog.showModal();
    // A dialog with nothing to answer but Close starts on its panel, so it opens without a ring on Close; Tab reaches Close from there.
    const first = initialFocus(dialog);
    (first.hasAttribute('data-dialog-close') && panel ? panel : first).focus();
    return () => {
      openDialogs -= 1;
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
    class="fixed inset-0 m-0 flex size-full max-h-none max-w-none flex-col items-center overflow-hidden border-0 bg-transparent p-4 text-ink backdrop:bg-transparent"
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
      onpointerdown={(event) =>
        (pressedVeil =
          event.target === event.currentTarget && performance.now() - shownAt > DOUBLE_CLICK)}
      onclick={() => {
        if (pressedVeil && dismissible) onclose?.();
        pressedVeil = false;
      }}
    ></div>
    <!-- The space above the panel, which gives way first when the window is short, so a tall dialog keeps its buttons in view. -->
    <div class={cn('shrink', stacked ? 'h-20' : 'h-10')} aria-hidden="true"></div>
    <section
      bind:this={panel}
      tabindex="-1"
      class={cn(
        DIALOG_SURFACE,
        'relative flex max-h-full w-full shrink-0 flex-col overflow-hidden outline-none',
        WIDTH[size],
        className,
      )}
      transition:settle
    >
      <header
        class={cn(
          'flex gap-3 px-6 pt-5 pb-2',
          leading ? 'items-center' : 'items-start',
          dismissible && 'pr-14',
        )}
      >
        {@render leading?.()}
        <div class="flex min-w-0 flex-1 flex-col gap-1">
          <h2
            id={titleId}
            class="font-display text-headline font-semibold wrap-break-word text-ink"
          >
            {title}
          </h2>
          {#if description}
            <p id={descriptionId} class="text-body text-ink-muted">{description}</p>
          {/if}
        </div>
        {@render meta?.()}
      </header>
      {#if children}
        <div class="min-h-0 flex-1 overflow-y-auto px-6 py-3 text-body" {@attach measure}>
          {@render children()}
        </div>
      {/if}
      {#if footer}
        <footer
          class={cn(
            'relative flex flex-wrap items-center justify-end gap-2 border-t border-line bg-wash px-6 py-4 transition-shadow duration-fast',
            more && 'shadow-scroll-edge',
          )}
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
          data-dialog-close
          class="absolute top-4 right-4"
          onclick={() => onclose?.()}
        />
      {/if}
    </section>
    <!-- In the dialog, not its panel: the panel moves as it opens, and the toasts keep to the window's corner. -->
    <ModalToasts />
  </dialog>
{/if}
