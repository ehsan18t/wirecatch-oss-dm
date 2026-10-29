<script lang="ts">
  // The saved proxy servers (D73.1): each with its address and type, its last test and what uses it, and Test, Edit and Delete; under them New proxy and Copy from system.
  import Pencil from '~icons/lucide/pencil';
  import Trash from '~icons/lucide/trash-2';
  import { MAX_PROXY_SERVERS, type ProxyServer, type ProxyServerId } from '@wirecatch/protocol';
  import { Badge, Button, IconButton } from '@wirecatch/ui';
  import { KIND_OPTIONS, lastTestLine, proxyAddress, shownName } from '$lib/proxy';
  import { getApp } from '$lib/state/app.svelte';
  import { usersLine } from '$lib/state/proxy-servers.svelte';

  const app = getApp();
  const profiles = $derived(app.proxies.list);
  let testing = $state<ProxyServerId | null>(null);
  let copying = $state(false);

  function kindLabel(profile: ProxyServer): string {
    return KIND_OPTIONS.find((k) => k.value === profile.settings.kind)?.label ?? '';
  }

  /** The second line: its address when its name is not the address, and its login. */
  function detailLine(profile: ProxyServer): string {
    const { name, user } = profile.settings;
    let login = 'no authentication';
    if (user !== '') {
      login = `user ${user}, ${profile.has_password ? 'password saved' : 'no password saved'}`;
    }
    if (name.trim() !== '') return `${proxyAddress(profile.settings)} · ${login}`;
    return `${login.charAt(0).toUpperCase()}${login.slice(1)}`;
  }

  async function test(profile: ProxyServer) {
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
      No proxy servers yet. Add one, or copy your system proxy.
    </p>
  {:else}
    <ul
      class="flex flex-col divide-y divide-line rounded-md border border-line"
      aria-label="Proxy servers"
    >
      {#each profiles as profile (profile.id)}
        {const status = lastTestLine(profile.last_test, app.clock.now)}
        {const name = shownName(profile.settings)}
        <li class="flex flex-wrap items-center gap-x-3 gap-y-1 px-3 py-2.5">
          <div class="flex min-w-0 flex-1 flex-col gap-0.5">
            <div class="flex min-w-0 items-center gap-2">
              <span class="min-w-0 text-ui font-medium wrap-anywhere text-ink">{name}</span>
              <Badge size="sm">{kindLabel(profile)}</Badge>
            </div>
            <span class="text-small wrap-anywhere text-ink-muted">{detailLine(profile)}</span>
            <span class="flex flex-wrap items-center gap-x-2 text-small">
              <Badge size="sm" dot tone={status.tone}>{status.text}</Badge>
              <span class="text-ink-muted">{usersLine(app.proxies.usersOf(profile.id))}</span>
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
      disabled={profiles.length >= MAX_PROXY_SERVERS}
      onclick={() => app.openDialog({ kind: 'proxy', id: null })}>New proxy</Button
    >
  </div>
</div>
