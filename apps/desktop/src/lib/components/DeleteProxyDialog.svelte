<script lang="ts">
  // Deleting a proxy profile: what uses it (the app's proxy, queues with it as their own) and what they use instead (D73.4). Downloads running through it finish on it.
  import type { ProxyProfileId, ProxyUse } from '@wirecatch/protocol';
  import { Button, Dialog, Field, Select, type FieldControl } from '@wirecatch/ui';
  import { shownName } from '$lib/proxy';
  import { getApp } from '$lib/state/app.svelte';
  import { usersLine } from '$lib/state/proxy-profiles.svelte';

  let { id }: { id: ProxyProfileId } = $props();
  const app = getApp();

  const profile = $derived(app.proxies.byId(id));
  const label = $derived(profile ? shownName(profile.settings) : 'proxy');
  const users = $derived(app.proxies.usersOf(id));
  const used = $derived(users.app || users.queues.length > 0);

  const SYSTEM = 'system';
  const DIRECT = 'direct';
  let choice = $state(SYSTEM);
  const options = $derived([
    { value: SYSTEM, label: 'The computer’s proxy' },
    { value: DIRECT, label: 'No proxy' },
    ...app.proxies.list
      .filter((p) => p.id !== id)
      .map((p) => ({ value: String(p.id), label: shownName(p.settings) })),
  ]);

  function replacement(): ProxyUse {
    if (choice === SYSTEM || choice === DIRECT) return { kind: choice };
    const other = app.proxies.list.find((p) => String(p.id) === choice);
    return other ? { kind: 'profile', id: other.id } : { kind: 'system' };
  }

  async function remove() {
    // Read before the dialog closes; its props go with it.
    const target = id;
    const instead = replacement();
    app.closeDialog();
    await app.proxies.remove(target, instead);
  }
</script>

<Dialog open role="alertdialog" title={`Delete “${label}”?`} onclose={() => app.closeDialog()}>
  <div class="flex flex-col gap-4">
    <p class="text-ui text-ink-muted">
      {used ? `${usersLine(users)}.` : 'Nothing uses it.'} Downloads running through it finish on it.
    </p>
    {#if used}
      <Field label="Switch what uses it to">
        {#snippet children(control: FieldControl)}
          <Select {...control} value={choice} {options} onchange={(v: string) => (choice = v)} />
        {/snippet}
      </Field>
    {/if}
  </div>

  {#snippet footer()}
    <Button data-autofocus onclick={() => app.closeDialog()}>Keep it</Button>
    <Button variant="danger" onclick={remove}>Delete proxy</Button>
  {/snippet}
</Dialog>
