<script lang="ts">
  // What a download window shows under its title bar (D23): a heading, a body that scrolls, and the buttons at the bottom, which always stay in view. The first `[data-autofocus]` element takes the focus when a view appears, so Enter does what the main button says, and again whenever the control holding the focus goes away. The frame reports the height its content needs, so the window is only as tall as what it shows (see `fit.ts`).
  import { onMount, type Snippet } from 'svelte';
  import { Callout, cn, fade, FOCUS_RING_INSET } from '@wirecatch/ui';
  import { findElement } from '$lib/dom';
  import { fitReporter, getFitWindow, neededHeight } from '$lib/window/fit';

  interface Props {
    title: string;
    /** A line under the heading, which also describes the window's content to screen readers. */
    description?: string | undefined;
    children: Snippet;
    /** What went wrong with the last thing asked of the window, shown just over the buttons, where it is in view however far the body scrolls. */
    error?: string | null | undefined;
    /** The buttons at the bottom, the main action last. */
    footer?: Snippet;
  }

  let { title, description, children, error, footer }: Props = $props();

  const uid = $props.id();
  const titleId = `${uid}-title`;
  const descriptionId = `${uid}-description`;
  let frame = $state<HTMLElement>();
  let body = $state<HTMLElement>();
  let content = $state<HTMLElement>();
  /** Whether the body is taller than the window shows, so it takes the focus for the keyboard to scroll it. */
  let scrolls = $state(false);
  const fit = getFitWindow();

  // Measures again whenever the frame or the content changes size: a view that switches from asking to progress, a message that appears, a longer name, a larger text size.
  $effect(() => {
    const [framePart, bodyPart, contentPart] = [frame, body, content];
    if (!framePart || !bodyPart || !contentPart) return;
    const report = fit === null ? null : fitReporter(fit);
    const measure = () => {
      scrolls = bodyPart.scrollHeight > bodyPart.clientHeight + 1;
      if (!report) return;
      const style = getComputedStyle(bodyPart);
      report(
        neededHeight({
          frame: framePart.getBoundingClientRect().height,
          body: bodyPart.getBoundingClientRect().height,
          content: contentPart.getBoundingClientRect().height,
          padding: parseFloat(style.paddingTop) + parseFloat(style.paddingBottom),
        }),
      );
    };
    const observer = new ResizeObserver(measure);
    observer.observe(framePart);
    observer.observe(bodyPart);
    observer.observe(contentPart);
    measure();
    return () => observer.disconnect();
  });

  /** Where the focus goes when the view appears: its main button, or else its first control. */
  function firstStop(): HTMLElement | null {
    return (
      findElement(frame, '[data-autofocus]') ??
      findElement(frame, 'input:enabled, button:enabled, textarea:enabled')
    );
  }

  onMount(() => {
    // Ready for Enter, with no ring until the keyboard moves the focus (a new window counts as a keyboard start, so it would otherwise draw one).
    firstStop()?.focus({ focusVisible: false });
  });

  // A view changes under the person: a button that follows the download, a question that closes, buttons that swap once the download finishes. When the control holding the focus goes, the focus would fall to the page, where Escape no longer reaches the window and a finished one closes by itself while in use; it goes where it goes when the view appears instead.
  $effect(() => {
    const root = frame;
    if (!root) return;
    let held = root.contains(document.activeElement) ? document.activeElement : null;
    const onfocusin = (event: FocusEvent) => {
      if (event.target instanceof Element) held = event.target;
    };
    const observer = new MutationObserver(() => {
      const active = document.activeElement;
      if (held && !held.isConnected && (active === null || active === document.body)) {
        held = null;
        firstStop()?.focus();
      }
    });
    root.addEventListener('focusin', onfocusin);
    observer.observe(root, { childList: true, subtree: true });
    return () => {
      root.removeEventListener('focusin', onfocusin);
      observer.disconnect();
    };
  });
</script>

<div
  bind:this={frame}
  role="group"
  aria-labelledby={titleId}
  aria-describedby={description ? descriptionId : undefined}
  class="flex h-full min-h-0 flex-col text-ink"
  in:fade
>
  <div class="flex flex-col gap-1 px-5 pt-4 pb-1">
    <h1 id={titleId} class="font-display text-headline font-semibold text-ink wrap-anywhere">
      {title}
    </h1>
    {#if description}
      <p id={descriptionId} class="text-body text-ink-muted wrap-anywhere">{description}</p>
    {/if}
  </div>
  <!-- svelte-ignore a11y_no_noninteractive_tabindex (a body that scrolls takes the focus so the keyboard can scroll it, a region named by the heading; one that fits is no tab stop) -->
  <div
    bind:this={body}
    role={scrolls ? 'region' : undefined}
    aria-labelledby={scrolls ? titleId : undefined}
    tabindex={scrolls ? 0 : undefined}
    class={cn('min-h-0 flex-1 overflow-y-auto px-5 pt-3 pb-4 text-body', FOCUS_RING_INSET)}
  >
    <div bind:this={content}>
      {@render children()}
    </div>
  </div>
  {#if error}
    <div class="shrink-0 px-5 pb-3">
      <Callout tone="danger" live="assertive" title="That did not work">{error}</Callout>
    </div>
  {/if}
  {#if footer}
    <div
      class="flex shrink-0 flex-wrap items-center justify-end gap-2 border-t border-line bg-wash px-5 py-3"
    >
      {@render footer()}
    </div>
  {/if}
</div>
