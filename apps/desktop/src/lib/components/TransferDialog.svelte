<script lang="ts">
  import Check from '@lucide/svelte/icons/check';
  import X from '@lucide/svelte/icons/x';
  import { getApp } from '$lib/state/app.svelte';
  import Dialog from './Dialog.svelte';

  let { mode }: { mode: 'export' | 'import' } = $props();
  const app = getApp();

  let done = $state<string | null>(null);
  let busy = $state(false);

  const included = [
    'All settings, including the browser extension settings',
    'Queues with their schedules and speed limits',
    'Folders by file type',
    'Sites to leave to the browser',
    'Which add-ons are installed (they are installed again on import)',
  ];
  const excluded = ['Download history', 'Saved site passwords'];

  async function run() {
    busy = true;
    done = mode === 'export' ? await app.exportSettings() : await app.importSettings();
    busy = false;
  }
</script>

<Dialog
  title={mode === 'export' ? 'Export settings' : 'Import settings'}
  description={mode === 'export'
    ? 'Saves your setup to one file. Load it on another computer, Windows or Linux, with Import settings. Folders like Downloads are matched to that computer automatically.'
    : 'Loads a file made with Export settings. It replaces your current settings and queues; your downloads stay as they are.'}
  onclose={() => app.closeDialog()}
  width={520}
>
  {#if done}
    <p class="done" role="status">
      <Check size={16} aria-hidden="true" />
      <span>
        {mode === 'export' ? 'Settings saved to' : 'Settings loaded from'}
        <strong>{done}</strong>
      </span>
    </p>
  {:else}
    <div class="lists">
      <div>
        <h3>{mode === 'export' ? 'Saved in the file' : 'Replaced from the file'}</h3>
        <ul>
          {#each included as item (item)}
            <li><Check size={14} aria-hidden="true" /> {item}</li>
          {/each}
        </ul>
      </div>
      <div>
        <h3>Not included</h3>
        <ul>
          {#each excluded as item (item)}
            <li class="no"><X size={14} aria-hidden="true" /> {item}</li>
          {/each}
        </ul>
      </div>
    </div>
  {/if}

  {#snippet footer()}
    {#if done}
      <button class="btn primary" data-autofocus onclick={() => app.closeDialog()}>Done</button>
    {:else}
      <button class="btn" onclick={() => app.closeDialog()}>Cancel</button>
      <button class="btn primary" data-autofocus disabled={busy} onclick={run}>
        {mode === 'export' ? 'Choose where to save' : 'Choose a settings file'}
      </button>
    {/if}
  {/snippet}
</Dialog>

<style>
  .lists {
    display: grid;
    gap: 14px;
  }

  h3 {
    margin: 0 0 6px;
    font-size: var(--text-sm);
    font-weight: 600;
  }

  ul {
    display: grid;
    gap: 4px;
    margin: 0;
    padding: 0;
    list-style: none;
    color: var(--ink-2);
    font-size: var(--text-sm);
  }

  li {
    display: flex;
    align-items: center;
    gap: 8px;
  }

  li :global(svg) {
    flex: none;
    color: var(--accent-text);
  }

  li.no :global(svg) {
    color: var(--ink-3);
  }

  .done {
    display: flex;
    gap: 8px;
    margin: 0;
    line-height: 1.5;
    overflow-wrap: anywhere;
  }

  .done :global(svg) {
    flex: none;
    margin-top: 3px;
    color: var(--accent-text);
  }
</style>
