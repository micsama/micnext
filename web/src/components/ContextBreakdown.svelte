<script lang="ts">
  import { getContextBreakdown } from "../api/client";
  import type { ContextBreakdown, ContextKind, Occupancy } from "../api/types";
  import { CONTEXT_LABEL, contextColor } from "../lib/context-kind";
  import { formatTokens } from "../lib/format";

  let { session, occupancy }: { session: number; occupancy: Occupancy } = $props();

  const SIDE = 20;
  const CELLS = SIDE * SIDE;

  let data = $state<ContextBreakdown | null>(null);
  let error = $state<string | null>(null);
  let hovered = $state<ContextKind | "tool" | null>(null);
  let request = 0;

  // 圆圈随上下文刷新换新的 occupancy，浮层跟着重取，不另维护事件名单。
  $effect(() => {
    void occupancy.used_tokens;
    const id = ++request;
    getContextBreakdown(session).then(
      (v) => { if (id === request) { data = v; error = null; } },
      (e) => { if (id === request) error = e instanceof Error ? e.message : String(e); },
    );
  });

  const o = $derived(data ?? occupancy);
  const percent = $derived(Math.round((o.used_tokens / o.window) * 100));
  const threshold = $derived(Math.floor((o.window * o.trigger_percent) / 100));
  const toCompact = $derived(threshold - o.used_tokens);
  const parts = $derived(data?.parts.filter((p) => p.tokens > 0) ?? []);

  /** 方格的色段：对话拆成消息与工具调用两段，同色，工具段格内加点。 */
  type Segment = { key: ContextKind | "tool"; kind: ContextKind; tokens: number };
  const segments = $derived(
    parts.flatMap((p): Segment[] =>
      p.tool_tokens
        ? ([
            { key: p.kind, kind: p.kind, tokens: p.tokens - p.tool_tokens },
            { key: "tool", kind: p.kind, tokens: p.tool_tokens },
          ] satisfies Segment[]).filter((s) => s.tokens > 0)
        : [{ key: p.kind, kind: p.kind, tokens: p.tokens }],
    ),
  );

  /** 每格一个色段；不足一格的色段按比例自下而上填一截（frac < 1）。null 为空余，"used" 为尚未取到构成时的占用。 */
  type Cell = { seg: Segment; frac: number } | "used" | null;
  const cells = $derived.by((): Cell[] => {
    const used = Math.min(CELLS, Math.round((o.used_tokens / o.window) * CELLS));
    if (segments.length === 0) return Array.from({ length: CELLS }, (_, i) => (i < used ? "used" : null));
    const sum = segments.reduce((a, s) => a + s.tokens, 0);
    const counts = allocate(segments.map((s) => s.tokens), used);
    const out: Cell[] = segments.flatMap((seg, i) => {
      const frac = Math.min(1, Math.max(0.2, (seg.tokens * used) / sum));
      return Array.from({ length: counts[i]! }, () => ({ seg, frac }));
    });
    return out.concat(Array(CELLS - out.length).fill(null));
  });
  const filled = $derived(Math.max(1, cells.filter((c) => c !== null).length));
  const reserveFrom = $derived(Math.round((o.trigger_percent / 100) * CELLS));

  /** 悬停对话时连同其工具段一起高亮。 */
  const lit = (s: Segment) => hovered === null || s.key === hovered || s.kind === hovered;

  /** 最大余数法分格；格数够时每个非零分类至少一格，从最大项匀出。 */
  function allocate(weights: number[], total: number): number[] {
    const sum = weights.reduce((a, b) => a + b, 0);
    const exact = weights.map((w) => (w * total) / sum);
    const out = exact.map(Math.floor);
    const order = exact.map((x, i) => [x - Math.floor(x), i] as const).sort((a, b) => b[0] - a[0]);
    for (let k = 0; k < total - out.reduce((a, b) => a + b, 0); k++) out[order[k]![1]]! += 1;
    if (total >= weights.length) {
      for (let i = 0; i < out.length; i++) {
        if (out[i] === 0) {
          const max = out.indexOf(Math.max(...out));
          out[max]! -= 1;
          out[i] = 1;
        }
      }
    }
    return out;
  }

  /** 返回 background-image；格子底色留给 .cell。 */
  function swatch(s: Segment, frac = 1): string {
    const pct = Math.round(frac * 100);
    const bar = `linear-gradient(to top, ${contextColor(s.kind)} ${pct}%, transparent ${pct}%)`;
    return s.key === "tool" ? `radial-gradient(circle, var(--popover) 0 1.1px, transparent 1.6px), ${bar}` : bar;
  }
  const share = (tokens: number) => `${((tokens / o.window) * 100).toFixed(1)}%`;
