<script lang="ts">
  import Search from '@lucide/svelte/icons/search';
  import { onMount } from 'svelte';
  import { buildCommands, searchCommands, type Command, type CommandGroup } from '$lib/commands';
  import { getApp } from '$lib/state/app.svelte';
  import Icon from './Icon.svelte';

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
  // While searching, best matches come first; otherwise actions stay grouped.
  const groups = $derived.by(() => {
    if (query.trim()) return [{ name: 'Best matches', items: results }];
    // A throwaway grouping, rebuilt on every change; it does not need to be reactive.
    // eslint-disable-next-line svelte/prefer-svelte-reactivity
    const map = new Map<CommandGroup, Command[]>();
    for (const c of results) {
      const g = map.get(c.group) ?? [];
      g.push(c);
      map.set(c.group, g);
    }
    return [...map].map(([name, items]) => ({ name, items }));
  });
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

<!-- svelte-ignore a11y_click_events_have_key_events -->
<dialog
  bind:this={el}
  class="palette"
  aria-label="Find an action or setting"
  oncancel={(e) => {
    e.preventDefault();
    close();
  }}
  onclick={(e) => {
    if (e.target === el) close();
  }}
>
  <div class="search">
    <Icon icon={Search} size="md" />
    <input
      bind:this={input}
      bind:value={query}
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
      {onkeydown}
    />
  </div>
  <div bind:this={list} id="palette-list" class="list" role="listbox" aria-label="Actions">
    {#each groups as group (group.name)}
      <div role="group" aria-label={group.name}>
        <div class="group" aria-hidden="true">{group.name}</div>
        {#each group.items as c (c.id)}
          {@const i = flat.indexOf(c)}
          <!-- svelte-ignore a11y_click_events_have_key_events -->
          <div
            id="cmd-{i}"
            role="option"
            tabindex="-1"
            aria-selected={i === index}
            class="item"
            class:current={i === index}
            onclick={() => run(c)}
            onmousemove={() => (index = i)}
          >
            <span>{c.title}</span>
            {#if c.shortcut}<kbd>{c.shortcut}</kbd>{/if}
          </div>
        {/each}
      </div>
    {:else}
      <p class="none">No action or setting matches "{query}". Try fewer letters.</p>
    {/each}
  </div>
  <p class="foot">
    <kbd>Up</kbd> <kbd>Down</kbd> to move, <kbd>Enter</kbd> to run, <kbd>Esc</kbd> to close
  </p>
</dialog>

<style>
  .palette {
    width: min(600px, calc(100vw - 32px));
    margin-top: 12vh;
    padding: 0;
    border: 1px solid var(--line);
    border-radius: var(--radius-l);
    background: var(--surface);
    color: var(--ink);
    box-shadow: var(--shadow);
  }

  .palette::backdrop {
    background: var(--scrim);
  }

  .search {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 0 16px;
    border-bottom: 1px solid var(--line);
    color: var(--ink-3);
  }

  input {
    flex: 1;
    height: 50px;
    border: 0;
    background: transparent;
    color: var(--ink);
    font-size: var(--text-lg);
    outline: none;
  }

  input::placeholder {
    color: var(--ink-3);
  }

  .list {
    max-height: min(420px, 56vh);
    padding: 6px;
    overflow-y: auto;
  }

  .group {
    padding: 8px 10px 4px;
    color: var(--ink-2);
    font-size: var(--text-xs);
    font-weight: 600;
  }

  .item {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    min-height: 34px;
    padding: 6px 10px;
    border-radius: var(--radius-m);
    cursor: pointer;
  }

  .item.current {
    background: var(--accent-soft);
    box-shadow: inset 3px 0 0 var(--accent);
  }

  .none {
    margin: 0;
    padding: 18px 12px;
    color: var(--ink-2);
  }

  .foot {
    margin: 0;
    padding: 8px 14px;
    border-top: 1px solid var(--line);
    color: var(--ink-2);
    font-size: var(--text-xs);
  }
</style>
