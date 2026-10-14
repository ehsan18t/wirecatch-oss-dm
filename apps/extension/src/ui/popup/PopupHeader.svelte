<script lang="ts">
  import { PRODUCT_NAME } from '@wirecatch/product';
  import { AppMark, IconButton } from '@wirecatch/ui';
  import Settings from '~icons/lucide/settings';
  import type { ConnectionStatus } from '../../lib/native/status';
  import ConnectionBadge from '../common/ConnectionBadge.svelte';

  interface Props {
    status: ConnectionStatus;
    onsettings: () => void;
  }

  let { status, onsettings }: Props = $props();
</script>

<!-- A status too long to sit beside the name (a translation, a wider font) moves under it, with the settings button. -->
<header class="flex flex-wrap items-center gap-x-2 gap-y-1.5 px-4 pt-3.5 pb-3">
  <AppMark size="lg" />
  <h1 class="font-display text-subtitle font-semibold text-ink">{PRODUCT_NAME}</h1>
  <div class="ml-auto flex items-center gap-1.5">
    <ConnectionBadge {status} />
    <IconButton icon={Settings} label="Settings" size="sm" onclick={onsettings} />
  </div>
</header>
