<script lang="ts">
  import { SHORTCUTS } from '$lib/commands';
  import { getApp } from '$lib/state/app.svelte';
  import Button from './Button.svelte';
  import Dialog from './Dialog.svelte';

  const app = getApp();
</script>

<Dialog title="Keyboard shortcuts" onclose={() => app.closeDialog()} width={480}>
  <table>
    <caption class="visually-hidden">Keyboard shortcuts</caption>
    <tbody>
      {#each SHORTCUTS as s (s.keys)}
        <tr>
          <th scope="row">
            {#each s.keys.split(', ') as combo, i (combo)}
              {#if i > 0}<span class="or">or</span>{/if}
              {#each combo.split('+') as key, j (j)}
                {#if j > 0}<span class="plus">+</span>{/if}<kbd>{key}</kbd>
              {/each}
            {/each}
          </th>
          <td>{s.what}</td>
        </tr>
      {/each}
    </tbody>
  </table>
  {#snippet footer()}
    <Button variant="primary" data-autofocus onclick={() => app.closeDialog()}>Done</Button>
  {/snippet}
</Dialog>

<style>
  table {
    width: 100%;
    border-collapse: collapse;
    font-size: var(--text-sm);
  }

  th {
    padding: 5px 12px 5px 0;
    font-weight: 400;
    text-align: left;
    white-space: nowrap;
  }

  td {
    padding: 5px 0;
    color: var(--ink-2);
  }

  .plus,
  .or {
    margin: 0 3px;
    color: var(--ink-3);
  }
</style>
