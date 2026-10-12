<script lang="ts">
  import type { ConnectionStatus } from '../lib/native/status';

  interface Props {
    status: ConnectionStatus;
  }

  let { status }: Props = $props();

  const statusLabel = $derived.by(() => {
    switch (status.state) {
      case 'connected':
        return 'Connected';
      case 'connecting':
        return 'Connecting';
      case 'unavailable':
        if (status.problem.kind === 'app_closed') return 'Closed';
        return status.retryAt === null ? 'Connecting' : 'Not connected';
    }
  });
</script>

<header class="header">
  <div class="brand">
    <svg class="logo" viewBox="0 0 24 24" aria-hidden="true" focusable="false">
      <rect x="1" y="1" width="22" height="22" rx="6" />
      <path d="M12 6v9m-4-4 4 4 4-4M7 18h10" />
    </svg>
    <h1>Wirecatch</h1>
  </div>
  <p class="status status-{status.state}" aria-live="polite">
    <span class="dot" aria-hidden="true"></span>
    <span>{statusLabel}</span>
  </p>
</header>

<style>
  .header {
    flex: none;
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 12px 16px;
    border-bottom: 1px solid var(--border);
  }

  .brand {
    display: flex;
    align-items: center;
    gap: 8px;
  }

  .logo {
    width: 22px;
    height: 22px;
  }

  .logo rect {
    fill: var(--accent);
  }

  .logo path {
    fill: none;
    stroke: var(--on-accent);
    stroke-width: 2;
    stroke-linecap: round;
    stroke-linejoin: round;
  }

  h1 {
    margin: 0;
    font-size: 15px;
    font-weight: 650;
  }

  .status {
    display: flex;
    align-items: center;
    gap: 6px;
    margin: 0;
    color: var(--muted);
    font-size: 12px;
  }

  .dot {
    width: 8px;
    height: 8px;
    border-radius: 50%;
    background: var(--muted);
  }

  .status-connected .dot {
    background: var(--ok);
  }

  .status-unavailable .dot {
    background: var(--warn);
  }
</style>
