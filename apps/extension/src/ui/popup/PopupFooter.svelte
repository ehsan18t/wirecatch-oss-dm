<script lang="ts">
  import { Button, cn } from '@wirecatch/ui';
  import Pause from '~icons/lucide/pause';
  import Play from '~icons/lucide/play';
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

<footer class="flex flex-col gap-1.5 border-t border-line px-4 pt-3 pb-2">
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
  <p
    role="status"
    class={cn(
      'min-h-4.5 text-small',
      feedback?.tone === 'error' ? 'text-danger' : 'text-ink-muted',
    )}
  >
    {feedback?.text ?? ''}
  </p>
</footer>
