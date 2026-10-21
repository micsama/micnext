<script lang="ts">
  import type { SessionView } from "../state/session.svelte";

  let { view }: { view: SessionView } = $props();

  /** 当前阶段：停止中、等某个工具返回、或等模型开口；`since` 为阶段起点。 */
  const phase = $derived.by(() => {
    if (view.stopping !== null) return { label: "正在停止", since: null };
    const last = view.messages.at(-1);
    // 并行调用的结果逐条到达，故看最近一条回复里还没有结果的调用。
    const reply = view.messages.filter((m) => m.body.kind === "Reply").at(-1);
    if (reply?.body.kind === "Reply") {
      const pending = reply.body.blocks.flatMap((b) => ("ToolCall" in b ? [b.ToolCall] : []))
        .filter((c) => !view.messages.some((m) => m.body.kind === "ToolResult" && m.body.tool_call_id === c.id));
      if (pending.length)
        return { label: `正在执行 ${pending.map((c) => c.name).join("、")}`, since: reply.created_at };
    }
    return { label: "等待模型回复", since: last?.created_at ?? null };
  });

  let now = $state(Date.now());
  $effect(() => {
    const timer = setInterval(() => (now = Date.now()), 1000);
    return () => clearInterval(timer);
  });
  const seconds = $derived(phase.since === null ? 0 : Math.floor((now - phase.since) / 1000));
</script>

<div class="flex items-center gap-2.5 text-sm" role="status">
  <span class="dots flex gap-1" aria-hidden="true"><i></i><i></i><i></i></span>
  <span class="shimmer font-medium">{phase.label}</span>
  {#if seconds >= 2}<span class="text-xs text-muted-foreground tabular-nums">{seconds}s</span>{/if}
</div>

<style>
  .dots i {
    width: 6px;
    height: 6px;
    border-radius: 9999px;
    background: var(--primary);
    animation: hop 1.2s ease-in-out infinite;
  }
  .dots i:nth-child(2) { animation-delay: .15s; }
  .dots i:nth-child(3) { animation-delay: .3s; }
  @keyframes hop {
    0%, 60%, 100% { transform: translateY(0); opacity: .35; }
    30% { transform: translateY(-4px); opacity: 1; }
  }
  .shimmer {
    background: linear-gradient(90deg, var(--muted-foreground) 40%, var(--foreground) 50%, var(--muted-foreground) 60%) 0 0 / 300% 100%;
    background-clip: text;
    color: transparent;
    animation: sweep 2.4s linear infinite;
  }
  @keyframes sweep { from { background-position: 100% 0; } to { background-position: 0 0; } }
  @media (prefers-reduced-motion: reduce) {
    .dots i, .shimmer { animation: none; }
    .dots i { opacity: .6; }
  }
</style>
