<script lang="ts">
  import "./trace.css";
  import "./dashboard.css";
  import { getDashboard } from "../../api/client";
  import type { CallStat, Dashboard, DayStat } from "../../api/trace";
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
    hot = null;
    void refresh();
  }

  const PURPOSE = { conversation: "对话", compaction: "压缩", title: "标题" } as const;
  const rate = (n: number, total: number) => (total === 0 ? "—" : `${((n / total) * 100).toFixed(n === total || n === 0 ? 0 : 1)}%`);
  const cacheHit = (s: CallStat) => (s.input_tokens === 0 ? "—" : rate(s.cache_read_tokens, s.input_tokens));

  // 服务端只回有数据的日子；按范围补齐空日，图上才连续。
  const series = $derived.by((): DayStat[] => {
    if (!data) return [];
    const key = (d: Date) => `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}-${String(d.getDate()).padStart(2, "0")}`;
    const by = new Map(data.days.map((d) => [d.day, d]));
    const today = key(new Date());
    const last = data.days.at(-1)?.day;
    const end = new Date(`${last && last > today ? last : today}T00:00:00`);
    return Array.from({ length: days }, (_, i) => {
      const d = new Date(end);
      d.setDate(end.getDate() - (days - 1 - i));
      const k = key(d);
      return by.get(k) ?? { day: k, runs: 0, completed_runs: 0, calls: 0, failed_calls: 0, input_tokens: 0, output_tokens: 0, cache_read_tokens: 0, cache_write_tokens: 0, unknown_usage: 0, cost: 0, unknown_cost: 0, avg_ms: null };
    });
  });

  const total = $derived(
    series.reduce(
      (s, d) => ({
        runs: s.runs + d.runs,
        completed: s.completed + d.completed_runs,
        calls: s.calls + d.calls,
        failed: s.failed + d.failed_calls,
        input: s.input + d.input_tokens,
        output: s.output + d.output_tokens,
        cacheRead: s.cacheRead + d.cache_read_tokens,
        unknownUsage: s.unknownUsage + d.unknown_usage,
        cost: s.cost + d.cost,
        unknownCost: s.unknownCost + d.unknown_cost,
      }),
      { runs: 0, completed: 0, calls: 0, failed: 0, input: 0, output: 0, cacheRead: 0, unknownUsage: 0, cost: 0, unknownCost: 0 },
    ),
  );

  type Metric = "calls" | "runs" | "tokens" | "cost";
  const METRICS: { id: Metric; label: string }[] = [
    { id: "calls", label: "调用" },
    { id: "runs", label: "运行" },
    { id: "tokens", label: "Token" },
    { id: "cost", label: "费用" },
  ];
  let metric = $state<Metric>("calls");
  let hot = $state<number | null>(null);

  // 每根柱拆成至多两段：[主段, 次段]，次段在上。
  const parts = (d: DayStat, m: Metric): [number, number] =>
    m === "calls" ? [d.calls - d.failed_calls, d.failed_calls]
    : m === "runs" ? [d.completed_runs, d.runs - d.completed_runs]
    : m === "tokens" ? [d.input_tokens, d.output_tokens]
    : [d.cost, 0];
  const LEGEND: Record<Metric, [string, string, string] | null> = {
    calls: ["成功", "失败", "s-bad"],
    runs: ["完成", "失败或中断", "s-warn"],
    tokens: ["输入", "输出", "s-b"],
    cost: null,
  };
  const peak = $derived(Math.max(0, ...series.map((d) => { const [a, b] = parts(d, metric); return a + b; })));
  const pct = (v: number) => (peak === 0 ? 0 : (v / peak) * 100);
  const tick = (i: number) => {
    const step = days <= 7 ? 1 : days <= 30 ? 5 : 15;
    return (days - 1 - i) % step === 0 ? series[i]!.day.slice(5).replace("-", "/") : "";
  };
  const focus = $derived(hot === null ? null : series[hot] ?? null);

  const models = $derived(data ? [...data.models].sort((a, b) => b.cost - a.cost || b.calls - a.calls) : []);
  const maxCost = $derived(Math.max(0, ...models.map((m) => m.cost)));
  const tools = $derived(data ? [...data.tools].sort((a, b) => b.calls - a.calls) : []);
  const maxTool = $derived(Math.max(0, ...tools.map((t) => t.calls)));
  const sparkOf = (f: (d: DayStat) => number) => { const v = series.map(f); const m = Math.max(0, ...v); return v.map((x) => (m === 0 ? 0 : (x / m) * 100)); };
