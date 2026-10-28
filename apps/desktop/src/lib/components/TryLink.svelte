<script lang="ts">
  // Try a link: which queue the claims send a link to and why, a switched-off queue that would have taken it included (D72.5, D72.7). The core matches, so this and the downloads never disagree.
  import type { DraftClaims, QueueId } from '@wirecatch/protocol';
  import { Button, Field, Input, type FieldControl } from '@wirecatch/ui';
  import { trialLines } from '$lib/queue-reasons';
  import { getApp } from '$lib/state/app.svelte';

  let {
    draft,
    draftName = 'This queue',
  }: {
    /** One queue's claims as its form has them, tried in its place; none tries the saved claims. */
    draft?: (() => DraftClaims) | undefined;
    /** The name of the queue `draft` stands for. */
    draftName?: string;
  } = $props();

  const app = getApp();
  const formId = $props.id();

  let url = $state('');
  let lines = $state<string[]>([]);
  let problem = $state('');
  let busy = $state(false);

  function name(queue: QueueId | null): string {
    const drafted = draft?.();
    if (queue === null || queue === drafted?.queue) return draftName.trim() || 'This queue';
    return app.queues.byId(queue)?.settings.name ?? 'A deleted queue';
  }

  async function run() {
    const link = url.trim();
    if (!link || busy) return;
    busy = true;
    try {
      const found = await app.queues.tryLink({ url: link, draft: draft?.() ?? null });
      if (typeof found === 'string') {
        problem = found;
        lines = [];
      } else {
        problem = '';
        lines = trialLines(found, name, app.queues.defaultQueue?.settings.name ?? 'Main');
      }
    } finally {
      busy = false;
    }
  }
</script>

<div class="flex flex-col gap-2">
  <div class="flex items-end gap-2">
    <Field label="Try a link" class="min-w-0 flex-1">
      {#snippet children(control: FieldControl)}
        <Input
          {...control}
          type="url"
          value={url}
          invalid={problem !== ''}
          aria-describedby="{formId}-result"
          placeholder="https://example.com/file.zip"
          onchange={(value: string) => (url = value)}
          onkeydown={(e: KeyboardEvent) => {
            // Inside the queue form, Enter tries the link instead of saving the queue.
            if (e.key !== 'Enter') return;
            e.preventDefault();
            void run();
          }}
        />
      {/snippet}
    </Field>
    <Button loading={busy} disabled={url.trim() === ''} onclick={run}>Try</Button>
  </div>
  <div id="{formId}-result" role="status" class="flex flex-col gap-0.5 text-small">
    {#if problem}
      <p class="text-danger">{problem}</p>
    {:else}
      {#each lines as line, i (i)}
        <p class={i === 0 ? 'text-ink' : 'text-ink-muted'}>{line}</p>
      {/each}
    {/if}
  </div>
</div>
