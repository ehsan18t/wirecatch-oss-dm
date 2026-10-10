<script lang="ts">
  import { onMount } from 'svelte';
  import { createBackend } from '$lib/backend';
  import Shell from '$lib/components/Shell.svelte';
  import { AppState, setApp } from '$lib/state/app.svelte';

  // The backend loads asynchronously so the mock never ships in a production build.
  const app = new AppState(createBackend);
  setApp(app);

  onMount(() => {
    void app.init();
    return () => app.dispose();
  });
</script>

<Shell />
