<script lang="ts">
  import { toolKey, toolResults } from "./tool-results";
  import type { SessionView } from "../state/session.svelte";

  let { view }: { view: SessionView } = $props();

  const THINKING = ["让我想想…", "正在琢磨…", "组织一下语言…", "灵感加载中…", "翻翻脑子里的笔记…", "斟酌措辞中…", "脑细胞加班中…", "在想怎么说更清楚…", "马上就好…"];
  const SLOW = ["这题有点难，多想一会儿…", "还在想，快了快了…", "想得有点深，再等等…", "认真起来了…"];
  /** 按阶段起点取句，同一阶段内不换。 */
  const pick = (lines: string[], since: number | null) => lines[(since ?? 0) % lines.length];

  /** 当前阶段：停止中、等某个工具返回、或等模型开口；`since` 为阶段起点。 */
  const phase = $derived.by(() => {
    if (view.stopping !== null) return { label: "正在停止", since: null };
    const last = view.messages.at(-1);
    // 并行调用的结果逐条到达，故看最近一条回复里还没有结果的调用。
    const reply = view.messages.filter((m) => m.body.kind === "Reply").at(-1);
    if (reply?.body.kind === "Reply") {
      const results = toolResults(view.messages);
      const pending = reply.body.blocks.flatMap((b, index) =>
        "ToolCall" in b && !results.has(toolKey({ reply: reply.id, index })) ? [b.ToolCall] : []);
      if (pending.length)
        return { label: `正在执行 ${pending.map((c) => c.name).join("、")}`, since: reply.created_at };
    }
    return { label: null, since: last?.created_at ?? null };
  });

  let now = $state(Date.now());
  $effect(() => {
    const timer = setInterval(() => (now = Date.now()), 1000);
    return () => clearInterval(timer);
  });
  const seconds = $derived(phase.since === null ? 0 : Math.floor((now - phase.since) / 1000));
  const label = $derived(phase.label ?? pick(seconds >= 20 ? SLOW : THINKING, phase.since));
</script>

<div class="flex items-center gap-2.5 text-sm" role="status">
  <span class="dots flex gap-1" aria-hidden="true"><i></i><i></i><i></i></span>
  <span class="shimmer font-medium">{label}</span>
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
