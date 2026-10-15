<script lang="ts">
  import type { TraceCall } from "../../api/trace";
  import { usageOf } from "./trace-presentation";
  import { formatCost, formatTokens } from "../../lib/format";
  import { router } from "../../state/route.svelte";
  import { settingsStore } from "../../state/settings.svelte";
  import Icon from "../../lib/Icon.svelte";
  let { calls }: { calls: TraceCall[] } = $props();
  const u = $derived(usageOf(calls));
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
</div>
