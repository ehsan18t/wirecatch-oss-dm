<script lang="ts">
  // Renders the whole interface around a given AppState, for component tests.
  import MainWindow from '../../ui/MainWindow.svelte';
  import { setApp, type AppState } from '$lib/state/app.svelte';

  let { app }: { app: AppState } = $props();
  // The state object is fixed for the life of the test.
  const fixed = (() => app)();
  setApp(fixed);
</script>

<MainWindow />
