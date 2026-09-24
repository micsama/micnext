<script lang="ts">
  import Icon from "../lib/Icon.svelte";

  let { title, detail, onmenu }: { title: string; detail?: string; onmenu: () => void } = $props();
</script>

<header class="flex h-12 shrink-0 items-center gap-2 border-b border-line px-3">
  <button type="button" class="rounded p-1.5 hover:bg-raised md:hidden" aria-label="打开会话列表" onclick={onmenu}>
    <Icon name="menu" />
  </button>
  <div class="min-w-0 flex-1">
    <div class="truncate text-sm font-medium">{title}</div>
    {#if detail}
      <div class="truncate font-mono text-xs text-muted">{detail}</div>
    {/if}
  </div>
</header>
