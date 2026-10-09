<script lang="ts">
  import type { Snippet } from "svelte";

  let {
    title,
    footer,
    actions,
    children,
  }: { title?: string; footer?: string; actions?: Snippet; children: Snippet } = $props();
</script>

<section class="space-y-2">
  {#if title || actions}
    <div class="flex min-h-7 items-end justify-between gap-2 px-1">
      <h3 class="text-xs font-medium text-muted-foreground">{title}</h3>
      {@render actions?.()}
    </div>
  {/if}
  <div class="divide-y divide-border overflow-hidden rounded-xl border bg-card">
    {@render children()}
  </div>
  {#if footer}
    <p class="px-1 text-xs text-muted-foreground">{footer}</p>
  {/if}
</section>
