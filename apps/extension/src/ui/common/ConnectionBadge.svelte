<script lang="ts">
  // The connection to the app at a glance; a change is read out without moving focus.
  import { Badge } from '@wirecatch/ui';
  import type { ConnectionStatus } from '../../lib/native/status';
  import { connectionBadge } from '../connection';

  interface Props {
    status: ConnectionStatus;
  }

  let { status }: Props = $props();

  const badge = $derived(connectionBadge(status));
</script>

<p aria-live="polite">
  <Badge tone={badge.tone} dot>{badge.label}</Badge>
</p>
