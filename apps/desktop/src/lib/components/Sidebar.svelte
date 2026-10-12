<script lang="ts">
  import Command from '@lucide/svelte/icons/command';
  import Keyboard from '@lucide/svelte/icons/keyboard';
  import Bell from '@lucide/svelte/icons/bell';
  import Plus from '@lucide/svelte/icons/plus';
  import SettingsIcon from '@lucide/svelte/icons/settings';
  import { CATEGORIES, CATEGORY_LABEL } from '$lib/format';
  import { shortcutParts } from '$lib/keymap';
  import { getApp } from '$lib/state/app.svelte';
  import { sameView, STATUS_GROUP_LABEL, STATUS_GROUPS, type View } from '$lib/state/view';
  import Button from './Button.svelte';
  import CategoryIcon from './CategoryIcon.svelte';
  import Icon from './Icon.svelte';
  import QueueList from './QueueList.svelte';
  import SidebarReadout from './SidebarReadout.svelte';

  const app = getApp();

  function current(view: View): boolean {
    return app.screen === 'downloads' && sameView(app.view, view);
  }
</script>

<nav class="sidebar" aria-label="Views and queues">
  <SidebarReadout />

  {#if app.questions.waitingCount > 0}
    <!-- Each waiting download has its own window (D23); this brings them all back. -->
    <button
      type="button"
      class="pending"
      title="Show the windows of the downloads that wait for your answer"
      onclick={() => app.showQuestionWindows()}
    >
      <Icon icon={Bell} size="sm" />
      <span class="waiting-label">Waiting for you</span>
      <span
        class="count num"
        aria-label="{app.questions.waitingCount} {app.questions.waitingCount === 1
          ? 'download'
          : 'downloads'}">{app.questions.waitingCount}</span
      >
    </button>
  {/if}

  <button type="button" class="palette" onclick={() => (app.paletteOpen = true)}>
    <Icon icon={Command} size="sm" />
    <span>Find any action</span>
    <kbd>{shortcutParts('palette').join(' ')}</kbd>
  </button>

  <div class="scroll">
    <section aria-labelledby="h-downloads">
      <h2 id="h-downloads" class="visually-hidden">Downloads by status</h2>
      <ul>
        <li>
          <button
            type="button"
            class="item"
            aria-current={current({ kind: 'all' }) ? 'page' : undefined}
            onclick={() => app.show({ kind: 'all' })}
          >
            <span class="label">All downloads</span>
            <span class="count num">{app.counts.all}</span>
          </button>
        </li>
        {#each STATUS_GROUPS as group (group)}
          {@const n = app.counts.status[group]}
          {#if group !== 'cancelled' || n > 0 || current({ kind: 'status', group })}
            <li>
              <button
                type="button"
                class="item"
                class:alert={group === 'failed' && n > 0}
                aria-current={current({ kind: 'status', group }) ? 'page' : undefined}
                onclick={() => app.show({ kind: 'status', group })}
              >
                <span class="dot {group}" aria-hidden="true"></span>
                <span class="label">{STATUS_GROUP_LABEL[group]}</span>
                {#if n > 0}<span class="count num">{n}</span>{/if}
              </button>
            </li>
          {/if}
        {/each}
      </ul>
    </section>

    <section aria-labelledby="h-queues">
      <div class="section-head">
        <h2 id="h-queues">Queues</h2>
        <Button
          variant="ghost"
          small
          icon={Plus}
          onclick={() => app.openDialog({ kind: 'queue', id: null })}
        >
          New queue
        </Button>
      </div>
      <QueueList />
    </section>

    <section aria-labelledby="h-types">
      <div class="section-head"><h2 id="h-types">File types</h2></div>
      <ul>
        {#each CATEGORIES as category (category)}
          {@const n = app.counts.category[category]}
          <li>
            <button
              type="button"
              class="item"
              class:empty={n === 0}
              aria-current={current({ kind: 'category', category }) ? 'page' : undefined}
              onclick={() => app.show({ kind: 'category', category })}
            >
              <CategoryIcon {category} size="sm" />
              <span class="label">{CATEGORY_LABEL[category]}</span>
              {#if n > 0}<span class="count num">{n}</span>{/if}
            </button>
          </li>
        {/each}
      </ul>
    </section>
  </div>

  <footer class="foot">
    <button
      type="button"
      class="item"
      aria-current={app.screen === 'settings' ? 'page' : undefined}
      onclick={() => app.openSettings()}
    >
      <Icon icon={SettingsIcon} size="md" />
      <span class="label">Settings</span>
      <kbd>{shortcutParts('settings').join(' ')}</kbd>
    </button>
    <button type="button" class="item" onclick={() => app.openDialog({ kind: 'shortcuts' })}>
      <Icon icon={Keyboard} size="md" />
      <span class="label">Keyboard shortcuts</span>
      <kbd>{shortcutParts('help').join(' ')}</kbd>
    </button>
    {#if app.sampleData}
      <p class="sample">Sample data. Nothing is really downloading.</p>
    {/if}
  </footer>
</nav>

<style>
  .sidebar {
    display: flex;
    flex-direction: column;
    min-height: 0;
    background: var(--bg);
    color: var(--ink-2);
  }

  p {
    margin: 0;
  }

  .palette {
    display: flex;
    align-items: center;
    gap: 8px;
    margin: 0 10px 8px;
    height: 32px;
    padding: 0 8px 0 10px;
    border: 1px solid var(--line-strong);
    border-radius: var(--radius-m);
    background: var(--surface);
    color: var(--ink-2);
    font-size: var(--text-sm);
    cursor: pointer;
    text-align: left;
  }

  .palette span {
    flex: 1;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .palette:hover {
    color: var(--ink);
  }

  .scroll {
    flex: 1;
    overflow-y: auto;
    padding: 4px 8px 12px;
  }

  section + section {
    margin-top: 14px;
  }

  ul {
    margin: 0;
    padding: 0;
    list-style: none;
  }

  .section-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 0 0 4px 8px;
  }

  h2 {
    margin: 0;
    color: var(--ink-2);
    font-size: var(--text-sm);
    font-weight: 650;
  }

  .item {
    display: flex;
    align-items: center;
    gap: 9px;
    width: 100%;
    height: 30px;
    padding: 0 8px;
    border: 0;
    border-radius: var(--radius-m);
    background: transparent;
    color: var(--ink-2);
    font-size: var(--text-md);
    text-align: left;
    cursor: pointer;
  }

  .item:hover {
    background: var(--surface-3);
    color: var(--ink);
  }

  .item[aria-current='page'] {
    background: var(--surface);
    color: var(--ink);
    box-shadow: inset 3px 0 0 var(--accent);
  }

  .item.empty {
    color: var(--ink-3);
  }

  .item.alert .label,
  .item.alert .count {
    color: var(--danger);
    font-weight: 600;
  }

  .label {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .count {
    color: var(--ink-3);
    font-size: var(--text-sm);
  }

  .dot {
    width: 8px;
    height: 8px;
    margin: 0 3px;
    border-radius: 50%;
    background: var(--wire-done);
  }

  .dot.active {
    background: var(--accent);
  }

  .dot.paused {
    background: var(--warn-wire);
  }

  .dot.failed {
    background: var(--danger);
  }

  .dot.waiting {
    background: transparent;
    border: 1.5px solid var(--ink-3);
  }

  .pending {
    display: flex;
    align-items: center;
    gap: 8px;
    margin: 0 10px 8px;
    padding: 7px 10px;
    border: 1px solid var(--accent);
    border-radius: var(--radius-m);
    background: var(--accent-soft);
    color: var(--ink);
    font-size: var(--text-sm);
    font-weight: 600;
    text-align: left;
    cursor: pointer;
  }

  .waiting-label {
    flex: 1;
  }

  .count {
    min-width: 20px;
    padding: 0 6px;
    border-radius: 999px;
    background: var(--accent);
    color: var(--accent-ink);
    font-size: var(--text-xs);
    line-height: 18px;
    text-align: center;
  }

  .pending :global(svg) {
    flex: none;
    color: var(--accent-text);
  }

  .foot {
    padding: 8px;
    border-top: 1px solid var(--line);
  }

  .sample {
    margin: 6px 8px 2px;
    color: var(--ink-3);
    font-size: var(--text-xs);
  }
</style>
