<script lang="ts">
  import ClipboardCopy from '~icons/lucide/clipboard-copy';
  import Settings from '~icons/lucide/settings';
  import { PRODUCT_NAME } from '@wirecatch/product';
  import { AppMark, IconButton } from '@wirecatch/ui';
  import type { ConnectionStatus } from '../../lib/native/status';
  import ConnectionBadge from '../common/ConnectionBadge.svelte';

  interface Props {
    status: ConnectionStatus;
    onsettings: () => void;
    /** Copies the diagnostics, for a support request; null when the popup offers none. */
    oncopy?: (() => void) | null;
  }

  let { status, onsettings, oncopy = null }: Props = $props();
</script>

<!-- A status too long to sit beside the name (a translation, a wider font) moves under it, with the settings button. -->
<header class="flex flex-wrap items-center gap-x-2 gap-y-1.5 px-4 pt-3.5 pb-3">
  <AppMark size="lg" />
  <h1 class="font-display text-subtitle font-semibold text-ink">{PRODUCT_NAME}</h1>
  <div class="ml-auto flex items-center gap-1.5">
    <ConnectionBadge {status} />
    {#if oncopy}
      <IconButton icon={ClipboardCopy} label="Copy diagnostics" size="sm" onclick={oncopy} />
    {/if}
    <IconButton icon={Settings} label="Open Wirecatch settings" size="sm" onclick={onsettings} />
  </div>
</header>
