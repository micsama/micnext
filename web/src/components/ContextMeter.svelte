<script lang="ts">
  import { Popover } from "bits-ui";
  import type { Occupancy } from "../api/types";
  import { formatTokens } from "../lib/format";
  import ContextBreakdown from "./ContextBreakdown.svelte";

  let { session, occupancy: o }: { session: number; occupancy: Occupancy } = $props();

  let open = $state(false);
  const R = 8;
  const C = 2 * Math.PI * R;
  const ratio = $derived(Math.min(o.used_tokens / o.window, 1));
  const percent = $derived(Math.round((o.used_tokens / o.window) * 100));
  const near = $derived(percent >= o.trigger_percent);
</script>

<Popover.Root bind:open>
  <Popover.Trigger
    aria-label="上下文占用 {percent}%，查看构成"
    title="上下文约 {formatTokens(o.used_tokens)} / {formatTokens(o.window)}（{percent}%）· 达到 {o.trigger_percent}% 自动压缩"
    class="inline-flex h-9 shrink-0 items-center gap-1.5 rounded-full px-2 text-xs font-medium tabular-nums transition-colors hover:bg-muted focus-visible:outline-2 focus-visible:outline-ring data-[state=open]:bg-muted">
    <svg viewBox="0 0 20 20" class="size-5 -rotate-90">
      <circle cx="10" cy="10" r={R} fill="none" stroke-width="3" class="stroke-border" />
      <circle cx="10" cy="10" r={R} fill="none" stroke-width="3" stroke-linecap="round" stroke-dasharray={C} stroke-dashoffset={C * (1 - ratio)} class={near ? "stroke-warning" : "stroke-primary"} />
    </svg>
    <span class={near ? "text-warning" : "text-foreground/80"}>{percent}%</span>
  </Popover.Trigger>
  <Popover.Portal>
    <Popover.Content
      side="top"
      align="end"
      sideOffset={8}
      collisionPadding={16}
      aria-label="上下文构成"
      class="picker-panel z-50 w-[26rem] max-w-[calc(100vw-2rem)] rounded-2xl border border-border/70 bg-popover/95 p-4 text-popover-foreground outline-none backdrop-blur-xl">
      <ContextBreakdown {session} occupancy={o} />
    </Popover.Content>
  </Popover.Portal>
</Popover.Root>
