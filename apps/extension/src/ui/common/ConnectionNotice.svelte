<script lang="ts">
  // What keeps the extension from the app, what to do about it, and the button that does it: open a closed app, or try again.
  import { Button, Callout } from '@wirecatch/ui';
  import { describeProblem, type Problem } from '../../lib/native/status';

  interface Props {
    problem: Problem;
    onopen: () => void;
    onretry: () => void;
  }

  let { problem, onopen, onretry }: Props = $props();

  const text = $derived(describeProblem(problem.kind));
  // A closed app is a state to know about, not a fault.
  const closed = $derived(problem.kind === 'app_closed');
</script>

<Callout tone={closed ? 'info' : 'warning'}>
  <h2 class="font-semibold text-ink">{text.title}</h2>
  <p>{text.fix}</p>
  {#if problem.detail}
    <p class="mt-1 text-small wrap-break-word">Details: {problem.detail}</p>
  {/if}
  <div class="mt-2.5 flex flex-wrap gap-2">
    {#if closed}
      <Button size="sm" variant="primary" onclick={onopen}>Open Wirecatch</Button>
    {:else}
      <Button size="sm" onclick={onretry}>Try again</Button>
    {/if}
  </div>
</Callout>
