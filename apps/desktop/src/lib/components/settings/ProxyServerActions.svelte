<script lang="ts">
  // Copy from system and New proxy, in the Proxy servers setting's name line (D73.1).
  import { MAX_PROXY_SERVERS } from '@wirecatch/protocol';
  import { Button } from '@wirecatch/ui';
  import { getApp } from '$lib/state/app.svelte';

  const app = getApp();
  let copying = $state(false);

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

<Button size="sm" loading={copying} onclick={() => void copy()}>Copy from system</Button>
<Button
  size="sm"
  variant="primary"
  disabled={app.proxies.list.length >= MAX_PROXY_SERVERS}
  onclick={() => app.openDialog({ kind: 'proxy', id: null })}>New proxy</Button
>