</script>

{#snippet spark(values: number[])}
  <div class="spark" aria-hidden="true">{#each values as v, i (i)}<i class:zero={v === 0} style="height:{Math.max(v, 4)}%"></i>{/each}</div>
{/snippet}

{#snippet cost(s: CallStat)}
  {formatCost(s.cost)}{#if s.unknown_cost > 0}<span class="unk" title="{s.unknown_cost} 次调用缺用量或单价，未计入">+{s.unknown_cost} 未知</span>{/if}
{/snippet}

<div class="trace-board">
  <div class="wrap">
    <div class="bar-top">
      <div class="seg" role="group" aria-label="时间范围">
        {#each RANGES as n (n)}<button type="button" class:on={n === days} onclick={() => setDays(n)}>近 {n} 天</button>{/each}
      </div>
      <button type="button" class="iconbtn" aria-label="刷新" title="刷新" onclick={refresh}><Icon name="refresh" class="size-3.5" /></button>
      <span class="note">费用按当前单价估算 · 美元</span>
    </div>

    {#if error}<p class="bad text-sm">{error}</p>{/if}

    {#if data}
      <div class="kpis">
        <div class="panel kpi">
          <div class="kpi-label">运行</div>
          <div class="kpi-val num">{total.runs}</div>
          <div class="kpi-sub">完成 {rate(total.completed, total.runs)}{#if total.runs > total.completed}<span class="bad"> · {total.runs - total.completed} 未完成</span>{/if}</div>
          {@render spark(sparkOf((d) => d.runs))}
        </div>
        <div class="panel kpi">
          <div class="kpi-label">模型调用</div>
          <div class="kpi-val num">{total.calls}</div>
          <div class="kpi-sub">{#if total.failed > 0}<span class="bad">失败 {total.failed}（{rate(total.failed, total.calls)}）</span>{:else}含重试与旁路{/if}</div>
          {@render spark(sparkOf((d) => d.calls))}
        </div>
        <div class="panel kpi">
          <div class="kpi-label">Token</div>
          <div class="kpi-val num">{formatTokens(total.input + total.output)}</div>
          <div class="kpi-sub" title="缓存读取占输入的比例">缓存命中 {total.input === 0 ? "—" : rate(total.cacheRead, total.input)}{#if total.unknownUsage > 0} · {total.unknownUsage} 次未报{/if}</div>
          {@render spark(sparkOf((d) => d.input_tokens + d.output_tokens))}
        </div>
        <div class="panel kpi">
          <div class="kpi-label">估算费用</div>
          <div class="kpi-val num">{formatCost(total.cost)}</div>
          <div class="kpi-sub">{#if total.unknownCost > 0}<span title="缺用量或单价的调用未计入">{total.unknownCost} 次未计入</span>{:else}日均 {formatCost(total.cost / days)}{/if}</div>
          {@render spark(sparkOf((d) => d.cost))}
        </div>
      </div>

      <section class="panel">
        <div class="panel-head">
          <div class="seg" role="group" aria-label="图表指标">
            {#each METRICS as m (m.id)}<button type="button" class:on={metric === m.id} onclick={() => (metric = m.id)}>{m.label}</button>{/each}
          </div>
          {#if LEGEND[metric]}{@const [a, b, cls] = LEGEND[metric]!}
            <span class="legend hide-sm"><i class="s-a"></i>{a}</span><span class="legend hide-sm"><i class={cls}></i>{b}</span>
          {/if}
          <div class="readout num">
            {#if focus}
              <span><b>{focus.day}</b></span>
              <span>运行 <b>{focus.runs}</b></span>
              <span>调用 <b>{focus.calls}</b>{#if focus.failed_calls}<span class="bad"> · 失败 {focus.failed_calls}</span>{/if}</span>
              <span>Token <b>{formatTokens(focus.input_tokens)}</b> / <b>{formatTokens(focus.output_tokens)}</b></span>
              <span>费用 <b>{formatCost(focus.cost)}</b></span>
            {:else}
              <span class="faint">悬停柱子查看当天</span>
            {/if}
          </div>
        </div>
        {#if total.runs + total.calls === 0}
          <p class="blank">近 {days} 天没有运行</p>
        {:else}
          <div class="chart" role="img" aria-label="按日{METRICS.find((m) => m.id === metric)!.label}" onmouseleave={() => (hot = null)}>
            {#each series as d, i (d.day)}
              {@const [a, b] = parts(d, metric)}
              <div class="col" class:hot={hot === i} role="presentation" onmouseenter={() => (hot = i)}>
                {#if b > 0}<i class={LEGEND[metric]?.[2]} style="height:{pct(b)}%"></i>{/if}
                {#if a > 0}<i class="s-a" style="height:{pct(a)}%"></i>{/if}
              </div>
            {/each}
          </div>
          <div class="axis num">{#each series as d, i (d.day)}<span>{tick(i)}</span>{/each}</div>
        {/if}
      </section>

      <section class="panel">
        <div class="panel-head"><h2>模型</h2><span class="faint text-xs">按费用排序</span></div>
        {#if models.length === 0}<p class="blank">暂无调用</p>{:else}
          <div class="scroll">
            <table>
              <thead><tr><th>模型</th><th>调用</th><th>失败率</th><th class="hide-sm">输入 / 输出</th><th class="hide-sm" title="缓存读取占输入的比例">缓存命中</th><th>费用</th><th class="hide-sm">平均耗时</th></tr></thead>
              <tbody>
                {#each models as m (`${m.purpose}:${m.model_id}:${m.model}`)}
                  <tr>
                    <td><span class="name"><span title={m.model}>{m.model}</span>{#if m.purpose !== "conversation"}<span class="cap">{PURPOSE[m.purpose]}</span>{/if}</span></td>
                    <td>{m.calls}</td>
                    <td class:bad={m.failed_calls > 0} title="失败 {m.failed_calls} 次">{rate(m.failed_calls, m.calls)}</td>
                    <td class="hide-sm" title="缓存读 {m.cache_read_tokens} · 缓存写 {m.cache_write_tokens}">{formatTokens(m.input_tokens)} / {formatTokens(m.output_tokens)}{#if m.unknown_usage > 0}<span class="unk">+{m.unknown_usage} 未报</span>{/if}</td>
                    <td class="hide-sm">{cacheHit(m)}</td>
                    <td><span class="share"><span>{@render cost(m)}</span><i style="--w:{maxCost === 0 ? 0 : (m.cost / maxCost) * 100}%"></i></span></td>
                    <td class="hide-sm">{m.avg_ms === null ? "—" : formatDuration(m.avg_ms)}</td>
                  </tr>
                {/each}
              </tbody>
            </table>
          </div>
        {/if}
      </section>

      <section class="panel">
        <div class="panel-head"><h2>工具</h2><span class="faint text-xs">按调用次数排序</span></div>
        {#if tools.length === 0}<p class="blank">暂无工具调用</p>{:else}
          <div class="scroll">
            <table>
              <thead><tr><th>工具</th><th>调用</th><th>报错率</th><th>平均耗时</th></tr></thead>
              <tbody>
                {#each tools as t (t.name)}
                  <tr>
                    <td><span class="mono">{t.name}</span></td>
                    <td><span class="share"><span>{t.calls}</span><i style="--w:{maxTool === 0 ? 0 : (t.calls / maxTool) * 100}%"></i></span></td>
                    <td class:bad={t.errors > 0} title="报错 {t.errors} 次">{rate(t.errors, t.calls)}</td>
                    <td>{t.avg_ms === null ? "—" : formatDuration(t.avg_ms)}</td>
                  </tr>
                {/each}
              </tbody>
            </table>
          </div>
        {/if}
      </section>

      <details class="panel">
        <summary class="panel-head"><span class="chev"><Icon name="chevron" class="size-3.5" /></span><h2>按日明细</h2></summary>
        <div class="scroll">
          <table>
            <thead><tr><th>日期</th><th>运行</th><th>完成率</th><th>调用</th><th>失败</th><th class="hide-sm">输入 / 输出</th><th>费用</th><th class="hide-sm">平均耗时</th></tr></thead>
            <tbody>
              {#each [...series].reverse().filter((d) => d.runs + d.calls > 0) as d (d.day)}
                <tr>
                  <td>{d.day}</td>
                  <td>{d.runs}</td>
                  <td class:bad={d.completed_runs < d.runs}>{rate(d.completed_runs, d.runs)}</td>
                  <td>{d.calls}</td>
                  <td class:bad={d.failed_calls > 0}>{d.failed_calls}</td>
                  <td class="hide-sm">{formatTokens(d.input_tokens)} / {formatTokens(d.output_tokens)}</td>
                  <td>{@render cost(d)}</td>
                  <td class="hide-sm">{d.avg_ms === null ? "—" : formatDuration(d.avg_ms)}</td>
                </tr>
              {/each}
            </tbody>
          </table>
        </div>
      </details>
    {/if}
  </div>
</div>
