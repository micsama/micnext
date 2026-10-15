<script lang="ts">
  import type { Occupancy } from "../api/types";
  import { formatTokens } from "../lib/format";

  let { occupancy: o }: { occupancy: Occupancy } = $props();

  const R = 8;
  const C = 2 * Math.PI * R;
  const ratio = $derived(Math.min(o.used_tokens / o.window, 1));
  const percent = $derived(Math.round((o.used_tokens / o.window) * 100));
  const near = $derived(percent >= o.trigger_percent);
</script>

<span
  class="inline-flex h-9 shrink-0 cursor-default items-center gap-1.5 px-1 text-xs font-medium tabular-nums"
  role="img"
  aria-label="上下文占用 {percent}%"
  title="上下文约 {formatTokens(o.used_tokens)} / {formatTokens(o.window)}（{percent}%）· 达到 {o.trigger_percent}% 自动压缩">
  <svg viewBox="0 0 20 20" class="size-5 -rotate-90">
    <circle cx="10" cy="10" r={R} fill="none" stroke-width="3" class="stroke-border" />
    <circle cx="10" cy="10" r={R} fill="none" stroke-width="3" stroke-linecap="round" stroke-dasharray={C} stroke-dashoffset={C * (1 - ratio)} class={near ? "stroke-warning" : "stroke-primary"} />
  </svg>
  <span class={near ? "text-warning" : "text-foreground/80"}>{percent}%</span>
</span>
