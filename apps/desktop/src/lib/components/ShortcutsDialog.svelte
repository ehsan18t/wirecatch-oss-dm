<script lang="ts">
  import { shortcutHelp } from '$lib/keymap';
  import { getApp } from '$lib/state/app.svelte';
  import Button from './Button.svelte';
  import Dialog from './Dialog.svelte';

  const app = getApp();
  const lines = shortcutHelp();
</script>

<Dialog title="Keyboard shortcuts" onclose={() => app.closeDialog()}>
  <table>
    <caption class="visually-hidden">Keyboard shortcuts</caption>
    <tbody>
      {#each lines as line (line.what)}
        <tr>
          <th scope="row">
            {#each line.keys as combo, i (combo.join('+'))}
              {#if i > 0}<span class="or">or</span>{/if}
              {#each combo as key, j (j)}
                {#if j > 0}<span class="plus">+</span>{/if}<kbd>{key}</kbd>
              {/each}
            {/each}
          </th>
          <td>{line.what}</td>
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
