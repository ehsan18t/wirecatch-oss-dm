<script lang="ts">
  // What an option does, behind an info icon after its label: on hover, keyboard focus and click (D84.2).
  import Info from '~icons/lucide/info';
  import { tooltip } from '../attachments/tooltip.svelte';
  import { cn } from '../lib/cn';
  import { FOCUS_RING } from '../recipes/control';
  import Icon from './Icon.svelte';

  interface Props {
    /** The label of the option it explains, for the button's name ("About …"). */
    label: string;
    text: string;
    class?: string | undefined;
  }

  let { label, text, class: className }: Props = $props();
</script>

<button
  type="button"
  aria-label="About {label}"
  data-info-tip
  class={cn(
    'inline-flex size-5 shrink-0 items-center justify-center rounded-full text-ink-muted transition-colors',
    'hover:text-ink',
    FOCUS_RING,
    className,
  )}
  {@attach tooltip(() => text, { pin: true })}
>
  <Icon icon={Info} size="xs" />
</button>
