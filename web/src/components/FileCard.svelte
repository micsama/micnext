<script lang="ts">
  import { formatBytes } from "../lib/format";
  import Icon from "../lib/Icon.svelte";

  let { name, size, title = name }: { name: string; size: number; title?: string } = $props();
</script>

<div class="flex h-16 w-44 items-center gap-2 rounded-lg bg-muted px-3 text-xs" {title}>
  <Icon name="file" class="size-5 shrink-0 text-muted-foreground" />
  <div class="min-w-0">
    <p class="truncate">{name}</p>
    <p class="text-muted-foreground">{formatBytes(size)}</p>
  </div>
</div>
