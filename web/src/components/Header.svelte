<script lang="ts">
  import MenuButton from "./MenuButton.svelte";
  import type { Snippet } from "svelte";

  let { title, detail, actions, detailActions }: { title: string; detail?: string; actions?: Snippet; detailActions?: Snippet } = $props();
</script>

<header class="flex h-12 shrink-0 items-center gap-2 border-b border-border px-3">
  <MenuButton />
  <div class="min-w-0 flex-1">
    <div class="flex items-center gap-1.5"><span class="truncate text-sm font-medium">{title}</span>{@render actions?.()}</div>
    {#if detail}
      <div class="flex min-w-0 items-center gap-2 text-xs text-muted-foreground"><span class="truncate font-mono">{detail}</span>{@render detailActions?.()}</div>
    {/if}
  </div>
</header>
