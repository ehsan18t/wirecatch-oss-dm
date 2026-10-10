<script lang="ts">
  let {
    checked,
    label,
    describedby,
    onchange,
  }: {
    checked: boolean;
    label: string;
    describedby?: string;
    onchange: (value: boolean) => void;
  } = $props();
</script>

<button
  type="button"
  role="switch"
  class="toggle"
  aria-checked={checked}
  aria-label={label}
  aria-describedby={describedby}
  onclick={() => onchange(!checked)}
>
  <span class="track"><span class="knob"></span></span>
  <span class="state" aria-hidden="true">{checked ? 'On' : 'Off'}</span>
</button>

<style>
  .toggle {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    padding: 2px;
    border: 0;
    border-radius: 999px;
    background: transparent;
    cursor: pointer;
  }

  .track {
    position: relative;
    width: 34px;
    height: 20px;
    border: 1px solid var(--line-strong);
    border-radius: 999px;
    background: var(--surface-3);
    transition: background 120ms var(--ease);
  }

  .knob {
    position: absolute;
    top: 2px;
    left: 2px;
    width: 14px;
    height: 14px;
    border-radius: 50%;
    background: var(--ink-2);
    transition: transform 120ms var(--ease);
  }

  [aria-checked='true'] .track {
    border-color: var(--accent);
    background: var(--accent);
  }

  [aria-checked='true'] .knob {
    background: var(--accent-ink);
    transform: translateX(14px);
  }

  .state {
    width: 22px;
    color: var(--ink-2);
    font-size: var(--text-sm);
    text-align: left;
  }
</style>
