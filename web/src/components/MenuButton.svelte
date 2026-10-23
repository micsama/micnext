<script lang="ts">
  import Icon from "../lib/Icon.svelte";
  import { sidebar } from "../state/sidebar.svelte";
  import { sessionsSync } from "../state/sessions.svelte";
</script>

<button type="button" title="展开侧栏" aria-label="展开侧栏" class={["relative rounded p-1.5 text-muted-foreground hover:bg-accent hover:text-foreground", sidebar.docked && "md:hidden"]} onclick={() => sidebar.toggle()}>
  <Icon name="sidebar" />
  {#if sessionsSync.unread}<span class="absolute top-1 right-1 size-2 rounded-full bg-primary" title="有新回复"></span>{/if}
</button>
