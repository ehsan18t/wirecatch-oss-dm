<script lang="ts">
  import Download from '~icons/lucide/download';
  import Pause from '~icons/lucide/pause';
  import Pencil from '~icons/lucide/pencil';
  import IconButton from '../primitives/IconButton.svelte';
  import Sidebar from '../shell/Sidebar.svelte';
  import SidebarItem from '../shell/SidebarItem.svelte';
  import SidebarSection from '../shell/SidebarSection.svelte';
  import type { SidebarState } from '../shell/sidebar-state.svelte';

  let { sidebar, onedit }: { sidebar: SidebarState; onedit?: () => void } = $props();
  let current = $state('active');
  let queuesOpen = $state(true);
</script>

<button type="button" onclick={() => sidebar.toggle()}>Navigation</button>
<Sidebar {sidebar} label="Views">
  <SidebarSection title="Downloads">
    <SidebarItem
      label="Active"
      icon={Download}
      active={current === 'active'}
      count={3}
      onclick={() => (current = 'active')}
    />
    <SidebarItem
      label="Paused"
      icon={Pause}
      active={current === 'paused'}
      onclick={() => (current = 'paused')}
    />
  </SidebarSection>
  <SidebarSection title="Queues" collapsible bind:expanded={queuesOpen}>
    <SidebarItem
      label="Overnight"
      icon={Download}
      description="Starts at 22:00"
      active={current === 'overnight'}
      onclick={() => (current = 'overnight')}
    >
      {#snippet actions()}
        <IconButton icon={Pencil} label="Edit Overnight" size="xs" onclick={() => onedit?.()} />
      {/snippet}
    </SidebarItem>
  </SidebarSection>
</Sidebar>
