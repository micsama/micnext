<script lang="ts">
  import type { SessionUsage, TraceCall } from "../../api/trace";
  import { usageOf } from "./trace-presentation";
  import { formatCost, formatTokens } from "../../lib/format";
  import { router } from "../../state/route.svelte";
  import { settingsStore } from "../../state/settings.svelte";
  import Icon from "../../lib/Icon.svelte";
  /** `tree`：本会话加全部后代的合计，卡底附一行。 */
  let { calls = [], summary, tree = null }: { calls?: TraceCall[]; summary?: SessionUsage; tree?: { descendants: number; usage: SessionUsage } | null } = $props();
  const treeTip = $derived.by(() => {
    if (!tree) return "";
    const t = tree.usage;
    return [
      `本会话与 ${tree.descendants} 个子 agent 合计`,
      `输入 ${formatTokens(t.input_tokens)} · 缓存读取 ${t.cache_complete ? formatTokens(t.cache_read_tokens) : "—"} · 缓存写入 ${formatTokens(t.cache_write_tokens)}`,
      `输出 ${formatTokens(t.output_tokens)}${t.reasoning_tokens === null ? "" : ` · 推理 ${formatTokens(t.reasoning_tokens)}`}`,
      t.unknown_usage ? `另 ${t.unknown_usage} 次用量未知` : "",
      t.unknown_cost ? `${t.unknown_cost} 次费用未知` : "",
    ].filter(Boolean).join("\n");
  });
  const u = $derived(summary ? {
    input: summary.input_tokens, output: summary.output_tokens, cache: summary.cache_read_tokens,
    cacheWrite: summary.cache_write_tokens, reasoning: summary.reasoning_tokens ?? 0,
    reasoningKnown: summary.reasoning_tokens !== null, known: summary.known_usage,
    unknown: summary.unknown_usage, cacheKnown: summary.cache_complete,
    cost: summary.cost, unknownCost: summary.unknown_cost,
  } : usageOf(calls));
  const hit = $derived(u.cacheKnown && u.input > 0 ? Math.round(u.cache / u.input * 100) : null);
  const models = $derived([...new Set(calls.map((c) => c.model_id).filter((id) => id !== null))]);
  const pct = (part: number, whole: number) => whole > 0 ? `${Math.min(100, part / whole * 100)}%` : "0%";

  // 与 mic-store `call_cost` 同一口径：缓存读要上报；缓存写未报按 0、无单价按输入价。
  function whyUnknown(c: TraceCall): string {
    if (!c.usage) return "上游未报告用量";
    const { input_tokens, output_tokens, cache_read_tokens: read } = c.usage;
    if (read === null) return "上游未报告缓存读取用量";
    const m = c.model_id === null ? undefined : settingsStore.model(c.model_id);
    if (!m) return `${c.model} 已删除，无单价`;
    const missing = ([["输入", input_tokens - read, m.price.input], ["缓存读取", read, m.price.cache_read], ["输出", output_tokens, m.price.output]] as const)
      .filter(([, n, p]) => n > 0 && p === null).map(([label]) => label);
    return `${c.model} 未设${missing.join("、")}单价`;
  }
  const costTip = $derived.by(() => {
    const reasons = new Map<string, number>();
    for (const c of calls.filter((c) => c.cost === null)) { const r = whyUnknown(c); reasons.set(r, (reasons.get(r) ?? 0) + 1); }
    if (summary?.unknown_cost) reasons.set("调用用量或单价未知", summary.unknown_cost);
    const head = u.cost === null ? "费用未知" : `按当前单价估算 ${formatCost(u.cost)}${reasons.size ? "（仅已知部分）" : ""}`;
    return [head, ...[...reasons].map(([r, n]) => `· ${r}${calls.length > 1 ? `（${n} 次）` : ""}`)].join("\n");
  });
</script>
<div class="metric" title={u.unknown ? `${u.unknown} 次调用用量未知，展示已知部分` : undefined}>
  <div class="m-head"><span class="m-label"><Icon name="layers" class="size-3.5" />Token</span><span class="m-val">{u.known ? formatTokens(u.input + u.output) : "未知"}</span></div>
  {#if u.known}
    <div class="m-row"><i class="dot s-in"></i><span>输入</span><b>{formatTokens(u.input)}</b></div>
    {#if u.cacheKnown}<div class="m-ratio" title="缓存读取占输入 {hit}%"><i class="s-cache" style="width:{pct(u.cache, u.input)}"></i></div>{/if}
    <div class="m-row sub" title="缓存读取 / 输入"><i class="dot s-cache"></i><span>缓存命中{#if hit !== null}<span class={hit >= 50 ? "cap c-tool" : "cap"}>{hit}%</span>{/if}</span><b>{u.cacheKnown ? formatTokens(u.cache) : "—"}</b></div>
    <div class="m-row"><i class="dot s-out"></i><span>输出</span><b>{formatTokens(u.output)}</b></div>
    {#if u.reasoningKnown}
      <div class="m-ratio" title="推理占输出 {Math.round(u.reasoning / Math.max(u.output, 1) * 100)}%"><i class="s-think" style="width:{pct(u.reasoning, u.output)}"></i></div>
      <div class="m-row sub"><i class="dot s-think"></i><span>推理</span><b>{formatTokens(u.reasoning)}</b></div>
    {/if}
    {#if u.unknown}<p class="m-note">另 {u.unknown} 次用量未知</p>{/if}
  {:else}<p class="m-note">上游未报告用量</p>{/if}
  <div class="m-row m-cost" title={costTip}>
    <i></i><span>费用</span>
    <b class:muted={u.cost === null} class="tip">{u.cost === null ? "未知" : `≈ ${formatCost(u.cost)}`}{#if u.unknownCost && u.cost !== null}<small> +{u.unknownCost} 次未知</small>{/if}</b>
    {#if models.length === 1}<button type="button" class="iconbtn xs" title="设置该模型单价" aria-label="设置该模型单价" onclick={() => router.go(`/settings/models/${models[0]}`)}><Icon name="settings" class="size-3.5" /></button>{/if}
  </div>
  {#if tree}
    {@const t = tree.usage}
    <div class="tree-sum" title={treeTip}><span>含 {tree.descendants} 个子 agent 合计</span><b>{t.known_usage ? `${formatTokens(t.input_tokens + t.output_tokens)} tok` : "用量未知"}{t.cost === null ? "" : ` · ≈${formatCost(t.cost)}`}</b></div>
  {/if}
</div>
