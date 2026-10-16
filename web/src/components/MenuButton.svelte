<script lang="ts">
  import Icon from "../lib/Icon.svelte";
  import { sidebar } from "../state/sidebar.svelte";
</script>

<button type="button" title="展开侧栏" aria-label="展开侧栏" class={["rounded p-1.5 text-muted-foreground hover:bg-accent hover:text-foreground", sidebar.docked && "md:hidden"]} onclick={() => sidebar.toggle()}>
  <Icon name="sidebar" />
</button>
