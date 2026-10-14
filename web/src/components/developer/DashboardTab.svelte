<script lang="ts">
  import { getDashboard } from "../../api/client";
  import type { CallStat, Dashboard } from "../../api/trace";
  import { cn } from "$lib/utils";
  import { formatCost, formatDuration, formatTokens } from "../../lib/format";
  import Icon from "../../lib/Icon.svelte";

  let { active }: { active: boolean } = $props();

  const RANGES = [7, 30, 90] as const;
  let days = $state<number>(7);
  let data = $state.raw<Dashboard | null>(null);
  let error = $state<string | null>(null);
  let loaded = false;
  let request = 0;

  async function refresh() {
    const current = ++request;
    try {
      error = null;
      const result = await getDashboard(days);
      if (current === request) data = result;
    } catch (e) {
      if (current === request) error = e instanceof Error ? e.message : String(e);
    }
  }

  $effect(() => {
    if (active && !loaded) {
      loaded = true;
      void refresh();
    }
  });

  function setDays(n: number) {
    days = n;
    void refresh();
  }

  const totals = $derived(data?.days.reduce((s, d) => ({ runs: s.runs + d.runs, completed: s.completed + d.completed_runs, calls: s.calls + d.calls, cost: s.cost + d.cost, unknown: s.unknown + d.unknown_cost }), { runs: 0, completed: 0, calls: 0, cost: 0, unknown: 0 }));
  const PURPOSE = { conversation: "对话", compaction: "压缩", title: "标题" } as const;

  const rate = (n: number, total: number) => total === 0 ? "—" : `${(n / total * 100).toFixed(1)}%`;

  const tokens = (s: CallStat) => `${formatTokens(s.input_tokens)} / ${formatTokens(s.output_tokens)}`;
</script>

{#snippet usageCells(s: CallStat)}
  <td class="px-2 py-1 text-right">{s.calls}</td>
  <td class={cn("px-2 py-1 text-right", s.failed_calls > 0 && "text-destructive")}>{s.failed_calls}（{rate(s.failed_calls, s.calls)}）</td>
  <td class="px-2 py-1 text-right" title="缓存读 {s.cache_read_tokens} · 缓存写 {s.cache_write_tokens}">
    {tokens(s)}{s.unknown_usage > 0 ? ` +${s.unknown_usage} 次未报` : ""}
  </td>
  <td class="px-2 py-1 text-right">{formatCost(s.cost)}{s.unknown_cost > 0 ? ` +${s.unknown_cost} 次未知` : ""}</td>
  <td class="px-2 py-1 text-right">{s.avg_ms === null ? "—" : formatDuration(s.avg_ms)}</td>
{/snippet}

<div class="min-h-0 flex-1 overflow-y-auto bg-sidebar p-5 text-sm">
  <div class="mb-3 flex items-center gap-1">
    {#each RANGES as n (n)}
      <button
        type="button"
        class={cn("rounded-md px-2 py-0.5 text-xs", n === days ? "bg-accent font-medium" : "text-muted-foreground hover:text-foreground")}
        onclick={() => setDays(n)}>近 {n} 天</button>
    {/each}
    <button type="button" class="ml-1 rounded p-1 text-muted-foreground hover:bg-accent hover:text-foreground" aria-label="刷新" onclick={refresh}>
      <Icon name="refresh" class="size-3.5" />
    </button>
  </div>
  {#if error}<p class="text-destructive">{error}</p>{/if}
  {#if data}
    {#if totals}<div class="mb-5 grid grid-cols-2 gap-3 lg:grid-cols-4">
      {#each [{ label: "运行次数", value: String(totals.runs), note: `近 ${days} 天` }, { label: "成功率", value: rate(totals.completed, totals.runs), note: `${totals.completed} 轮完成` }, { label: "模型调用", value: String(totals.calls), note: "含重试与旁路" }, { label: "估算费用", value: formatCost(totals.cost), note: totals.unknown ? `${totals.unknown} 次费用未知` : "按当前单价估算" }] as metric}
        <div class="rounded-xl border bg-background px-5 py-4"><p class="text-xs text-muted-foreground">{metric.label}</p><p class="my-2 text-2xl font-semibold tracking-tight tabular-nums">{metric.value}</p><p class="text-xs text-muted-foreground">{metric.note}</p></div>
      {/each}
    </div>{/if}
    <div class="rounded-xl border bg-background p-4">
    <h3 class="mb-1 font-medium">按日</h3>
    <table class="mb-6 w-full">
      <thead class="text-xs text-muted-foreground">
        <tr>
          <th class="px-2 py-1 text-left font-normal">日期</th>
          <th class="px-2 py-1 text-right font-normal">运行 / 成功率</th>
          <th class="px-2 py-1 text-right font-normal">调用</th>
          <th class="px-2 py-1 text-right font-normal">失败 / 失败率</th>
          <th class="px-2 py-1 text-right font-normal">输入 / 输出</th>
          <th class="px-2 py-1 text-right font-normal">费用</th>
          <th class="px-2 py-1 text-right font-normal">平均耗时</th>
        </tr>
      </thead>
      <tbody>
        {#each data.days as d (d.day)}
          <tr class="border-t">
            <td class="px-2 py-1">{d.day}</td>
            <td class="px-2 py-1 text-right">{d.runs} / {rate(d.completed_runs, d.runs)}</td>
            {@render usageCells(d)}
          </tr>
        {/each}
      </tbody>
    </table>

    <h3 class="mb-1 font-medium">按模型</h3>
    <table class="mb-6 w-full">
      <thead class="text-xs text-muted-foreground">
        <tr>
          <th class="px-2 py-1 text-left font-normal">模型</th>
          <th class="px-2 py-1 text-right font-normal">调用</th>
          <th class="px-2 py-1 text-right font-normal">失败 / 失败率</th>
          <th class="px-2 py-1 text-right font-normal">输入 / 输出</th>
          <th class="px-2 py-1 text-right font-normal">费用</th>
          <th class="px-2 py-1 text-right font-normal">平均耗时</th>
        </tr>
      </thead>
      <tbody>
        {#each data.models as m (`${m.purpose}:${m.model_id}:${m.model}`)}
          <tr class="border-t">
            <td class="px-2 py-1">{m.model}<span class="text-xs text-muted-foreground"> · {PURPOSE[m.purpose]}</span></td>
            {@render usageCells(m)}
          </tr>
        {/each}
      </tbody>
    </table>

    <h3 class="mb-1 font-medium">按工具</h3>
    <table class="w-full">
      <thead class="text-xs text-muted-foreground">
        <tr>
          <th class="px-2 py-1 text-left font-normal">工具</th>
          <th class="px-2 py-1 text-right font-normal">调用</th>
          <th class="px-2 py-1 text-right font-normal">失败 / 失败率</th>
          <th class="px-2 py-1 text-right font-normal">平均耗时</th>
        </tr>
      </thead>
      <tbody>
        {#each data.tools as t (t.name)}
          <tr class="border-t">
            <td class="px-2 py-1">{t.name}</td>
            <td class="px-2 py-1 text-right">{t.calls}</td>
            <td class={cn("px-2 py-1 text-right", t.errors > 0 && "text-destructive")}>{t.errors}（{rate(t.errors, t.calls)}）</td>
            <td class="px-2 py-1 text-right">{t.avg_ms === null ? "—" : formatDuration(t.avg_ms)}</td>
          </tr>
        {/each}
      </tbody>
    </table>
    </div>
  {/if}
</div>
