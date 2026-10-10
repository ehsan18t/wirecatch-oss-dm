<script lang="ts">
  interface Props {
    id: string;
    label: string;
    description: string;
    checked: boolean;
    disabled?: boolean;
    onchange: (checked: boolean) => void;
  }

  let { id, label, description, checked, disabled = false, onchange }: Props = $props();
</script>

<div class="row">
  <div class="text">
    <label for={id}>{label}</label>
    <p id="{id}-help" class="help">{description}</p>
  </div>
  <input
    {id}
    type="checkbox"
    role="switch"
    class="switch"
    {checked}
    {disabled}
    aria-describedby="{id}-help"
    onchange={(event) => onchange(event.currentTarget.checked)}
  />
</div>

<style>
  .row {
    display: flex;
    align-items: flex-start;
    justify-content: space-between;
    gap: 12px;
    padding: 8px 0;
  }

  .text {
    min-width: 0;
  }

  label {
    display: block;
    font-weight: 600;
  }

  .switch {
    appearance: none;
    flex: none;
    position: relative;
    width: 36px;
    height: 20px;
    margin: 2px 0 0;
    border-radius: 999px;
    background: var(--switch-off);
    cursor: pointer;
    transition: background-color 120ms ease;
  }

  .switch::after {
    content: '';
    position: absolute;
    top: 2px;
    left: 2px;
    width: 16px;
    height: 16px;
    border-radius: 50%;
    background: var(--switch-knob);
    box-shadow: 0 1px 2px rgb(0 0 0 / 0.25);
    transition: transform 120ms ease;
  }

  .switch:checked {
    background: var(--accent);
  }

  .switch:checked::after {
    transform: translateX(16px);
  }

  .switch:disabled {
    cursor: not-allowed;
    opacity: 0.5;
  }

  @media (forced-colors: active) {
    .switch {
      border: 1px solid CanvasText;
    }

    .switch::after {
      background: CanvasText;
    }

    .switch:checked {
      background: Highlight;
    }
  }

  @media (prefers-reduced-motion: reduce) {
    .switch,
    .switch::after {
      transition: none;
    }
  }
</style>