</script>

<div class="space-y-3">
  <header class="space-y-0.5">
    <div class="flex items-baseline justify-between gap-3">
      <p class="text-[13px] font-medium text-muted-foreground">上下文</p>
      <p class="text-xs tabular-nums {toCompact <= 0 ? 'text-warning' : 'text-muted-foreground'}">{percent}%</p>
    </div>
    <p class="tabular-nums"><span class="text-xl font-semibold tracking-tight">{formatTokens(o.used_tokens)}</span><span class="text-[13px] text-muted-foreground"> / {formatTokens(o.window)}</span></p>
    <p class="text-xs text-muted-foreground">上次调用 · 分项估算</p>
  </header>

  <div class="flex items-start gap-4">
    <div class="grid w-40 shrink-0 gap-[1.5px] sm:w-48" style:grid-template-columns="repeat({SIDE}, minmax(0, 1fr))" role="img" aria-label="上下文占用 {percent}%">
      {#each cells as cell, i (i)}
        {@const seg = cell !== null && cell !== "used" ? cell.seg : null}
        <span
          class="cell aspect-square rounded-[1.5px] transition-opacity duration-150"
          class:reserve={cell === null && i >= reserveFrom}
          class:fill={cell !== null}
          style:--delay="{Math.round((i / filled) * 300)}ms"
          style:background-image={cell === null ? undefined : cell === "used" ? "linear-gradient(var(--muted-foreground), var(--muted-foreground))" : swatch(cell.seg, cell.frac)}
          style:opacity={seg && !lit(seg) ? 0.25 : undefined}></span>
      {/each}
    </div>

    {#if parts.length > 0}
      <ul class="min-w-0 flex-1 text-[13px]">
        {#each parts as p (p.kind)}
          <li
            class="flex items-center gap-2 rounded-md px-1.5 py-[3px] transition-colors {hovered === p.kind ? 'bg-muted' : ''}"
            title={share(p.tokens)}
            onpointerenter={() => (hovered = p.kind)}
            onpointerleave={() => (hovered = null)}>
            <span class="size-2 shrink-0 rounded-[2px]" style:background-image={swatch({ key: p.kind, kind: p.kind, tokens: 0 })}></span>
            <span class="min-w-0 flex-1 truncate">{CONTEXT_LABEL[p.kind]}</span>
            <span class="tabular-nums text-foreground/80">{formatTokens(p.tokens)}</span>
          </li>
          {#if p.tool_tokens}
            <li
              class="flex items-center gap-2 rounded-md py-[3px] pr-1.5 pl-4 text-xs text-muted-foreground transition-colors {hovered === 'tool' ? 'bg-muted' : ''}"
              title={share(p.tool_tokens)}
              onpointerenter={() => (hovered = "tool")}
              onpointerleave={() => (hovered = null)}>
              <span class="size-2 shrink-0 rounded-[2px]" style:background-image={swatch({ key: "tool", kind: p.kind, tokens: 0 })}></span>
              <span class="min-w-0 flex-1 truncate">其中工具调用</span>
              <span class="tabular-nums">{formatTokens(p.tool_tokens)}</span>
            </li>
          {/if}
        {/each}
      </ul>
    {/if}
  </div>

  {#if error}<p class="text-xs text-destructive">构成读取失败：{error}</p>{/if}

  <p class="border-t border-border pt-2.5 text-xs {toCompact <= 0 ? 'text-warning' : 'text-muted-foreground'}">
    {toCompact <= 0 ? "已达自动压缩阈值" : `距自动压缩 ${formatTokens(toCompact)}`}
  </p>
</div>

<style>
  .cell { background: var(--muted); }
  :global([data-theme="dark"]) .cell { background: var(--secondary); }
  .cell.reserve {
    background-image: repeating-linear-gradient(-45deg, transparent 0 1.5px, color-mix(in srgb, var(--muted-foreground) 30%, transparent) 1.5px 2.5px);
  }
  .cell.fill { animation: cell-in 160ms var(--ease-apple) both; animation-delay: var(--delay); }
  @keyframes cell-in {
    from { opacity: 0; transform: scale(.4); }
  }
  @media (prefers-reduced-motion: reduce) {
    .cell.fill { animation: none; }
  }
</style>
