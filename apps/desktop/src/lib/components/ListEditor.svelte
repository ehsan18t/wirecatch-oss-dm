<script lang="ts">
  import X from '@lucide/svelte/icons/x';

  let {
    items,
    label,
    placeholder,
    describedby,
    normalize = (s: string) => s.trim().toLowerCase(),
    onchange,
  }: {
    items: readonly string[];
    label: string;
    placeholder: string;
    describedby?: string;
    normalize?: (s: string) => string;
    onchange: (items: string[]) => void;
  } = $props();

  let draft = $state('');

  function add() {
    const parts = draft
      .split(/[\s,;]+/)
      .map(normalize)
      .filter((x) => x && !items.includes(x));
    if (parts.length > 0) onchange([...items, ...new Set(parts)]);
    draft = '';
  }
</script>

<div class="list-editor">
  {#if items.length > 0}
    <ul aria-label={label}>
      {#each items as item (item)}
        <li>
          <span>{item}</span>
          <button
            type="button"
            class="remove"
            aria-label="Remove {item}"
            onclick={() => onchange(items.filter((x) => x !== item))}
          >
            <X size={12} aria-hidden="true" />
          </button>
        </li>
      {/each}
    </ul>
  {/if}
  <div class="add">
    <input
      class="field"
      aria-label="Add to {label.toLowerCase()}"
      aria-describedby={describedby}
      {placeholder}
      bind:value={draft}
      onkeydown={(e) => {
        if (e.key === 'Enter') {
          e.preventDefault();
          add();
        }
      }}
    />
    <button type="button" class="btn small" onclick={add} disabled={!draft.trim()}>Add</button>
  </div>
</div>

<style>
  .list-editor {
    display: grid;
    gap: 8px;
    width: 100%;
  }

  ul {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
    margin: 0;
    padding: 0;
    list-style: none;
  }

  li {
    display: inline-flex;
    align-items: center;
    gap: 2px;
    height: 26px;
    padding: 0 2px 0 10px;
    border: 1px solid var(--line-strong);
    border-radius: 999px;
    background: var(--surface-2);
    font-size: var(--text-sm);
  }

  .remove {
    display: grid;
    place-items: center;
    width: 20px;
    height: 20px;
    padding: 0;
    border: 0;
    border-radius: 50%;
    background: transparent;
    color: var(--ink-2);
    cursor: pointer;
  }

  .remove:hover {
    background: var(--surface-3);
    color: var(--ink);
  }

  .add {
    display: flex;
    gap: 6px;
  }

  .add input {
    flex: 1;
  }
</style>
