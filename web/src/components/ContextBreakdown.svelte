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
  let hovered = $state<ContextKind | null>(null);
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

  /** 每格一个分类；null 为空余，"used" 为尚未取到构成时的占用。 */
  const cells = $derived.by(() => {
    const used = Math.min(CELLS, Math.round((o.used_tokens / o.window) * CELLS));
    if (parts.length === 0) return Array.from({ length: CELLS }, (_, i) => (i < used ? "used" : null));
    const counts = allocate(parts.map((p) => p.tokens), used);
    const out: (ContextKind | "used" | null)[] = parts.flatMap((p, i) => Array<ContextKind>(counts[i]!).fill(p.kind));
    return out.concat(Array(CELLS - out.length).fill(null));
  });
  const filled = $derived(Math.max(1, cells.filter((c) => c !== null).length));
  const reserveFrom = $derived(Math.round((o.trigger_percent / 100) * CELLS));

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

  const share = (tokens: number) => `${((tokens / o.window) * 100).toFixed(1)}%`;
</script>

<div class="space-y-3">
  <header class="space-y-0.5">
    <div class="flex items-baseline justify-between gap-3">
      <p class="text-sm font-medium text-muted-foreground">上下文</p>
      <p class="text-xs tabular-nums {toCompact <= 0 ? 'text-warning' : 'text-muted-foreground'}">{percent}%</p>
    </div>
    <p class="tabular-nums"><span class="text-2xl font-semibold tracking-tight">{formatTokens(o.used_tokens)}</span><span class="text-sm text-muted-foreground"> / {formatTokens(o.window)}</span></p>
    <p class="text-xs text-muted-foreground">上次调用 · 分项估算</p>
  </header>

  <div class="grid gap-[2px]" style:grid-template-columns="repeat({SIDE}, minmax(0, 1fr))" role="img" aria-label="上下文占用 {percent}%">
    {#each cells as kind, i (i)}
      <span
        role="presentation"
        class="cell aspect-square rounded-[3px] transition-opacity duration-150"
        class:reserve={kind === null && i >= reserveFrom}
        class:fill={kind !== null}
        style:--delay="{Math.round((i / filled) * 300)}ms"
        style:background={kind === null ? undefined : kind === "used" ? "var(--muted-foreground)" : contextColor(kind)}
        style:opacity={hovered && kind !== hovered && kind !== null ? 0.25 : undefined}
        onpointerenter={() => { if (kind !== null && kind !== "used") hovered = kind; }}
        onpointerleave={() => (hovered = null)}></span>
    {/each}
  </div>

  {#if error}<p class="text-xs text-destructive">构成读取失败：{error}</p>{/if}

  {#if parts.length > 0}
    <ul class="-mx-1.5 text-sm">
      {#each parts as p (p.kind)}
        <li
          class="flex items-center gap-2 rounded-md px-1.5 py-1 transition-colors {hovered === p.kind ? 'bg-muted' : ''}"
          title={share(p.tokens)}
          onpointerenter={() => (hovered = p.kind)}
          onpointerleave={() => (hovered = null)}>
          <span class="size-2 shrink-0 rounded-full" style:background={contextColor(p.kind)}></span>
          <span class="flex-1">{CONTEXT_LABEL[p.kind]}</span>
          <span class="tabular-nums text-foreground/80">{formatTokens(p.tokens)}</span>
        </li>
        {#if p.tool_tokens}
          <li class="flex items-center gap-2 px-1.5 pb-1 pl-5.5 text-xs text-muted-foreground" title={share(p.tool_tokens)}>
            <span class="flex-1">其中工具调用与结果</span>
            <span class="tabular-nums">{formatTokens(p.tool_tokens)}</span>
          </li>
        {/if}
      {/each}
    </ul>
  {/if}

  <p class="border-t border-border pt-2.5 text-xs {toCompact <= 0 ? 'text-warning' : 'text-muted-foreground'}">
    {toCompact <= 0 ? "已达自动压缩阈值" : `距自动压缩 ${formatTokens(toCompact)}`}
  </p>
</div>

<style>
  .cell { background: var(--muted); }
  :global([data-theme="dark"]) .cell { background: var(--secondary); }
  .cell.reserve {
    background-image: repeating-linear-gradient(-45deg, transparent 0 2px, color-mix(in srgb, var(--muted-foreground) 28%, transparent) 2px 3px);
  }
  .cell.fill { animation: cell-in 160ms var(--ease-apple) both; animation-delay: var(--delay); }
  @keyframes cell-in {
    from { opacity: 0; transform: scale(.4); }
  }
  @media (prefers-reduced-motion: reduce) {
    .cell.fill { animation: none; }
  }
</style>
