<script lang="ts">
  // The command palette: every action and setting, found by typing; results stay grouped by kind, best group first.
  import Search from '~icons/lucide/search';
  import { onMount } from 'svelte';
  import { buildCommands, groupCommands, searchCommands, type Command } from '$lib/commands';
  import { getApp } from '$lib/state/app.svelte';
  import { cn, DIALOG_SURFACE, Icon, Kbd } from '@wirecatch/ui';

  const app = getApp();

  // The list of actions as of when the palette opened (it depends on the selection).
  const commands = buildCommands(app);

  let query = $state('');
  let index = $state(0);
  let el = $state<HTMLDialogElement>();
  let input = $state<HTMLInputElement>();
  let list = $state<HTMLDivElement>();
  let previous: Element | null = null;

  const results = $derived(searchCommands(commands, query));
  // Grouped by kind while searching too: the group of the best match first.
  const groups = $derived(groupCommands(results));
  const flat = $derived(groups.flatMap((g) => g.items));

  onMount(() => {
    previous = document.activeElement;
    if (el && typeof el.showModal === 'function') el.showModal();
    else el?.setAttribute('open', '');
    input?.focus();
  });

  function close(restoreFocus = true) {
    app.paletteOpen = false;
    if (restoreFocus && previous instanceof HTMLElement && previous.isConnected) previous.focus();
  }

  function run(c: Command | undefined) {
    if (!c) return;
    close(false);
    // Let the palette leave the page before the action opens anything new.
    queueMicrotask(() => {
      void c.run();
    });
  }

  function onkeydown(e: KeyboardEvent) {
    if (e.key === 'ArrowDown') {
      e.preventDefault();
      index = Math.min(flat.length - 1, index + 1);
    } else if (e.key === 'ArrowUp') {
      e.preventDefault();
      index = Math.max(0, index - 1);
    } else if (e.key === 'Home' && e.ctrlKey) {
      index = 0;
    } else if (e.key === 'End' && e.ctrlKey) {
      index = flat.length - 1;
    } else if (e.key === 'Escape') {
      e.preventDefault();
      e.stopPropagation();
      close();
    } else if (e.key === 'Enter') {
      e.preventDefault();
      run(flat[index]);
    }
  }

  $effect(() => {
    const active = list?.querySelector(`#cmd-${index}`);
    active?.scrollIntoView({ block: 'nearest' });
  });
</script>

<!-- The keys work wherever the focus is in the palette, so they are handled on the dialog. -->
<dialog
  bind:this={el}
  class={cn(
    DIALOG_SURFACE,
    'mx-auto mt-24 w-full max-w-dialog-lg overflow-hidden p-0 backdrop:bg-scrim backdrop:backdrop-blur-scrim',
  )}
  aria-label="Find an action or setting"
  oncancel={(e) => {
    e.preventDefault();
    close();
  }}
  onclick={(e) => {
    if (e.target === el) close();
  }}
  {onkeydown}
>
  <div class="flex items-center gap-2.5 border-b border-line px-4 text-ink-subtle">
    <Icon icon={Search} size="md" />
    <input
      bind:this={input}
      bind:value={query}
      class="h-12 min-w-0 flex-1 bg-transparent text-body text-ink outline-none placeholder:text-ink-subtle"
      role="combobox"
      aria-expanded="true"
      aria-controls="palette-list"
      aria-autocomplete="list"
      aria-activedescendant={flat.length > 0 ? `cmd-${index}` : undefined}
      aria-label="Type an action or setting"
      placeholder="Type an action or setting, like pause, queue or dark"
      spellcheck="false"
      autocomplete="off"
      oninput={() => (index = 0)}
    />
  </div>
  <div
    bind:this={list}
    id="palette-list"
    class="max-h-96 overflow-y-auto p-1.5"
    role="listbox"
    aria-label="Actions"
  >
    {#each groups as group (group.name)}
      <div role="group" aria-label={group.name}>
        <div class="px-2.5 pt-2 pb-1 text-small font-semibold text-ink-muted" aria-hidden="true">
          {group.name}
        </div>
        {#each group.items as c (c.id)}
          {@const i = flat.indexOf(c)}
          <!-- The search field keeps the focus and moves through the options (aria-activedescendant); each option is a button, so a click runs it and it is a control in its own right. -->
          <button
            type="button"
            id="cmd-{i}"
            role="option"
            tabindex="-1"
            aria-selected={i === index}
            class={cn(
              'flex min-h-9 w-full items-center justify-between gap-3 rounded-sm px-2.5 text-left text-ui',
              c.danger ? 'text-danger' : 'text-ink',
              i === index && 'bg-accent-soft',
            )}
            onclick={() => run(c)}
            onmousemove={() => (index = i)}
          >
            <span class="truncate">{c.title}</span>
            {#if c.shortcut}
              <span class="shrink-0 text-small text-ink-muted">{c.shortcut}</span>
            {/if}
          </button>
        {/each}
      </div>
    {:else}
      <p class="px-3 py-5 text-ui text-ink-muted">
        No action or setting matches “{query}”. Try fewer letters.
      </p>
    {/each}
  </div>
  <p class="flex items-center gap-4 border-t border-line px-4 py-2 text-small text-ink-muted">
    <span class="flex items-center gap-1.5"><Kbd keys={['Up']} /><Kbd keys={['Down']} /> move</span>
    <span class="flex items-center gap-1.5"><Kbd keys={['Enter']} /> run</span>
    <span class="flex items-center gap-1.5"><Kbd keys={['Esc']} /> close</span>
  </p>
</dialog>
