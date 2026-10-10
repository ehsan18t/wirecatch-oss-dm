<script lang="ts">
  // Renders the whole interface around a given AppState, for component tests.
  import Shell from '$lib/components/Shell.svelte';
  import { setApp, type AppState } from '$lib/state/app.svelte';

  let { app }: { app: AppState } = $props();
  // The state object is fixed for the life of the test.
  const fixed = (() => app)();
  setApp(fixed);
</script>

<Shell />
