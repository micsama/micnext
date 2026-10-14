<script lang="ts">
  import type { TraceCall } from "../../api/trace";
  import { usageOf } from "./trace-presentation";
  import { formatCost, formatTokens } from "../../lib/format";
  let { calls }: { calls: TraceCall[] } = $props();
  const u = $derived(usageOf(calls));
</script>
<div title={u.unknown ? `${u.unknown} 次调用用量未知，展示已知部分` : `输入 ${u.input} · 输出 ${u.output}`}><span>Token</span><strong>{u.known ? formatTokens(u.input + u.output) : "未知"}</strong><small>{u.known ? `入 ${formatTokens(u.input)} · 出 ${formatTokens(u.output)}` : "上游未报告"}{u.unknown && u.known ? ` · 另 ${u.unknown} 次未知` : ""}</small></div>
<div title="缓存读取 / 输入 token"><span>缓存命中</span><strong>{u.cacheKnown && u.input > 0 ? `${Math.round(u.cache / u.input * 100)}%` : "—"}</strong><small>{u.cacheKnown ? `${formatTokens(u.cache)} / ${formatTokens(u.input)}` : "上游未报告"}</small></div>
<div title="按当前单价估算"><span>费用</span><strong>{u.cost === null ? "未知" : `≈ ${formatCost(u.cost)}`}</strong><small>{u.unknownCost ? `未知 ${u.unknownCost} 次${u.cost !== null ? " · 已知部分" : ""}` : "当前单价"}</small></div>
