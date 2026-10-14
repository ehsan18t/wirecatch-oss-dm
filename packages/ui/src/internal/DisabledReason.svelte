<script lang="ts">
  // Why a control is disabled, for screen readers: the control stays focusable (aria-disabled) and points here with aria-describedby (WCAG 4.1.2).
  interface Props {
    id: string;
    reason: string | null;
  }

  let { id, reason }: Props = $props();
</script>

{#if reason}
  <span {id} class="sr-only">{reason}</span>
{/if}
