<script lang="ts">
  import { fileIconUrl } from "../lib/file-icon.svelte";
  import { formatBytes } from "../lib/format";
  import { theme } from "../state/theme.svelte";

  let { name, size, title = name }: { name: string; size: number; title?: string } = $props();
</script>

<div class="flex h-16 w-44 items-center gap-2 rounded-lg bg-muted px-3 text-xs" {title}>
  <img src={fileIconUrl(name, theme.dark)} alt="" class="size-6 shrink-0" />
  <div class="min-w-0">
    <p class="truncate">{name}</p>
    <p class="text-muted-foreground">{formatBytes(size)}</p>
  </div>
</div>
