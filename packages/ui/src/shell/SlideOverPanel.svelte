<script lang="ts">
  // The slide-over's panel, the same whether it sits beside the content or over a veil.
  import X from '~icons/lucide/x';
  import type { Snippet } from 'svelte';
  import { textIfCut, tooltip } from '../attachments/tooltip.svelte';
  import { cn } from '../lib/cn';
  import { focusedElement, restoreFocus } from '../lib/focus';
  import { shift } from '../motion/motion';
  import IconButton from '../primitives/IconButton.svelte';
  import { PANEL_SURFACE } from '../recipes/surface';

  interface Props {
    titleId: string;
    title: string;
    description?: string | undefined;
    modal: boolean;
    /** A column beside the content, rather than a layer over its edge. */
    docked?: boolean;
    onclose?: (() => void) | undefined;
    footer?: Snippet | undefined;
    meta?: Snippet | undefined;
    class?: string | undefined;
    /** Beside the content, the panel is the layer and takes its keys; over a veil the dialog around it does. */
    onkeydown?: ((event: KeyboardEvent) => void) | undefined;
    children: Snippet;
  }

  let {
    titleId,
    title,
    description,
    modal,
    docked = false,
    onclose,
    footer,
    meta,
    class: className,
    onkeydown,
    children,
  }: Props = $props();

  /** Beside the content: takes focus as the panel opens and gives it back as it closes, if it is still inside. Over a veil the dialog does this, once it is open. */
  function takeFocus(panel: HTMLElement) {
    const opener = focusedElement();
    panel.focus({ preventScroll: true });
    return () => {
      restoreFocus(opener, panel);
    };
  }
</script>

<!-- Global: the panel must slide out whichever block above it closes. Over a veil the dialog around it is the layer, so the panel is not one of its own: keys from inside must reach the dialog as its own. -->
<section
  role={modal ? undefined : 'dialog'}
  aria-labelledby={titleId}
  aria-describedby={!modal && description ? `${titleId}-description` : undefined}
  tabindex="-1"
  data-layer={modal ? undefined : ''}
  class={cn(
    'flex w-slide-over max-w-full flex-col outline-none',
    docked
      ? 'h-full border-l border-line text-ink forced-colors:border-sys-text'
      : cn(PANEL_SURFACE, 'absolute inset-y-0 right-0 z-slide-over rounded-l-lg border-y border-l'),
    className,
  )}
  transition:shift|global={{ from: 'right', by: docked ? 'md' : 'panel' }}
  {@attach !modal && takeFocus}
  {onkeydown}
>
  <header class="flex items-start gap-3 border-b border-line px-5 pt-4 pb-3">
    <div class="flex min-w-0 flex-1 flex-col gap-1">
      <!-- The title is often a file name, the one fact the panel must not cut: it wraps, anywhere if it has to, and only a very long one stops after two lines, with the whole of it in a tooltip. -->
      <h2
        id={titleId}
        class="line-clamp-2 font-display text-heading font-semibold wrap-anywhere text-ink"
        {@attach tooltip(() => textIfCut(title))}
      >
        {title}
      </h2>
      {#if meta !== undefined || Boolean(description)}
        <div class="flex min-w-0 items-center gap-2">
          {@render meta?.()}
          {#if description}
            <p id="{titleId}-description" class="truncate text-small text-ink-muted">
              {description}
            </p>
          {/if}
        </div>
      {/if}
    </div>
    <IconButton icon={X} label="Close" size="sm" class="-mr-1.5" onclick={() => onclose?.()} />
  </header>
  <div class="min-h-0 flex-1 overflow-y-auto px-5 py-4">{@render children()}</div>
  {#if footer}
    <footer class="flex flex-wrap items-center gap-2 border-t border-line px-5 py-3">
      {@render footer()}
    </footer>
  {/if}
</section>
