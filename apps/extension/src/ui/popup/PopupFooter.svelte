<script lang="ts">
  import Pause from '~icons/lucide/pause';
  import Play from '~icons/lucide/play';
  import { Button, cn } from '@wirecatch/ui';
  import type { Feedback } from '../../lib/popup/model.svelte';
  import { offlineReason } from '../connection';
  import { wholeLabel } from '../cut-label';

  interface Props {
    connected: boolean;
    /** The outcome of the last action, read out as it changes. */
    feedback: Feedback | null;
    onpause: () => void;
    onresume: () => void;
  }

  let { connected, feedback, onpause, onresume }: Props = $props();

  const reason = $derived(offlineReason(connected));
  const labelTip = wholeLabel(() => reason);
</script>

<footer class="flex flex-col gap-1.5 border-t border-line px-4 py-3">
  <!-- Each button starts at its label's width, so a longer label (a translation) puts each on a line of its own rather than cut it; only a label wider than the whole row is cut, and offered whole in a tooltip. -->
  <div class="flex flex-wrap gap-2">
    <Button
      size="sm"
      icon={Pause}
      class="max-w-full min-w-0 grow"
      disabled={!connected}
      tooltip={reason}
      onclick={onpause}
      {@attach labelTip}><span class="truncate">Pause all</span></Button
    >
    <Button
      size="sm"
      icon={Play}
      class="max-w-full min-w-0 grow"
      disabled={!connected}
      tooltip={reason}
      onclick={onresume}
      {@attach labelTip}><span class="truncate">Resume all</span></Button
    >
  </div>
  <!-- Kept in the page while empty, so a screen reader hears the next outcome; it takes no room until then. -->
  <p
    role="status"
    class={cn(
      'text-small',
      feedback === null && 'sr-only',
      feedback?.tone === 'error' ? 'text-danger' : 'text-ink-muted',
    )}
  >
    {feedback?.text ?? ''}
  </p>
</footer>
