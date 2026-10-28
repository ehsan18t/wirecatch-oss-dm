<script lang="ts">
  // The saved proxy profiles (D73.1): each with its address and type, its last test and what uses it, and Test, Edit and Delete; under them Add proxy and Copy from system.
  import Pencil from '~icons/lucide/pencil';
  import Trash from '~icons/lucide/trash-2';
  import { MAX_PROXY_PROFILES, type ProxyProfile, type ProxyProfileId } from '@wirecatch/protocol';
  import { Badge, Button, IconButton } from '@wirecatch/ui';
  import { KIND_OPTIONS, lastTestLine, proxyAddress, shownName } from '$lib/proxy';
  import { getApp } from '$lib/state/app.svelte';

  const app = getApp();
  const profiles = $derived(app.proxies.list);
  let testing = $state<ProxyProfileId | null>(null);
  let copying = $state(false);

  function kindLabel(profile: ProxyProfile): string {
    return KIND_OPTIONS.find((k) => k.value === profile.settings.kind)?.label ?? '';
  }

  /** Which queues go through the profile, for the line under it. */
  function usedLine(profile: ProxyProfile): string {
    const names = app.proxies.queuesThrough(profile.id).map((q) => q.settings.name);
    if (names.length === 0) return 'No queue uses it.';
    return `Used by ${names.join(', ')}.`;
  }

  async function test(profile: ProxyProfile) {
    if (testing !== null) return;
    testing = profile.id;
    try {
      // The saved profile as it is: the core keeps the result as its last test.
      await app.proxies.test(profile.id, { settings: $state.snapshot(profile.settings) });
    } finally {
      testing = null;
    }
  }

  async function copy() {
    if (copying) return;
    copying = true;
    try {
      await app.proxies.copyFromSystem();
    } finally {
      copying = false;
    }
  }
</script>

<div class="flex w-full flex-col gap-3">
  {#if profiles.length === 0}
    <p class="text-small text-ink-muted">
      No proxies saved yet. Add the proxy servers you use, or copy the one your computer sets.
    </p>
  {:else}
    <ul
      class="flex flex-col divide-y divide-line rounded-md border border-line"
      aria-label="Saved proxies"
    >
      {#each profiles as profile (profile.id)}
        {const status = lastTestLine(profile.last_test, app.clock.now)}
        {const name = shownName(profile.settings)}
        <li class="flex flex-wrap items-center gap-x-3 gap-y-1 px-3 py-2.5">
          <div class="flex min-w-0 flex-1 flex-col gap-0.5">
            <div class="flex min-w-0 items-center gap-2">
              <span class="truncate text-ui font-medium text-ink">{name}</span>
              <Badge size="sm">{kindLabel(profile)}</Badge>
            </div>
            <span class="truncate text-small text-ink-muted">
              {#if profile.settings.name.trim() !== ''}{proxyAddress(profile.settings)} ·
              {/if}{profile.settings.user === ''
                ? 'No login'
                : `User ${profile.settings.user}${profile.has_password ? ', password saved' : ''}`}
            </span>
            <span class="flex flex-wrap items-center gap-x-2 text-small">
              <Badge size="sm" dot tone={status.tone}>{status.text}</Badge>
              <span class="text-ink-muted">{usedLine(profile)}</span>
            </span>
          </div>
          <div class="flex items-center gap-1">
            <Button
              size="sm"
              variant="secondary"
              loading={testing === profile.id}
              aria-label={`Test ${name}`}
              onclick={() => void test(profile)}>Test</Button
            >
            <IconButton
              icon={Pencil}
              label={`Edit ${name}`}
              onclick={() => app.openDialog({ kind: 'proxy', id: profile.id })}
            />
            <IconButton
              icon={Trash}
              label={`Delete ${name}`}
              onclick={() => app.openDialog({ kind: 'delete-proxy', id: profile.id })}
            />
          </div>
        </li>
      {/each}
    </ul>
  {/if}
  <div class="flex flex-wrap items-center justify-end gap-2">
    <Button size="sm" loading={copying} onclick={() => void copy()}>Copy from system</Button>
    <Button
      size="sm"
      variant="primary"
      disabled={profiles.length >= MAX_PROXY_PROFILES}
      onclick={() => app.openDialog({ kind: 'proxy', id: null })}>Add proxy</Button
    >
  </div>
</div>
