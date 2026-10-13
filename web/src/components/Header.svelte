<script lang="ts">
  import Icon from "../lib/Icon.svelte";
  import type { Snippet } from "svelte";

  let { title, detail, onmenu, actions }: { title: string; detail?: string; onmenu: () => void; actions?: Snippet } = $props();
</script>

<header class="flex h-12 shrink-0 items-center gap-2 border-b border-border px-3">
  <button type="button" class="rounded p-1.5 hover:bg-accent md:hidden" aria-label="打开会话列表" onclick={onmenu}>
    <Icon name="menu" />
  </button>
  <div class="min-w-0 flex-1">
    <div class="flex items-center gap-1.5"><span class="truncate text-sm font-medium">{title}</span>{@render actions?.()}</div>
    {#if detail}
      <div class="truncate font-mono text-xs text-muted-foreground">{detail}</div>
    {/if}
  </div>
</header>
